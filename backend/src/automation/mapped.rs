use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use regex::Regex;
use thiserror::Error;
use walkdir::WalkDir;

use crate::config::{
    MappingDefinition, MappingRow, ReportLayoutProfile, TaskDefinition, TransformDefinition,
};

use super::csv_data::{
    csv_fingerprint, required_number, round_to, wait_for_stable_csv, CsvDataError, CsvTable,
};
use super::processors::{FailureDetail, ProcessorResult, ProcessorTaskOutput};
use super::reports::{patch_workbooks_transactional, CellUpdate, ReportError, WorkbookPatch};

#[derive(Debug, Error)]
enum MappedProcessorError {
    #[error("{0}")]
    Csv(CsvDataError),
    #[error("{0}")]
    CsvNotReady(String),
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    MissingCsv(String),
    #[error("{0}")]
    Report(#[from] ReportError),
}

impl From<CsvDataError> for MappedProcessorError {
    fn from(error: CsvDataError) -> Self {
        if error.is_transient_file_access() {
            MappedProcessorError::CsvNotReady(format!(
                "CSV is still being written by ATS; waiting for it to unlock. {error}"
            ))
        } else {
            MappedProcessorError::Csv(error)
        }
    }
}

type MappedAttempt<T> = Result<T, MappedProcessorError>;

const CSV_STABLE_FOR: Duration = Duration::from_millis(400);
const CSV_MAX_WAIT: Duration = Duration::from_millis(1_500);

#[derive(Debug, Clone)]
struct CsvSource {
    path: PathBuf,
    fingerprint: String,
}

pub fn process_task(
    profile: &ReportLayoutProfile,
    task: &TaskDefinition,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
) -> ProcessorResult {
    let output = compute_task(profile, task, unit_folder, already_processed_fingerprint);

    if output.result.state == "pass" && !output.patches.is_empty() {
        if let Err(error) = patch_workbooks_transactional(&output.patches) {
            return mapped_error_result(MappedProcessorError::Report(error));
        }
    }

    output.result
}

pub fn compute_task(
    profile: &ReportLayoutProfile,
    task: &TaskDefinition,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
) -> ProcessorTaskOutput {
    match process_task_inner(profile, task, unit_folder, already_processed_fingerprint) {
        Ok(output) => output,
        Err(error) => ProcessorTaskOutput::result_only(mapped_error_result(error)),
    }
}

fn mapped_error_result(error: MappedProcessorError) -> ProcessorResult {
    match error {
        MappedProcessorError::CsvNotReady(message) => ProcessorResult {
            state: "waiting".to_string(),
            code: 2,
            failure: None,
            message,
            log: Vec::new(),
            report_path: None,
            print_report_path: None,
            source_csv_path: None,
            csv_fingerprint: None,
        },
        MappedProcessorError::MissingCsv(message) => ProcessorResult {
            state: "warning".to_string(),
            code: 3,
            failure: Some(FailureDetail {
                title: "CSV Not Found".to_string(),
                message: message.clone(),
                location: None,
            }),
            message,
            log: Vec::new(),
            report_path: None,
            print_report_path: None,
            source_csv_path: None,
            csv_fingerprint: None,
        },
        error => {
            let message = error.to_string();
            ProcessorResult {
                state: "fail".to_string(),
                code: 1,
                failure: Some(FailureDetail {
                    title: "Mapped Processing Error".to_string(),
                    message: message.clone(),
                    location: None,
                }),
                message,
                log: Vec::new(),
                report_path: None,
                print_report_path: None,
                source_csv_path: None,
                csv_fingerprint: None,
            }
        }
    }
}

fn process_task_inner(
    profile: &ReportLayoutProfile,
    task: &TaskDefinition,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
) -> MappedAttempt<ProcessorTaskOutput> {
    if task.mappings.is_empty() {
        return Err(MappedProcessorError::Config(format!(
            "task '{}' has no mappings to execute",
            task.id
        )));
    }

    let csv_source = require_csv_by_pattern(unit_folder, &task.csv_pattern, &task.label)?;
    if let Some(result) = idempotent_success(task, &csv_source, already_processed_fingerprint) {
        return Ok(ProcessorTaskOutput::result_only(result));
    }

    let table = CsvTable::read(&csv_source.path)?;
    let mut log = vec![
        format!("[mapped] {}", task.label),
        format!("[mapped] CSV: {}", csv_source.path.display()),
    ];
    let mut updates_by_workbook = HashMap::<String, Vec<CellUpdate>>::new();

    for mapping in &task.mappings {
        match cell_update_from_mapping(mapping, &table) {
            Ok(Some((workbook, update, value))) => {
                log.push(format!(
                    "[mapped]   {}!{} <- {} = {}",
                    update.sheet, update.cell, mapping.label, value
                ));
                updates_by_workbook
                    .entry(workbook)
                    .or_default()
                    .push(update);
            }
            Ok(None) => {
                log.push(format!(
                    "[mapped]   {} skipped because optional source was blank",
                    mapping.label
                ));
            }
            Err(error) => return Err(error),
        }
    }

    if updates_by_workbook.is_empty() {
        return Err(MappedProcessorError::Config(format!(
            "task '{}' produced no workbook updates",
            task.id
        )));
    }

    let mut paths_by_workbook = HashMap::<String, PathBuf>::new();

    let mut workbook_patches = Vec::new();

    for (workbook, updates) in updates_by_workbook {
        let workbook_path = resolve_workbook_path(profile, unit_folder, &workbook)?;
        log.push(format!(
            "[mapped] Workbook {}: {}",
            workbook,
            workbook_path.display()
        ));
        workbook_patches.push(WorkbookPatch::new(workbook_path.clone(), updates));
        paths_by_workbook.insert(workbook, workbook_path);
    }

    Ok(ProcessorTaskOutput::new(
        success(
            format!("{} processed successfully", task.label),
            log,
            paths_by_workbook,
            Some(csv_source),
        ),
        workbook_patches,
    ))
}

fn cell_update_from_mapping(
    mapping: &MappingDefinition,
    table: &CsvTable,
) -> MappedAttempt<Option<(String, CellUpdate, String)>> {
    let source = mapping.source.as_ref().ok_or_else(|| {
        MappedProcessorError::Config(format!("mapping '{}' has no source rule", mapping.label))
    })?;
    let row = match select_row(table, &source.row, &source.column) {
        Ok(row) => row,
        Err(error) if !source.required && is_optional_source_error(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let raw = match required_number(row, &source.column, table.path(), &mapping.label) {
        Ok(value) => value,
        Err(error) if !source.required && is_optional_source_error(&error) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let value = apply_transform(raw, mapping.transform.as_ref())?;
    let update = CellUpdate::number(
        mapping.target.sheet.clone(),
        mapping.target.cell.clone(),
        value,
    );

    Ok(Some((
        mapping.target.workbook.clone(),
        update,
        format_number_for_log(value, mapping.transform.as_ref()),
    )))
}

fn select_row<'a>(
    table: &'a CsvTable,
    row: &MappingRow,
    source_column: &str,
) -> Result<&'a [String], CsvDataError> {
    match row {
        MappingRow::Index(index) => table.row_at(*index),
        MappingRow::Selector(selector) if selector == "first_data_after_header" => {
            table.first_data_row_after_header()
        }
        MappingRow::Selector(selector) if selector == "last_data_after_header" => {
            table.last_data_row_after_header()
        }
        MappingRow::Selector(selector) if selector == "last_data" => table.last_data_row(),
        MappingRow::Selector(selector) if selector == "last_numeric" => {
            table.last_numeric_row_after_header(source_column)
        }
        MappingRow::Selector(selector) => Err(CsvDataError::Empty(format!(
            "{} uses unsupported row selector '{}'",
            table.path().display(),
            selector
        ))),
    }
}

fn apply_transform(mut value: f64, transform: Option<&TransformDefinition>) -> MappedAttempt<f64> {
    let Some(transform) = transform else {
        return Ok(value);
    };

    if let Some(scale_by) = transform.scale_by {
        if scale_by == 0.0 {
            return Err(MappedProcessorError::Config(
                "mapping transform.scale_by must not be zero".to_string(),
            ));
        }

        value /= scale_by;
    }

    if let Some(digits) = transform.round {
        value = round_to(value, digits);
    }

    Ok(value)
}

fn is_optional_source_error(error: &CsvDataError) -> bool {
    matches!(
        error,
        CsvDataError::BlankValue { .. }
            | CsvDataError::MissingColumn { .. }
            | CsvDataError::NonnumericValue { .. }
            | CsvDataError::Empty(_)
    )
}

fn resolve_workbook_path(
    profile: &ReportLayoutProfile,
    unit_folder: &Path,
    workbook: &str,
) -> MappedAttempt<PathBuf> {
    let definition = profile
        .workbooks
        .get(workbook)
        .ok_or_else(|| MappedProcessorError::Config(format!("unknown workbook '{workbook}'")))?;

    find_latest_file_by_pattern(unit_folder, &definition.file_pattern).ok_or_else(|| {
        MappedProcessorError::Config(format!(
            "workbook '{}' matching '{}' was not found under {}",
            workbook,
            definition.file_pattern,
            unit_folder.display()
        ))
    })
}

fn require_csv_by_pattern(
    unit_folder: &Path,
    csv_pattern: &str,
    task_label: &str,
) -> MappedAttempt<CsvSource> {
    let path = find_latest_file_by_pattern(unit_folder, csv_pattern).ok_or_else(|| {
        MappedProcessorError::MissingCsv(format!(
            "{task_label}: no matching CSV '{}' found under {}",
            csv_pattern,
            unit_folder.display()
        ))
    })?;

    wait_for_stable_csv(&path, CSV_STABLE_FOR, CSV_MAX_WAIT)?;
    let fingerprint = csv_fingerprint(&path)?;

    Ok(CsvSource { path, fingerprint })
}

pub(crate) fn find_latest_file_by_pattern(root: &Path, pattern: &str) -> Option<PathBuf> {
    let pattern = wildcard_pattern_to_regex(pattern)?;

    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| pattern.is_match(name))
        })
        .filter_map(|path| {
            let modified = path
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);

            Some((path, modified))
        })
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

