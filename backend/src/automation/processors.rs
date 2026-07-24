use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Local;
use serde::Serialize;

use crate::config::{
    load_accuracy_thresholds, AccuracyThresholdConfig, AccuracyThresholdError,
    BreakerMetricThresholds, SystemMetricThresholds,
};

use super::csv_data::{
    csv_fingerprint, find_latest_csv, required_number, round_to, wait_for_stable_csv, CsvDataError,
    CsvTable,
};
use super::reports::{
    extract_serial_number, patch_workbooks_transactional, require_main_report_with_config,
    require_print_report_with_config, CellUpdate, ReportError, ReportFileConfig, WorkbookPatch,
};
use super::tasks::{
    step_for_breaker, step_for_system, AutomationTask, LoadLevel, TaskKind, VoltageSet,
};

#[derive(Debug, Clone, Serialize)]
pub struct ProcessorResult {
    pub state: String,
    pub code: u8,
    pub message: String,
    pub log: Vec<String>,
    pub report_path: Option<String>,
    pub print_report_path: Option<String>,
    pub failure: Option<FailureDetail>,
    pub source_csv_path: Option<String>,
    pub csv_fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProcessorTaskOutput {
    pub result: ProcessorResult,
    pub patches: Vec<WorkbookPatch>,
}

impl ProcessorTaskOutput {
    pub fn new(result: ProcessorResult, patches: Vec<WorkbookPatch>) -> Self {
        Self { result, patches }
    }