fn wildcard_pattern_to_regex(pattern: &str) -> Option<Regex> {
    if pattern.trim().is_empty() {
        return None;
    }

    let mut regex = String::from("(?i)^");

    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            _ => regex.push_str(&regex::escape(&ch.to_string())),
        }
    }

    regex.push('$');
    Regex::new(&regex).ok()
}

fn success(
    message: String,
    log: Vec<String>,
    paths_by_workbook: HashMap<String, PathBuf>,
    csv_source: Option<CsvSource>,
) -> ProcessorResult {
    ProcessorResult {
        state: "pass".to_string(),
        code: 0,
        message,
        log,
        report_path: paths_by_workbook
            .get("main")
            .map(|path| path.display().to_string()),
        print_report_path: paths_by_workbook
            .get("print")
            .map(|path| path.display().to_string()),
        failure: None,
        source_csv_path: csv_source
            .as_ref()
            .map(|source| source.path.display().to_string()),
        csv_fingerprint: csv_source.map(|source| source.fingerprint),
    }
}

fn idempotent_success(
    task: &TaskDefinition,
    csv_source: &CsvSource,
    already_processed_fingerprint: Option<&str>,
) -> Option<ProcessorResult> {
    if already_processed_fingerprint != Some(csv_source.fingerprint.as_str()) {
        return None;
    }

    Some(success(
        format!(
            "{} already processed from the same CSV; no workbook changes were applied",
            task.label
        ),
        vec![format!(
            "[idempotent] {} already processed from {} ({})",
            task.label,
            csv_source.path.display(),
            csv_source.fingerprint
        )],
        HashMap::new(),
        Some(csv_source.clone()),
    ))
}

fn format_number_for_log(value: f64, transform: Option<&TransformDefinition>) -> String {
    match transform.and_then(|transform| transform.round) {
        Some(digits) => format!("{value:.digits$}", digits = digits as usize),
        None => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MappingSource, MappingTarget};

    #[test]
    fn wildcard_patterns_match_case_insensitively() {
        let pattern = wildcard_pattern_to_regex("*STEP14*TRANSFORMER_TEST_DATA_AVG*.csv")
            .expect("pattern should compile");

        assert!(pattern.is_match("unit_step14_transformer_test_data_avg.csv"));
        assert!(!pattern.is_match("unit_STEP15_TRANSFORMER_TEST_DATA_AVG.csv"));
    }

    #[test]
    fn mapping_builds_number_update_from_first_data_row() {
        let temp = tempfile::tempdir().expect("temp dir");
        let csv_path = temp.path().join("STEP14_TRANSFORMER_TEST_DATA_AVG.csv");
        let mut row = vec![""; 32];
        row[25] = "120.456";
        std::fs::write(&csv_path, format!("header\n{}\n", row.join(","))).expect("write csv");
        let table = CsvTable::read(&csv_path).expect("csv should parse");
        let mapping = MappingDefinition {
            label: "Usum".to_string(),
            source: Some(MappingSource {
                column: "Z".to_string(),
                row: MappingRow::Selector("first_data_after_header".to_string()),
                required: true,
            }),
            transform: Some(TransformDefinition {
                scale_by: None,
                round: Some(2),
            }),
            target: MappingTarget {
                workbook: "main".to_string(),
                sheet: "XFMR Check_208VAC".to_string(),
                cell: "B9".to_string(),
                number_format: None,
            },
        };

        let (_, update, value) = cell_update_from_mapping(&mapping, &table)
            .expect("mapping should process")
            .expect("mapping should produce update");

        assert_eq!(update.sheet, "XFMR Check_208VAC");
        assert_eq!(update.cell, "B9");
        assert_eq!(value, "120.46");
    }
}