    pub fn result_only(result: ProcessorResult) -> Self {
        Self {
            result,
            patches: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureDetail {
    pub title: String,
    pub message: String,
    pub location: Option<FailureLocation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureLocation {
    pub workbook_path: String,
    pub sheet: String,
    pub cell: String,
}

#[derive(Debug, thiserror::Error)]
enum ProcessorError {
    #[error("{0}")]
    Csv(CsvDataError),
    #[error("{0}")]
    CsvNotReady(String),
    #[error("{0}")]
    Config(#[from] AccuracyThresholdError),
    #[error("{0}")]
    Report(#[from] ReportError),
    #[error("{0}")]
    MissingCsv(String),
}

impl From<CsvDataError> for ProcessorError {
    fn from(error: CsvDataError) -> Self {
        if error.is_transient_file_access() {
            ProcessorError::CsvNotReady(format!(
                "CSV is still being written by ATS; waiting for it to unlock. {error}"
            ))
        } else {
            ProcessorError::Csv(error)
        }
    }
}

type ProcessorAttempt<T> = Result<T, ProcessorError>;

const CSV_STABLE_FOR: Duration = Duration::from_millis(400);
const CSV_MAX_WAIT: Duration = Duration::from_millis(1_500);
pub(crate) const ACCURACY_CHECK_FAILURE_TITLE: &str = "Accuracy Check Failed";

#[derive(Debug, Clone)]
struct CsvSource {
    path: PathBuf,
    fingerprint: String,
}

pub fn process_task(
    task: &AutomationTask,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
    report_config: &ReportFileConfig,
) -> Result<ProcessorResult, ReportError> {
    let output = compute_task(
        task,
        unit_folder,
        already_processed_fingerprint,
        report_config,
    )?;

    if !output.patches.is_empty() {
        patch_workbooks_transactional(&output.patches)?;
    }

    Ok(output.result)
}

pub fn compute_task(
    task: &AutomationTask,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
    report_config: &ReportFileConfig,
) -> Result<ProcessorTaskOutput, ReportError> {
    match process_task_inner(
        task,
        unit_folder,
        already_processed_fingerprint,
        report_config,
    ) {
        Ok(output) => Ok(output),
        Err(ProcessorError::Report(error)) => Err(error),
        Err(error) => Ok(ProcessorTaskOutput::result_only(processor_error_result(
            error,
        ))),
    }
}

fn processor_error_result(error: ProcessorError) -> ProcessorResult {
    match error {
        ProcessorError::CsvNotReady(message) => ProcessorResult {
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
        ProcessorError::MissingCsv(message) => ProcessorResult {
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
                    title: "Processing Error".to_string(),
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
    task: &AutomationTask,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
    report_config: &ReportFileConfig,
) -> ProcessorAttempt<ProcessorTaskOutput> {
    match task.kind {
        TaskKind::Transformer { voltage } => process_transformer(
            task,
            unit_folder,
            voltage,
            already_processed_fingerprint,
            report_config,
        ),
        TaskKind::System { voltage, load } => process_system(
            task,
            unit_folder,
            voltage,
            load,
            already_processed_fingerprint,
            report_config,
        ),
        TaskKind::Breaker {
            voltage,
            breaker,
            load,
        } => process_breaker(
            task,
            unit_folder,
            voltage,
            breaker,
            load,
            already_processed_fingerprint,
            report_config,
        ),
        TaskKind::SystemBurnIn => process_system_burn_in(
            task,
            unit_folder,
            already_processed_fingerprint,
            report_config,
        ),
        TaskKind::BreakerBurnIn { breaker } => process_breaker_burn_in(
            task,
            unit_folder,
            breaker,
            already_processed_fingerprint,
            report_config,
        ),
    }
}

fn process_transformer(
    task: &AutomationTask,
    unit_folder: &Path,
    voltage: VoltageSet,
    already_processed_fingerprint: Option<&str>,
    report_config: &ReportFileConfig,
) -> ProcessorAttempt<ProcessorTaskOutput> {
    let step = match voltage {
        VoltageSet::V208 => 14,
        VoltageSet::V415 => 43,
    };
    let sheet = match voltage {
        VoltageSet::V208 => "XFMR Check_208VAC",
        VoltageSet::V415 => "XFMR Check_415VAC",
    };
    let csv_source = require_csv(
        unit_folder,
        step,
        &["TRANSFORMER_TEST_DATA_AVG"],
        &task.label,
    )?;
    if let Some(result) = idempotent_success(task, &csv_source, already_processed_fingerprint) {
        return Ok(ProcessorTaskOutput::result_only(result));
    }

    let table = CsvTable::read(&csv_source.path)?;
    let row = table.first_data_row_after_header()?;
    let report_path = require_main_report_with_config(unit_folder, report_config)?;
    let mut updates = Vec::new();
    let mut log = vec![
        format!("[xfmr] Step {step}: {}", csv_source.path.display()),
        format!("[xfmr] Report: {}", report_path.display()),
    ];

    for (label, column, cell) in [
        ("Usum B9", "Z", "B9"),
        ("fU1 B10", "AE", "B10"),
        ("Usum B11", "BG", "B11"),
        ("fU1 B12", "BL", "B12"),
    ] {
        let value = round_to(required_number(row, column, table.path(), label)?, 2);
        updates.push(CellUpdate::number(sheet, cell, value));
        log.push(format!("[xfmr]   {sheet}!{cell} <- {column} = {value:.2}"));
    }

    Ok(ProcessorTaskOutput::new(
        success(
            format!("{} processed successfully", task.label),
            log,
            Some(report_path.clone()),
            None,
            Some(csv_source),
        ),
        vec![WorkbookPatch::new(report_path, updates)],
    ))
}

fn process_system(
    task: &AutomationTask,
    unit_folder: &Path,
    voltage: VoltageSet,
    load: LoadLevel,
    already_processed_fingerprint: Option<&str>,
    report_config: &ReportFileConfig,
) -> ProcessorAttempt<ProcessorTaskOutput> {
    let step = step_for_system(voltage, load);
    let sheet = match voltage {
        VoltageSet::V208 => "System Test - 480_208",
        VoltageSet::V415 => "System Test - 480_415",
    };
    let csv_source = require_csv(
        unit_folder,
        step,
        &["SYSTEM_ACCURACY_TEST_DATA_AVG"],
        &task.label,
    )?;
    if let Some(result) = idempotent_success(task, &csv_source, already_processed_fingerprint) {
        return Ok(ProcessorTaskOutput::result_only(result));
    }

    let table = CsvTable::read(&csv_source.path)?;
    let row = table.last_data_row_after_header()?;
    let report_path = require_main_report_with_config(unit_folder, report_config)?;
    let mut updates = Vec::new();
    let mut values = HashMap::<String, f64>::new();
    let mut log = vec![
        format!("[system] {} {}", voltage.display(), load.system_test_name()),
        format!("[system] CSV: {}", csv_source.path.display()),
    ];

    add_system_group(
        &mut updates,
        &mut values,
        sheet,
        row,
        table.path(),
        load,
        SystemGroup::InputMeter,
    )?;
    add_system_group(
        &mut updates,
        &mut values,
        sheet,
        row,
        table.path(),
        load,
        SystemGroup::OutputMeter,
    )?;
    add_system_group(
        &mut updates,
        &mut values,
        sheet,
        row,
        table.path(),
        load,
        SystemGroup::InputDetect,
    )?;
    add_system_group(
        &mut updates,
        &mut values,
        sheet,
        row,
        table.path(),
        load,
        SystemGroup::OutputDetect,
    )?;

    if load == LoadLevel::Full {
        add_system_cf_updates(&mut updates, row, table.path(), voltage)?;
    }

    let thresholds = load_accuracy_thresholds()?;
    let failures = add_system_accuracy_updates(&mut updates, &values, sheet, load, &thresholds);

    if failures.is_empty() {
        log.push("[system] Verification Result: PASS".to_string());
        Ok(ProcessorTaskOutput::new(
            success(
                format!(
                    "{} {} processed successfully",
                    voltage.display(),
                    load.display()
                ),
                log,
                Some(report_path.clone()),
                None,
                Some(csv_source),
            ),
            vec![WorkbookPatch::new(report_path, updates)],
        ))
    } else {
        log.extend(failures.iter().map(|failure| format!("[system] {failure}")));
        let failure_text = verification_messages(&failures);
        let message = format!(
            "{} {} verification failed: {}",
            voltage.display(),
            load.display(),
            failure_text
        );
        let failure = failure_detail(
            ACCURACY_CHECK_FAILURE_TITLE,
            &message,
            report_path.as_path(),
            failures.first(),
        );

        Ok(ProcessorTaskOutput::new(
            failed(
                message,
                log,
                Some(report_path.clone()),
                None,
                failure,
                Some(csv_source),
            ),
            vec![WorkbookPatch::new(report_path, updates)],
        ))
    }
}

fn process_breaker(
    task: &AutomationTask,
    unit_folder: &Path,
    voltage: VoltageSet,
    breaker: u8,
    load: LoadLevel,
    already_processed_fingerprint: Option<&str>,
    report_config: &ReportFileConfig,
) -> ProcessorAttempt<ProcessorTaskOutput> {
    let step = step_for_breaker(voltage, breaker, load);
    let csv_fragment = format!("SUB_FEED_{breaker:02}_ACCURACY_TEST_DATA_AVG");
    let csv_source = require_csv(unit_folder, step, &[csv_fragment.as_str()], &task.label)?;
    if let Some(result) = idempotent_success(task, &csv_source, already_processed_fingerprint) {
        return Ok(ProcessorTaskOutput::result_only(result));
    }

    let table = CsvTable::read(&csv_source.path)?;
    let row = table.last_data_row_after_header()?;
    let report_path = require_main_report_with_config(unit_folder, report_config)?;
    let sheet = match voltage {
        VoltageSet::V208 => "Subfeed - 480_208",
        VoltageSet::V415 => "Subfeed - 480_415",
    };
    let mut updates = Vec::new();
    let mut log = vec![
        format!(
            "[breaker] {} Breaker {breaker} {}",
            voltage.display(),
            load.display()
        ),
        format!("[breaker] CSV: {}", csv_source.path.display()),
    ];

    add_breaker_cf_updates(&mut updates, unit_folder, voltage, breaker);
    let thresholds = load_accuracy_thresholds()?;
    let failures = add_breaker_load_updates(
        &mut updates,
        row,
        table.path(),
        sheet,
        voltage,
        breaker,
        load,
        &thresholds,
    )?;

    if failures.is_empty() {
        log.push("[breaker] Verification Result: PASS".to_string());
        Ok(ProcessorTaskOutput::new(
            success(
                format!(
                    "{} Breaker {breaker} {} processed successfully",
                    voltage.display(),
                    load.display()
                ),
                log,
                Some(report_path.clone()),
                None,
                Some(csv_source),
            ),
            vec![WorkbookPatch::new(report_path, updates)],
        ))
    } else {
        log.extend(
            failures
                .iter()
                .map(|failure| format!("[breaker] {failure}")),
        );
        let failure_text = verification_messages(&failures);
        let message = format!(
            "{} Breaker {breaker} {} verification failed: {}",
            voltage.display(),
            load.display(),
            failure_text
        );
        let failure = failure_detail(
            ACCURACY_CHECK_FAILURE_TITLE,
            &message,
            report_path.as_path(),
            failures.first(),
        );

        Ok(ProcessorTaskOutput::new(
            failed(
                message,
                log,
                Some(report_path.clone()),
                None,
                failure,
                Some(csv_source),
            ),
            vec![WorkbookPatch::new(report_path, updates)],
        ))
    }
}

fn process_system_burn_in(
    task: &AutomationTask,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
    report_config: &ReportFileConfig,
) -> ProcessorAttempt<ProcessorTaskOutput> {
    let csv_source = require_csv(
        unit_folder,
        72,
        &["SYSTEM_ACCURACY_TEST_DATA_AVG"],
        &task.label,
    )?;
    if let Some(result) = idempotent_success(task, &csv_source, already_processed_fingerprint) {
        return Ok(ProcessorTaskOutput::result_only(result));
    }

    let table = CsvTable::read(&csv_source.path)?;
    let row = table.last_data_row()?;
    let report_path = require_main_report_with_config(unit_folder, report_config)?;
    let print_path = require_print_report_with_config(unit_folder, report_config)?;
    let serial = extract_serial_number(unit_folder).unwrap_or_default();
    let today = Local::now().format("%m/%d/%Y").to_string();
    let mut main_updates = Vec::new();
    let mut print_updates = Vec::new();
    let mut log = vec![
        format!("[burn-in system] CSV: {}", csv_source.path.display()),
        format!("[burn-in system] Main: {}", report_path.display()),
        format!("[burn-in system] Print: {}", print_path.display()),
    ];

    for point in system_burn_in_points() {
        let raw = required_number(row, point.column, table.path(), point.description)?;
        let value = round_to(raw / point.scale_by, 2);
        main_updates.push(CellUpdate::number(
            "Burn-in System - 415",
            point.main_cell,
            value,
        ));
        print_updates.push(CellUpdate::number("Test Report", point.print_cell, value));
    }

    if !serial.is_empty() {
        main_updates.push(CellUpdate::text("Test Summary", "B2", serial.clone()));
        print_updates.push(CellUpdate::text(
            "Test Report",
            "F3",
            format!("SN:{serial}"),
        ));
    }

    print_updates.push(CellUpdate::text("Test Report", "K3", today));

    log.push("[burn-in system] Reports updated".to_string());

    Ok(ProcessorTaskOutput::new(
        success(
            "System burn-in processed successfully".to_string(),
            log,
            Some(report_path.clone()),
            Some(print_path.clone()),
            Some(csv_source),
        ),
        vec![
            WorkbookPatch::new(report_path, main_updates),
            WorkbookPatch::new(print_path, print_updates),
        ],
    ))
}

fn process_breaker_burn_in(
    task: &AutomationTask,
    unit_folder: &Path,
    breaker: u8,
    already_processed_fingerprint: Option<&str>,
    report_config: &ReportFileConfig,
) -> ProcessorAttempt<ProcessorTaskOutput> {
    let step = 72 + u16::from(breaker);
    let csv_fragment = format!("SUB_FEED_{breaker:02}_ACCURACY_TEST_DATA_AVG");
    let csv_source = require_csv(unit_folder, step, &[csv_fragment.as_str()], &task.label)?;
    if let Some(result) = idempotent_success(task, &csv_source, already_processed_fingerprint) {
        return Ok(ProcessorTaskOutput::result_only(result));
    }

    let table = CsvTable::read(&csv_source.path)?;
    let row = table.last_data_row()?;
    let report_path = require_main_report_with_config(unit_folder, report_config)?;
    let print_path = require_print_report_with_config(unit_folder, report_config)?;
    let main_row = 11 + (u32::from(breaker) - 1) * 4;
    let print_row = 6 + (u32::from(breaker) - 1) * 4;
    let mut main_updates = Vec::new();
    let mut print_updates = Vec::new();
    let mut log = vec![format!(
        "[burn-in breaker] Breaker {breaker} CSV: {}",
        csv_source.path.display()
    )];

    for (metric_offset, metric) in ["V", "I", "P", "PF"].iter().enumerate() {
        let row_offset = metric_offset as u32;
        for (phase_index, phase) in ["A", "B", "C"].iter().enumerate() {
            let meter_col = burn_in_meter_columns(metric)[phase_index];
            let detect_col = burn_in_detect_columns(metric)[phase_index];
            let scale = if *metric == "P" { 1000.0 } else { 1.0 };
            let meter = round_to(
                required_number(
                    row,
                    meter_col,
                    table.path(),
                    &format!("Breaker {breaker} burn-in {metric} meter {phase}"),
                )? / scale,
                2,
            );
            let detect = round_to(
                required_number(
                    row,
                    detect_col,
                    table.path(),
                    &format!("Breaker {breaker} burn-in {metric} detect {phase}"),
                )? / scale,
                2,
            );
            let main_cols = burn_in_main_cols(phase);
            let print_cols = burn_in_print_cols(phase);
            let main_target_row = main_row + row_offset;
            let print_target_row = print_row + row_offset;

            main_updates.push(CellUpdate::number(
                "Burn-in Subfeed - 415",
                format!("{}{}", main_cols.0, main_target_row),
                meter,
            ));
            main_updates.push(CellUpdate::number(
                "Burn-in Subfeed - 415",
                format!("{}{}", main_cols.1, main_target_row),
                detect,
            ));
            print_updates.push(CellUpdate::number(
                "Test Report #2",
                format!("{}{}", print_cols.0, print_target_row),
                meter,
            ));
            print_updates.push(CellUpdate::number(
                "Test Report #2",
                format!("{}{}", print_cols.1, print_target_row),
                detect,
            ));
        }
    }

    log.push("[burn-in breaker] Reports updated".to_string());

    Ok(ProcessorTaskOutput::new(
        success(
            format!("Breaker {breaker} burn-in processed successfully"),
            log,
            Some(report_path.clone()),
            Some(print_path.clone()),
            Some(csv_source),
        ),
        vec![
            WorkbookPatch::new(report_path, main_updates),
            WorkbookPatch::new(print_path, print_updates),
        ],
    ))
}

fn success(
    message: String,
    log: Vec<String>,
    report_path: Option<PathBuf>,
    print_report_path: Option<PathBuf>,
    csv_source: Option<CsvSource>,
) -> ProcessorResult {
    ProcessorResult {
        state: "pass".to_string(),
        code: 0,
        message,
        log,
        report_path: report_path.map(|path| path.display().to_string()),
        print_report_path: print_report_path.map(|path| path.display().to_string()),
        failure: None,
        source_csv_path: csv_source
            .as_ref()
            .map(|source| source.path.display().to_string()),
        csv_fingerprint: csv_source.map(|source| source.fingerprint),
    }
}

fn failed(
    message: String,
    log: Vec<String>,
    report_path: Option<PathBuf>,
    print_report_path: Option<PathBuf>,
    failure: FailureDetail,
    csv_source: Option<CsvSource>,
) -> ProcessorResult {
    ProcessorResult {
        state: "fail".to_string(),
        code: 1,
        message,
        log,
        report_path: report_path.map(|path| path.display().to_string()),
        print_report_path: print_report_path.map(|path| path.display().to_string()),
        failure: Some(failure),
        source_csv_path: csv_source
            .as_ref()
            .map(|source| source.path.display().to_string()),
        csv_fingerprint: csv_source.map(|source| source.fingerprint),
    }
}

fn idempotent_success(
    task: &AutomationTask,
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
        None,
        None,
        Some(csv_source.clone()),
    ))
}

fn require_csv(
    unit_folder: &Path,
    step: u16,
    required_fragments: &[&str],
    task_label: &str,
) -> ProcessorAttempt<CsvSource> {
    let fragments = required_fragments
        .iter()
        .map(|fragment| (*fragment).to_string())
        .collect::<Vec<_>>();

    let path = find_latest_csv(unit_folder, step, &fragments).ok_or_else(|| {
        ProcessorError::MissingCsv(format!(
            "{task_label}: no matching STEP{step} CSV found under {}",
            unit_folder.display()
        ))
    })?;

    wait_for_stable_csv(&path, CSV_STABLE_FOR, CSV_MAX_WAIT)?;
    let fingerprint = csv_fingerprint(&path)?;

    Ok(CsvSource { path, fingerprint })
}

#[derive(Debug, Clone, Copy)]
enum SystemGroup {
    InputMeter,
    OutputMeter,
    InputDetect,
    OutputDetect,
}

fn add_system_group(
    updates: &mut Vec<CellUpdate>,
    values: &mut HashMap<String, f64>,
    sheet: &str,
    row: &[String],
    csv_path: &Path,
    load: LoadLevel,
    group: SystemGroup,
) -> ProcessorAttempt<()> {
    let row_shift = match load {
        LoadLevel::Full => 0,
        LoadLevel::Half => 20,
        LoadLevel::Low => 40,
    };
    let (summary_col, summary_base, phase_cols, phase_base, specs) = match group {
        SystemGroup::InputMeter => (
            "E",
            11,
            ["E", "H", "K"],
            17,
            [
                ("Frequency", "AF", 1.0),
                ("Active Power", "AC", 1000.0),
                ("Apparent Power", "AD", 1000.0),
                ("Power Factor", "AE", 1.0),
                ("Voltage A", "AG", 1.0),
                ("Current A", "G", 1.0),
                ("Voltage B", "AH", 1.0),
                ("Current B", "N", 1.0),
                ("Voltage C", "AI", 1.0),
                ("Current C", "U", 1.0),
            ],
        ),
        SystemGroup::OutputMeter => (
            "E",
            20,
            ["E", "H", "K"],
            26,
            [
                ("Frequency", "BT", 1.0),
                ("Active Power", "BQ", 1000.0),
                ("Apparent Power", "BR", 1000.0),
                ("Power Factor", "BS", 1.0),
                ("Voltage A", "AT", 1.0),
                ("Current A", "AU", 1.0),
                ("Voltage B", "BA", 1.0),
                ("Current B", "BB", 1.0),
                ("Voltage C", "BH", 1.0),
                ("Current C", "BI", 1.0),
            ],
        ),
        SystemGroup::InputDetect => (
            "F",
            11,
            ["F", "I", "L"],
            17,
            [
                ("Frequency", "DH", 1.0),
                ("Active Power", "DE", 1000.0),
                ("Apparent Power", "DF", 1000.0),
                ("Power Factor", "DG", 1.0),
                ("Voltage A", "CH", 1.0),
                ("Current A", "CI", 1.0),
                ("Voltage B", "CO", 1.0),
                ("Current B", "CP", 1.0),
                ("Voltage C", "CV", 1.0),
                ("Current C", "CW", 1.0),
            ],
        ),
        SystemGroup::OutputDetect => (
            "F",
            20,
            ["F", "I", "L"],
            26,
            [
                ("Frequency", "EO", 1.0),
                ("Active Power", "EL", 1000.0),
                ("Apparent Power", "EM", 1000.0),
                ("Power Factor", "EN", 1.0),
                ("Voltage A", "DO", 1.0),
                ("Current A", "DP", 1.0),
                ("Voltage B", "DV", 1.0),
                ("Current B", "DW", 1.0),
                ("Voltage C", "EC", 1.0),
                ("Current C", "ED", 1.0),
            ],
        ),
    };

    for (index, (label, column, scale)) in specs.iter().enumerate() {
        let target = match index {
            0..=3 => format!("{summary_col}{}", summary_base + row_shift + index as u32),
            4 => format!("{}{}", phase_cols[0], phase_base + row_shift),
            5 => format!("{}{}", phase_cols[0], phase_base + row_shift + 1),
            6 => format!("{}{}", phase_cols[1], phase_base + row_shift),
            7 => format!("{}{}", phase_cols[1], phase_base + row_shift + 1),
            8 => format!("{}{}", phase_cols[2], phase_base + row_shift),
            9 => format!("{}{}", phase_cols[2], phase_base + row_shift + 1),
            _ => unreachable!(),
        };
        let value = round_to(
            required_number(row, column, csv_path, &format!("{label} {target}"))? / scale,
            2,
        );

        values.insert(target.clone(), value);
        updates.push(CellUpdate::number(sheet, target, value));
    }

    Ok(())
}

fn add_system_cf_updates(
    updates: &mut Vec<CellUpdate>,
    row: &[String],
    csv_path: &Path,
    voltage: VoltageSet,
) -> ProcessorAttempt<()> {
    let targets = match voltage {
        VoltageSet::V208 => [
            ("AK", "B15"),
            ("AL", "C15"),
            ("AM", "D15"),
            ("BY", "B16"),
            ("BZ", "C16"),
            ("CA", "D16"),
        ],
        VoltageSet::V415 => [
            ("AK", "H15"),
            ("AL", "I15"),
            ("AM", "J15"),
            ("BY", "H16"),
            ("BZ", "I16"),
            ("CA", "J16"),
        ],
    };

    for (column, cell) in targets {
        let value = required_number(row, column, csv_path, &format!("CF {cell}"))?;
        updates.push(CellUpdate::text("CF", cell, format!("{value:.3}")));
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct VerificationIssue {
    message: String,
    sheet: String,
    cell: String,
}

impl VerificationIssue {
    fn new(message: impl Into<String>, sheet: impl Into<String>, cell: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sheet: sheet.into(),
            cell: cell.into(),
        }
    }
}

impl std::fmt::Display for VerificationIssue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

fn failure_detail(
    title: &str,
    message: &str,
    workbook_path: &Path,
    issue: Option<&VerificationIssue>,
) -> FailureDetail {
    FailureDetail {
        title: title.to_string(),
        message: message.to_string(),
        location: issue.map(|issue| FailureLocation {
            workbook_path: workbook_path.display().to_string(),
            sheet: issue.sheet.clone(),
            cell: issue.cell.clone(),
        }),
    }
}

fn verification_messages(issues: &[VerificationIssue]) -> String {
    issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

fn system_thresholds_for_load(
    load: LoadLevel,
    thresholds: &AccuracyThresholdConfig,
) -> &SystemMetricThresholds {
    match load {
        LoadLevel::Full => &thresholds.system.full_load,
        LoadLevel::Half => &thresholds.system.half_load,
        LoadLevel::Low => &thresholds.system.low_load,
    }
}

fn breaker_thresholds_for_load(
    load: LoadLevel,
    thresholds: &AccuracyThresholdConfig,
) -> &BreakerMetricThresholds {
    match load {
        LoadLevel::Full => &thresholds.breaker.full_load,
        LoadLevel::Half => &thresholds.breaker.half_load,
        LoadLevel::Low => &thresholds.breaker.low_load,
    }
}

fn add_system_accuracy_updates(
    updates: &mut Vec<CellUpdate>,
    values: &HashMap<String, f64>,
    sheet: &str,
    load: LoadLevel,
    thresholds: &AccuracyThresholdConfig,
) -> Vec<VerificationIssue> {
    let mut failures = Vec::new();

    for entry in system_accuracy_entries(load, thresholds) {
        let Some(detect) = values.get(&entry.detect_cell) else {
            failures.push(VerificationIssue::new(
                format!("{} missing detect cell {}", entry.label, entry.detect_cell),
                sheet,
                entry.accuracy_cell.clone(),
            ));
            continue;
        };
        let Some(meter) = values.get(&entry.meter_cell) else {
            failures.push(VerificationIssue::new(
                format!("{} missing meter cell {}", entry.label, entry.meter_cell),
                sheet,
                entry.accuracy_cell.clone(),
            ));
            continue;
        };

        let accuracy = if entry.is_power_factor {
            (detect - meter) * 100.0
        } else if *meter == 0.0 {
            failures.push(VerificationIssue::new(
                format!("{} meter cell {} is zero", entry.label, entry.meter_cell),
                sheet,
                entry.meter_cell.clone(),
            ));
            continue;
        } else {
            (detect - meter) / meter * 100.0
        };
        let accuracy = round_to(accuracy, 4);
        updates.push(CellUpdate::number(
            sheet,
            entry.accuracy_cell.clone(),
            accuracy,
        ));

        if accuracy.abs() > entry.threshold {
            failures.push(VerificationIssue::new(
                format!(
                    "{} @ {} = {accuracy:.4}% exceeds +/-{}%",
                    entry.label, entry.accuracy_cell, entry.threshold
                ),
                sheet,
                entry.accuracy_cell.clone(),
            ));
        }
    }

    failures
}

#[derive(Debug, Clone)]
struct SystemAccuracyEntry {
    label: String,
    detect_cell: String,
    meter_cell: String,
    accuracy_cell: String,
    threshold: f64,
    is_power_factor: bool,
}

fn system_accuracy_entries(
    load: LoadLevel,
    thresholds: &AccuracyThresholdConfig,
) -> Vec<SystemAccuracyEntry> {
    let row_shift = match load {
        LoadLevel::Full => 0,
        LoadLevel::Half => 20,
        LoadLevel::Low => 40,
    };
    let load_thresholds = system_thresholds_for_load(load, thresholds);
    let mut entries = Vec::new();

    for (label, detect_col, meter_col, accuracy_col, base_row, threshold, is_pf) in [
        (
            "Active Power in",
            "E",
            "F",
            "G",
            12,
            load_thresholds.active_power,
            false,
        ),
        (
            "Apparent Power in",
            "E",
            "F",
            "G",
            13,
            load_thresholds.apparent_power,
            false,
        ),
        (
            "Power Factor in",
            "E",
            "F",
            "G",
            14,
            load_thresholds.power_factor,
            true,
        ),
        (
            "Active Power out",
            "E",
            "F",
            "G",
            21,
            load_thresholds.active_power,
            false,
        ),
        (
            "Apparent Power out",
            "E",
            "F",
            "G",
            22,
            load_thresholds.apparent_power,
            false,
        ),
        (
            "Power Factor out",
            "E",
            "F",
            "G",
            23,
            load_thresholds.power_factor,
            true,
        ),
    ] {
        entries.push(SystemAccuracyEntry {
            label: label.to_string(),
            detect_cell: format!("{detect_col}{}", base_row + row_shift),
            meter_cell: format!("{meter_col}{}", base_row + row_shift),
            accuracy_cell: format!("{accuracy_col}{}", base_row + row_shift),
            threshold,
            is_power_factor: is_pf,
        });
    }

    for (label, row, threshold) in [
        ("Voltage in", 17, load_thresholds.voltage),
        ("Current in", 18, load_thresholds.current),
        ("Voltage out", 26, load_thresholds.voltage),
        ("Current out", 27, load_thresholds.current),
    ] {
        for (phase, detect_col, meter_col, accuracy_col) in [
            ("A", "E", "F", "G"),
            ("B", "H", "I", "J"),
            ("C", "K", "L", "M"),
        ] {
            entries.push(SystemAccuracyEntry {
                label: format!("{label} {phase}"),
                detect_cell: format!("{detect_col}{}", row + row_shift),
                meter_cell: format!("{meter_col}{}", row + row_shift),
                accuracy_cell: format!("{accuracy_col}{}", row + row_shift),
                threshold,
                is_power_factor: false,
            });
        }
    }

    entries
}

fn add_breaker_cf_updates(
    updates: &mut Vec<CellUpdate>,
    unit_folder: &Path,
    voltage: VoltageSet,
    breaker: u8,
) {
    let step = step_for_breaker(voltage, breaker, LoadLevel::Full);
    let csv_fragment = format!("SUB_FEED_{breaker:02}_ACCURACY_TEST_DATA_AVG");
    let Some(csv_path) = find_latest_csv(unit_folder, step, &[csv_fragment]) else {
        return;
    };
    if wait_for_stable_csv(&csv_path, CSV_STABLE_FOR, CSV_MAX_WAIT).is_err() {
        return;
    }
    let Ok(table) = CsvTable::read(&csv_path) else {
        return;
    };
    let Ok(row) = table.last_data_row_after_header() else {
        return;
    };
    let columns = ["BY", "BZ", "CA"];
    let target_columns = match voltage {
        VoltageSet::V208 => ["B", "C", "D"],
        VoltageSet::V415 => ["H", "I", "J"],
    };
    let target_row = 7 + u32::from(breaker) - 1;

    for (index, column) in columns.iter().enumerate() {
        let Ok(value) = required_number(
            row,
            column,
            table.path(),
            &format!("Breaker {breaker} CF {}", target_columns[index]),
        ) else {
            continue;
        };
        updates.push(CellUpdate::text(
            "CF",
            format!("{}{}", target_columns[index], target_row),
            format!("{value:.3}"),
        ));
    }
}

fn add_breaker_load_updates(
    updates: &mut Vec<CellUpdate>,
    row: &[String],
    csv_path: &Path,
    sheet: &str,
    voltage: VoltageSet,
    breaker: u8,
    load: LoadLevel,
    thresholds: &AccuracyThresholdConfig,
) -> ProcessorAttempt<Vec<VerificationIssue>> {
    let base_row = breaker_base_row(load) + (u32::from(breaker) - 1) * 12;
    let load_thresholds = breaker_thresholds_for_load(load, thresholds);
    let mut failures = Vec::new();

    for (metric_index, metric) in ["V", "I", "P", "PF"].iter().enumerate() {
        let target_row = base_row + metric_index as u32;
        let threshold = breaker_threshold(metric, load_thresholds);

        for (phase_index, phase) in ["A", "B", "C"].iter().enumerate() {
            let meter_col = breaker_meter_columns(metric)[phase_index];
            let detect_col = breaker_detect_columns(metric)[phase_index];
            let scale = if *metric == "P" { 1000.0 } else { 1.0 };
            let raw_meter = required_number(
                row,
                meter_col,
                csv_path,
                &format!(
                    "Breaker {breaker} {} {metric} meter {phase}",
                    load.display()
                ),
            )? / scale;
            let raw_detect = required_number(
                row,
                detect_col,
                csv_path,
                &format!(
                    "Breaker {breaker} {} {metric} detect {phase}",
                    load.display()
                ),
            )? / scale;
            let meter = round_to(raw_meter, 2);
            let detect = round_to(raw_detect, 2);
            let (meter_target, detect_target, accuracy_target) =
                breaker_target_columns(phase, target_row);
            let accuracy =
                breaker_accuracy_for_verification(voltage, metric, raw_meter, raw_detect);

            updates.push(CellUpdate::number(sheet, meter_target, meter));
            updates.push(CellUpdate::number(sheet, detect_target, detect));
            updates.push(CellUpdate::number(sheet, accuracy_target.clone(), accuracy));

            if accuracy.abs() > threshold {
                failures.push(VerificationIssue::new(
                    format!(
                        "Breaker {breaker} {} {metric} phase {phase} @ {accuracy_target} = {accuracy:.4}% exceeds +/-{threshold}%",
                        load.display()
                    ),
                    sheet,
                    accuracy_target,
                ));
            }
        }
    }

    Ok(failures)
}

fn breaker_base_row(load: LoadLevel) -> u32 {
    match load {
        LoadLevel::Full => 11,
        LoadLevel::Half => 15,
        LoadLevel::Low => 19,
    }
}

fn breaker_threshold(metric: &str, thresholds: &BreakerMetricThresholds) -> f64 {
    match metric {
        "V" => thresholds.voltage,
        "I" => thresholds.current,
        "P" => thresholds.active_power,
        "PF" => thresholds.power_factor,
        _ => 0.0,
    }
}

fn breaker_accuracy_for_verification(
    voltage: VoltageSet,
    metric: &str,
    raw_meter: f64,
    raw_detect: f64,
) -> f64 {
    let (meter, detect, digits) = match voltage {
        VoltageSet::V208 => (round_to(raw_meter, 2), round_to(raw_detect, 2), 4),
        VoltageSet::V415 => (raw_meter, raw_detect, 2),
    };
    let accuracy = if metric == "PF" {
        (detect - meter) * 100.0
    } else if meter == 0.0 {
        if detect == 0.0 {
            0.0
        } else {
            9999.0
        }
    } else {
        (detect - meter) / meter * 100.0
    };

    round_to(accuracy, digits)
}

fn breaker_meter_columns(metric: &str) -> [&'static str; 3] {
    match metric {
        "V" => ["AT", "BA", "BH"],
        "I" => ["AU", "BB", "BI"],
        "P" => ["AV", "BC", "BJ"],
        "PF" => ["AX", "BE", "BL"],
        _ => unreachable!(),
    }
}

fn breaker_detect_columns(metric: &str) -> [&'static str; 3] {
    match metric {
        "V" => ["DO", "DV", "EC"],
        "I" => ["DP", "DW", "ED"],
        "P" => ["DQ", "DX", "EE"],
        "PF" => ["DS", "DZ", "EG"],
        _ => unreachable!(),
    }
}

fn breaker_target_columns(phase: &str, row: u32) -> (String, String, String) {
    match phase {
        "A" => (format!("G{row}"), format!("H{row}"), format!("I{row}")),
        "B" => (format!("J{row}"), format!("K{row}"), format!("L{row}")),
        "C" => (format!("M{row}"), format!("N{row}"), format!("O{row}")),
        _ => unreachable!(),
    }
}

#[derive(Debug, Clone, Copy)]
struct BurnInPoint {
    description: &'static str,
    column: &'static str,
    main_cell: &'static str,
    print_cell: &'static str,
    scale_by: f64,
}

fn system_burn_in_points() -> Vec<BurnInPoint> {
    vec![
        BurnInPoint {
            description: "Freq G1",
            column: "AF",
            main_cell: "E11",
            print_cell: "F30",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Active Power G1",
            column: "AC",
            main_cell: "E12",
            print_cell: "F31",
            scale_by: 1000.0,
        },
        BurnInPoint {
            description: "Apparent Power G1",
            column: "AD",
            main_cell: "E13",
            print_cell: "F32",
            scale_by: 1000.0,
        },
        BurnInPoint {
            description: "Power Factor G1",
            column: "AE",
            main_cell: "E14",
            print_cell: "F33",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage A G1",
            column: "AG",
            main_cell: "E17",
            print_cell: "F36",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current A G1",
            column: "G",
            main_cell: "E18",
            print_cell: "F37",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage B G1",
            column: "AH",
            main_cell: "H17",
            print_cell: "I36",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current B G1",
            column: "N",
            main_cell: "H18",
            print_cell: "I37",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage C G1",
            column: "AI",
            main_cell: "K17",
            print_cell: "L36",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current C G1",
            column: "U",
            main_cell: "K18",
            print_cell: "L37",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Freq G2",
            column: "BT",
            main_cell: "E20",
            print_cell: "F39",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Active Power G2",
            column: "BQ",
            main_cell: "E21",
            print_cell: "F40",
            scale_by: 1000.0,
        },
        BurnInPoint {
            description: "Apparent Power G2",
            column: "BR",
            main_cell: "E22",
            print_cell: "F41",
            scale_by: 1000.0,
        },
        BurnInPoint {
            description: "Power Factor G2",
            column: "BS",
            main_cell: "E23",
            print_cell: "F42",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage A G2",
            column: "AT",
            main_cell: "E26",
            print_cell: "F45",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current A G2",
            column: "AU",
            main_cell: "E27",
            print_cell: "F46",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage B G2",
            column: "BA",
            main_cell: "H26",
            print_cell: "I45",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current B G2",
            column: "BB",
            main_cell: "H27",
            print_cell: "I46",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage C G2",
            column: "BH",
            main_cell: "K26",
            print_cell: "L45",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current C G2",
            column: "BI",
            main_cell: "K27",
            print_cell: "L46",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Freq G3",
            column: "DH",
            main_cell: "F11",
            print_cell: "G30",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Active Power G3",
            column: "DE",
            main_cell: "F12",
            print_cell: "G31",
            scale_by: 1000.0,
        },
        BurnInPoint {
            description: "Apparent Power G3",
            column: "DF",
            main_cell: "F13",
            print_cell: "G32",
            scale_by: 1000.0,
        },
        BurnInPoint {
            description: "Power Factor G3",
            column: "DG",
            main_cell: "F14",
            print_cell: "G33",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage A G3",
            column: "CH",
            main_cell: "F17",
            print_cell: "G36",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current A G3",
            column: "CI",
            main_cell: "F18",
            print_cell: "G37",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage B G3",
            column: "CO",
            main_cell: "I17",
            print_cell: "J36",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current B G3",
            column: "CP",
            main_cell: "I18",
            print_cell: "J37",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage C G3",
            column: "CV",
            main_cell: "L17",
            print_cell: "M36",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current C G3",
            column: "CW",
            main_cell: "L18",
            print_cell: "M37",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Freq G4",
            column: "EO",
            main_cell: "F20",
            print_cell: "G39",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Active Power G4",
            column: "EL",
            main_cell: "F21",
            print_cell: "G40",
            scale_by: 1000.0,
        },
        BurnInPoint {
            description: "Apparent Power G4",
            column: "EM",
            main_cell: "F22",
            print_cell: "G41",
            scale_by: 1000.0,
        },
        BurnInPoint {
            description: "Power Factor G4",
            column: "EN",
            main_cell: "F23",
            print_cell: "G42",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage A G4",
            column: "DO",
            main_cell: "F26",
            print_cell: "G45",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current A G4",
            column: "DP",
            main_cell: "F27",
            print_cell: "G46",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage B G4",
            column: "DV",
            main_cell: "I26",
            print_cell: "J45",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current B G4",
            column: "DW",
            main_cell: "I27",
            print_cell: "J46",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Voltage C G4",
            column: "EC",
            main_cell: "L26",
            print_cell: "M45",
            scale_by: 1.0,
        },
        BurnInPoint {
            description: "Current C G4",
            column: "ED",
            main_cell: "L27",
            print_cell: "M46",
            scale_by: 1.0,
        },
    ]
}

fn burn_in_meter_columns(metric: &str) -> [&'static str; 3] {
    breaker_meter_columns(metric)
}

fn burn_in_detect_columns(metric: &str) -> [&'static str; 3] {
    breaker_detect_columns(metric)
}

fn burn_in_main_cols(phase: &str) -> (&'static str, &'static str) {
    match phase {
        "A" => ("G", "H"),
        "B" => ("J", "K"),
        "C" => ("M", "N"),
        _ => unreachable!(),
    }
}

fn burn_in_print_cols(phase: &str) -> (&'static str, &'static str) {
    match phase {
        "A" => ("H", "I"),
        "B" => ("K", "L"),
        "C" => ("N", "O"),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_thresholds() -> AccuracyThresholdConfig {
        serde_json::from_str(include_str!(
            "../../../config/report-layouts/pdu500.accuracy-thresholds.json"
        ))
        .expect("default threshold config should parse")
    }

    #[test]
    fn breaker_step_map_matches_legacy() {
        assert_eq!(step_for_breaker(VoltageSet::V208, 1, LoadLevel::Full), 18);
        assert_eq!(step_for_breaker(VoltageSet::V208, 8, LoadLevel::Low), 41);
        assert_eq!(step_for_breaker(VoltageSet::V415, 1, LoadLevel::Full), 47);
        assert_eq!(step_for_breaker(VoltageSet::V415, 8, LoadLevel::Low), 70);
    }

    #[test]
    fn system_accuracy_thresholds_follow_upgraded_scripts() {
        let thresholds = default_thresholds();
        let full = system_accuracy_entries(LoadLevel::Full, &thresholds);
        let half = system_accuracy_entries(LoadLevel::Half, &thresholds);
        let low = system_accuracy_entries(LoadLevel::Low, &thresholds);

        assert!(full
            .iter()
            .any(|entry| entry.label == "Current in A" && entry.threshold == 0.30));
        assert!(half
            .iter()
            .any(|entry| entry.label == "Current in A" && entry.threshold == 0.39));
        assert!(low
            .iter()
            .any(|entry| entry.label == "Current in A" && entry.threshold == 0.45));
        assert!(low
            .iter()
            .any(|entry| entry.label == "Active Power in" && entry.threshold == 0.75));
    }

    #[test]
    fn breaker_accuracy_matches_voltage_specific_upgraded_scripts() {
        let thresholds = default_thresholds();
        let raw_meter = 333.333;
        let raw_detect = raw_meter * 1.00304;

        let v208_accuracy =
            breaker_accuracy_for_verification(VoltageSet::V208, "V", raw_meter, raw_detect);
        let v415_accuracy =
            breaker_accuracy_for_verification(VoltageSet::V415, "V", raw_meter, raw_detect);

        assert!(v208_accuracy > breaker_threshold("V", &thresholds.breaker.full_load));
        assert_eq!(v415_accuracy, 0.3);
        assert!(v415_accuracy <= breaker_threshold("V", &thresholds.breaker.full_load));
    }

    #[test]
    fn breaker_accuracy_thresholds_follow_excel_formula_by_load() {
        let thresholds = default_thresholds();

        assert_eq!(
            breaker_threshold(
                "I",
                breaker_thresholds_for_load(LoadLevel::Full, &thresholds)
            ),
            0.3
        );
        assert_eq!(
            breaker_threshold(
                "P",
                breaker_thresholds_for_load(LoadLevel::Full, &thresholds)
            ),
            0.6
        );
        assert_eq!(
            breaker_threshold(
                "I",
                breaker_thresholds_for_load(LoadLevel::Half, &thresholds)
            ),
            0.39
        );
        assert_eq!(
            breaker_threshold(
                "P",
                breaker_thresholds_for_load(LoadLevel::Half, &thresholds)
            ),
            0.69
        );
        assert_eq!(
            breaker_threshold(
                "I",
                breaker_thresholds_for_load(LoadLevel::Low, &thresholds)
            ),
            0.45
        );
        assert_eq!(
            breaker_threshold(
                "P",
                breaker_thresholds_for_load(LoadLevel::Low, &thresholds)
            ),
            0.75
        );
    }
}
