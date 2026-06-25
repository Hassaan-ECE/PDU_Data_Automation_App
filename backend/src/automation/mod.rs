mod csv_data;
mod mapped;
mod processors;
mod reports;
pub mod tasks;
mod unit_candidates;
mod unit_state;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::Serialize;
use thiserror::Error;

use crate::config::load_layout_profile;

use self::csv_data::{detected_steps, find_latest_csv};
use self::processors::{FailureDetail, ProcessorResult};
use self::reports::{
    inspect_reports, read_transformer_serial_number, require_print_report, setup_reports,
    setup_reports_with_serial_number, write_final_operator_name, write_transformer_serial_number,
    ReportError, ReportSetup,
};
use self::tasks::{automation_tasks, find_task};
pub use self::unit_candidates::{LatestUnitCandidateResult, UnitCandidate};
use self::unit_state::TaskStateSeed;

#[derive(Debug, Error)]
pub enum AutomationError {
    #[error("{0}")]
    Report(#[from] ReportError),
    #[error("unit state could not be read or written: {0}")]
    UnitState(#[from] io::Error),
    #[error("unknown automation task id: {0}")]
    UnknownTask(String),
    #[error("{0}")]
    OpenReport(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationCommandError {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
}

impl AutomationCommandError {
    fn validation(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    fn report(
        code: impl Into<String>,
        message: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Some(details.into()),
        }
    }

    fn from_report_error(error: ReportError) -> Self {
        match &error {
            ReportError::Io(source) if is_locked_file_error(source) => Self::report(
                "workbook_locked",
                "The main report workbook is locked or open in another program. Close Excel or ATS access to the report and try again.",
                error.to_string(),
            ),
            ReportError::WorkbookLocked(_) => Self::report(
                "workbook_locked",
                "The main report workbook is locked by another app operation. Wait for the current write to finish and try again.",
                error.to_string(),
            ),
            ReportError::Zip(zip::result::ZipError::Io(source))
                if is_locked_file_error(source) =>
            {
                Self::report(
                    "workbook_locked",
                    "The main report workbook is locked or open in another program. Close Excel or ATS access to the report and try again.",
                    error.to_string(),
                )
            }
            ReportError::Io(_) => Self::report(
                "report_io_failed",
                "The report workbook could not be read or written.",
                error.to_string(),
            ),
            ReportError::MainReportMissing(_) => Self::report(
                "main_report_missing",
                "The main report workbook was not found. Check the selected unit folder and report template setup.",
                error.to_string(),
            ),
            ReportError::SheetMissing(sheet) => Self::report(
                "report_sheet_missing",
                format!("The main report workbook is missing the required sheet '{sheet}'."),
                error.to_string(),
            ),
            ReportError::InvalidCell(cell) => Self::report(
                "report_cell_invalid",
                format!("The required report cell '{cell}' is invalid."),
                error.to_string(),
            ),
            ReportError::UnitFolderMissing(_) => Self::report(
                "unit_folder_missing",
                "The selected unit folder does not exist.",
                error.to_string(),
            ),
            _ => Self::report(
                "report_write_failed",
                "The Transformer SN could not be written to the report workbook.",
                error.to_string(),
            ),
        }
    }

    fn from_print_report_error(error: ReportError) -> Self {
        match &error {
            ReportError::Io(source) if is_locked_file_error(source) => Self::report(
                "workbook_locked",
                "The print report workbook is locked or open in another program. Close Excel or ATS access to the report and try again.",
                error.to_string(),
            ),
            ReportError::WorkbookLocked(_) => Self::report(
                "workbook_locked",
                "The print report workbook is locked by another app operation. Wait for the current write to finish and try again.",
                error.to_string(),
            ),
            ReportError::Zip(zip::result::ZipError::Io(source))
                if is_locked_file_error(source) =>
            {
                Self::report(
                    "workbook_locked",
                    "The print report workbook is locked or open in another program. Close Excel or ATS access to the report and try again.",
                    error.to_string(),
                )
            }
            ReportError::Io(_) => Self::report(
                "report_io_failed",
                "The print report workbook could not be read or written.",
                error.to_string(),
            ),
            ReportError::PrintReportMissing(_) => Self::report(
                "print_report_missing",
                "The print report workbook was not found. Check the selected unit folder and report template setup.",
                error.to_string(),
            ),
            ReportError::MainReportMissing(_) => Self::report(
                "main_report_missing",
                "The main report workbook was not found. Check the selected unit folder and report template setup.",
                error.to_string(),
            ),
            ReportError::SheetMissing(sheet) => Self::report(
                "report_sheet_missing",
                format!("The print report workbook is missing the required sheet '{sheet}'."),
                error.to_string(),
            ),
            ReportError::InvalidCell(cell) => Self::report(
                "report_cell_invalid",
                format!("The required print report cell '{cell}' is invalid."),
                error.to_string(),
            ),
            ReportError::UnitFolderMissing(_) => Self::report(
                "unit_folder_missing",
                "The selected unit folder does not exist.",
                error.to_string(),
            ),
            _ => Self::report(
                "print_report_write_failed",
                "The final operator name could not be written to the print report workbook.",
                error.to_string(),
            ),
        }
    }

    fn from_unit_state_error(error: io::Error) -> Self {
        let code = if error.kind() == io::ErrorKind::InvalidData {
            "unit_state_corrupt"
        } else if is_unit_state_locked_error(&error) {
            "unit_state_locked"
        } else if error.kind() == io::ErrorKind::PermissionDenied {
            "unit_state_unreadable"
        } else {
            "unit_state_io_failed"
        };
        let message = match code {
            "unit_state_corrupt" => {
                "The saved unit state is corrupt and cannot be used. Stop and repair unit_state.json before continuing."
            }
            "unit_state_locked" => {
                "The saved unit state is locked by another app operation. Wait for the current state write to finish and try again."
            }
            "unit_state_unreadable" => {
                "The saved unit state could not be read or written because Windows denied access."
            }
            _ => "The saved unit state could not be read or written.",
        };

        Self::report(code, message, error.to_string())
    }

    fn print_dialog(message: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            code: "print_dialog_failed".to_string(),
            message: message.into(),
            details: Some(details.into()),
        }
    }

    fn print_readiness(readiness: &PrintReadinessResult) -> Self {
        let details = serde_json::to_string(&readiness.blocking_issues).ok();

        Self {
            code: "print_validation_failed".to_string(),
            message: readiness.message.clone(),
            details,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UnitFolderSummary {
    pub unit_folder: String,
    pub serial_number: Option<String>,
    pub report_path: Option<String>,
    pub print_report_path: Option<String>,
    pub detected_count: usize,
    pub tasks: Vec<TaskStatus>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskStatus {
    pub task_id: String,
    pub label: String,
    pub step: String,
    pub state: String,
    pub detected_steps: Vec<u16>,
    pub latest_csv: Option<String>,
    pub latest_csv_created_ms: Option<u64>,
    pub latest_csv_readable: Option<bool>,
    pub timer_start_ms: Option<u64>,
    pub processable: bool,
    pub match_reason: String,
    pub source_csv_path: Option<String>,
    pub csv_fingerprint: Option<String>,
    pub processed_at: Option<String>,
    pub result: Option<String>,
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskProcessResult {
    pub task_id: String,
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

#[derive(Debug, Clone, Serialize)]
pub struct PrintReadinessResult {
    pub ready: bool,
    pub message: String,
    pub blocking_issues: Vec<PrintReadinessBlocker>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrintReadinessBlocker {
    pub task_id: Option<String>,
    pub label: Option<String>,
    pub code: String,
    pub reason: String,
}

pub fn setup_unit_folder(unit_folder: String) -> Result<UnitFolderSummary, AutomationError> {
    let unit_folder = PathBuf::from(unit_folder);
    let report_setup = setup_reports(&unit_folder)?;

    build_summary(&unit_folder, report_setup).map_err(AutomationError::UnitState)
}

pub fn find_latest_unit_candidate() -> LatestUnitCandidateResult {
    unit_candidates::latest_unit_candidate()
}

pub fn setup_unit_folder_with_transformer_sn(
    unit_folder: String,
    unit_serial_number: Option<String>,
    transformer_sn: String,
) -> Result<UnitFolderSummary, AutomationCommandError> {
    let transformer_sn = transformer_sn.trim();

    if transformer_sn.is_empty() {
        return Err(AutomationCommandError::validation(
            "blank_transformer_sn",
            "Transformer SN is required before setup can continue.",
        ));
    }

    let unit_folder = PathBuf::from(unit_folder);
    let unit_serial_number = unit_serial_number
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let report_setup = setup_reports_with_serial_number(&unit_folder, unit_serial_number)
        .map_err(AutomationCommandError::from_report_error)?;

    write_transformer_serial_number(&unit_folder, transformer_sn)
        .map_err(AutomationCommandError::from_report_error)?;

    build_summary(&unit_folder, report_setup).map_err(AutomationCommandError::from_unit_state_error)
}

pub fn save_transformer_sn(
    unit_folder: String,
    transformer_sn: String,
) -> Result<(), AutomationCommandError> {
    if unit_folder.trim().is_empty() {
        return Err(AutomationCommandError::validation(
            "unit_folder_missing",
            "Select Test Unit before saving Transformer SN.",
        ));
    }

    let transformer_sn = transformer_sn.trim();

    if transformer_sn.is_empty() {
        return Err(AutomationCommandError::validation(
            "blank_transformer_sn",
            "Transformer SN is required before it can be saved.",
        ));
    }

    let unit_folder = PathBuf::from(unit_folder);

    if !unit_folder.is_dir() {
        return Err(AutomationCommandError::report(
            "unit_folder_missing",
            "The selected unit folder does not exist.",
            format!("unit folder does not exist: {}", unit_folder.display()),
        ));
    }

    write_transformer_serial_number(&unit_folder, transformer_sn)
        .map_err(AutomationCommandError::from_report_error)?;

    Ok(())
}

pub fn save_final_operator_name(
    unit_folder: String,
    operator_name: String,
) -> Result<String, AutomationCommandError> {
    if unit_folder.trim().is_empty() {
        return Err(AutomationCommandError::validation(
            "unit_folder_missing",
            "Select Test Unit before printing the report.",
        ));
    }

    let operator_name = operator_name.trim();

    if operator_name.is_empty() {
        return Err(AutomationCommandError::validation(
            "blank_operator_name",
            "Operator name is required before printing the report.",
        ));
    }

    let unit_folder = PathBuf::from(unit_folder);
    let readiness = validate_ready_for_print_path(&unit_folder)?;
    if !readiness.ready {
        return Err(AutomationCommandError::print_readiness(&readiness));
    }

    let print_report_path = write_final_operator_name(&unit_folder, operator_name)
        .map_err(AutomationCommandError::from_print_report_error)?;

    Ok(print_report_path.display().to_string())
}

pub fn open_print_report_dialog(unit_folder: String) -> Result<(), AutomationCommandError> {
    if unit_folder.trim().is_empty() {
        return Err(AutomationCommandError::validation(
            "unit_folder_missing",
            "Select Test Unit before printing the report.",
        ));
    }

    let unit_folder = PathBuf::from(unit_folder);

    if !unit_folder.is_dir() {
        return Err(AutomationCommandError::report(
            "unit_folder_missing",
            "The selected unit folder does not exist.",
            format!("unit folder does not exist: {}", unit_folder.display()),
        ));
    }

    let readiness = validate_ready_for_print_path(&unit_folder)?;
    if !readiness.ready {
        return Err(AutomationCommandError::print_readiness(&readiness));
    }

    let print_report_path = require_print_report(&unit_folder)
        .map_err(AutomationCommandError::from_print_report_error)?;

    open_excel_print_dialog(&print_report_path).map_err(|error| {
        AutomationCommandError::print_dialog(
            "The Excel print dialog could not be opened for the print report.",
            error.to_string(),
        )
    })
}

pub fn validate_ready_for_print(
    unit_folder: String,
) -> Result<PrintReadinessResult, AutomationCommandError> {
    if unit_folder.trim().is_empty() {
        return Err(AutomationCommandError::validation(
            "unit_folder_missing",
            "Select Test Unit before printing the report.",
        ));
    }

    let unit_folder = PathBuf::from(unit_folder);

    if !unit_folder.is_dir() {
        return Err(AutomationCommandError::report(
            "unit_folder_missing",
            "The selected unit folder does not exist.",
            format!("unit folder does not exist: {}", unit_folder.display()),
        ));
    }

    validate_ready_for_print_path(&unit_folder)
}

fn validate_ready_for_print_path(
    unit_folder: &Path,
) -> Result<PrintReadinessResult, AutomationCommandError> {
    let mut blocking_issues = Vec::new();

    require_print_report(unit_folder).map_err(AutomationCommandError::from_print_report_error)?;

    if read_transformer_serial_number(unit_folder)
        .map_err(AutomationCommandError::from_print_report_error)?
        .is_none()
    {
        blocking_issues.push(PrintReadinessBlocker {
            task_id: None,
            label: None,
            code: "transformer_sn_missing".to_string(),
            reason: "Transformer SN is missing from the saved main report.".to_string(),
        });
    }

    let seeds = automation_tasks()
        .into_iter()
        .map(|task| TaskStateSeed {
            task_id: task.id,
            state: "off".to_string(),
            source_csv_path: None,
            csv_fingerprint: None,
        })
        .collect::<Vec<_>>();

    let state = unit_state::load_or_ensure_task_entries(unit_folder, &seeds)
        .map_err(AutomationCommandError::from_unit_state_error)?;

    for task in automation_tasks() {
        let Some(entry) = state.tasks.get(&task.id) else {
            blocking_issues.push(PrintReadinessBlocker {
                task_id: Some(task.id.clone()),
                label: Some(task.label.clone()),
                code: "task_state_missing".to_string(),
                reason: "Task has no persisted processing state.".to_string(),
            });
            continue;
        };

        if entry.is_print_ready() {
            continue;
        }

        blocking_issues.push(PrintReadinessBlocker {
            task_id: Some(task.id.clone()),
            label: Some(task.label.clone()),
            code: format!("task_{}", entry.state),
            reason: task_blocking_reason(&entry.state),
        });
    }

    let ready = blocking_issues.is_empty();
    let message = if ready {
        "Ready to print.".to_string()
    } else {
        format!(
            "Report is not ready to print. {} blocking issue{} must be resolved.",
            blocking_issues.len(),
            if blocking_issues.len() == 1 { "" } else { "s" }
        )
    };

    Ok(PrintReadinessResult {
        ready,
        message,
        blocking_issues,
    })
}

fn task_blocking_reason(state: &str) -> String {
    match state {
        "fail" => "Task failed and has not been explicitly accepted.".to_string(),
        "waiting" => "Task is still waiting for stable CSV data.".to_string(),
        "warning" => "Task has a warning or missing CSV and has not been resolved.".to_string(),
        "detected" => "Task CSV was detected but has not been processed successfully.".to_string(),
        "off" => "Task has not been detected or processed successfully.".to_string(),
        other => format!("Task is not print-ready (state: {other})."),
    }
}

pub fn scan_unit_folder(unit_folder: String) -> Result<UnitFolderSummary, AutomationError> {
    let unit_folder = PathBuf::from(unit_folder);
    let report_setup = inspect_reports(&unit_folder)?;

    build_summary(&unit_folder, report_setup).map_err(AutomationError::UnitState)
}

pub fn process_task(
    unit_folder: String,
    task_id: String,
) -> Result<TaskProcessResult, AutomationError> {
    let unit_folder = PathBuf::from(unit_folder);
    let task = find_task(&task_id).ok_or_else(|| AutomationError::UnknownTask(task_id.clone()))?;
    let state = unit_state::load_or_default(&unit_folder)?;
    let already_processed_fingerprint = state
        .tasks
        .get(&task_id)
        .and_then(|entry| entry.already_processed_fingerprint())
        .map(ToOwned::to_owned);
    let result = process_task_with_profile_mapping(
        &task_id,
        &unit_folder,
        already_processed_fingerprint.as_deref(),
    )
    .unwrap_or_else(|| {
        processors::process_task(
            &task,
            &unit_folder,
            already_processed_fingerprint.as_deref(),
        )
    });

    unit_state::record_processor_result(&unit_folder, &task_id, &result)?;

    Ok(to_task_process_result(task_id, result))
}

pub fn open_report_path(unit_folder: String, path: String) -> Result<(), AutomationError> {
    let report_path = validate_report_path(&unit_folder, &path)?;

    open_path_with_default_app(&report_path)
}

pub fn open_report_location(
    unit_folder: String,
    path: String,
    sheet: String,
    cell: String,
) -> Result<(), AutomationError> {
    let report_path = validate_report_path(&unit_folder, &path)?;
    validate_sheet_name(&sheet)?;
    validate_cell_reference(&cell)?;

    open_excel_at_location(&report_path, &sheet, &cell)
}

fn build_summary(unit_folder: &Path, report_setup: ReportSetup) -> io::Result<UnitFolderSummary> {
    let detected = detected_steps(unit_folder);
    let mut steps_to_paths = HashMap::<u16, Vec<PathBuf>>::new();

    for (step, path) in detected {
        steps_to_paths.entry(step).or_default().push(path);
    }

    let mut detected_task_ids = HashSet::<String>::new();
    let automation_tasks = automation_tasks();
    let seeds = automation_tasks
        .iter()
        .map(|task| {
            let detected_for_task = task
                .detection_steps
                .iter()
                .copied()
                .filter(|step| steps_to_paths.contains_key(step))
                .collect::<Vec<_>>();
            let csv_match = task_csv_match(unit_folder, task);

            TaskStateSeed {
                task_id: task.id.clone(),
                state: if detected_for_task.is_empty() {
                    "off".to_string()
                } else {
                    "detected".to_string()
                },
                source_csv_path: csv_match
                    .path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                csv_fingerprint: None,
            }
        })
        .collect::<Vec<_>>();

    let warnings = report_setup.warnings;
    let state = unit_state::load_or_ensure_task_entries(unit_folder, &seeds)?;

    let tasks = automation_tasks
        .into_iter()
        .map(|task| {
            let detected_for_task = task
                .detection_steps
                .iter()
                .copied()
                .filter(|step| steps_to_paths.contains_key(step))
                .collect::<Vec<_>>();
            let latest_csv_info = latest_csv_for_steps(&task.detection_steps, &steps_to_paths);
            let latest_csv = latest_csv_info
                .as_ref()
                .map(|info| info.path.display().to_string());
            let latest_csv_created_ms = latest_csv_info.as_ref().and_then(|info| info.created_ms);
            let latest_csv_readable = latest_csv_info.as_ref().map(|info| info.readable);
            let timer_start_ms =
                timer_start_millis_for_steps(&task.detection_steps, &steps_to_paths)
                    .or(latest_csv_created_ms);
            let detected_state = if detected_for_task.is_empty() {
                "off"
            } else {
                detected_task_ids.insert(task.id.clone());
                "detected"
            };
            let csv_match = task_csv_match(unit_folder, &task);
            let persisted = state.tasks.get(&task.id);
            let state = persisted
                .map(|entry| merged_summary_state(entry, detected_state))
                .unwrap_or_else(|| detected_state.to_string());

            TaskStatus {
                task_id: task.id,
                label: task.label,
                step: task.step_display,
                state,
                detected_steps: detected_for_task,
                latest_csv,
                latest_csv_created_ms,
                latest_csv_readable,
                timer_start_ms,
                processable: csv_match.processable,
                match_reason: csv_match.reason,
                source_csv_path: persisted.and_then(|entry| entry.source_csv_path.clone()),
                csv_fingerprint: persisted.and_then(|entry| entry.csv_fingerprint.clone()),
                processed_at: persisted.and_then(|entry| entry.processed_at.clone()),
                result: persisted.and_then(|entry| entry.result.clone()),
                accepted: persisted.is_some_and(|entry| entry.accepted.accepted),
            }
        })
        .collect::<Vec<_>>();

    Ok(UnitFolderSummary {
        unit_folder: unit_folder.display().to_string(),
        serial_number: report_setup.serial_number,
        report_path: report_setup.report_path,
        print_report_path: report_setup.print_report_path,
        detected_count: detected_task_ids.len(),
        tasks,
        warnings,
    })
}

#[derive(Debug, Clone)]
struct TaskCsvMatch {
    path: Option<PathBuf>,
    processable: bool,
    reason: String,
}

fn merged_summary_state(entry: &unit_state::UnitTaskState, detected_state: &str) -> String {
    if entry.accepted.accepted {
        return "pass".to_string();
    }

    match entry.state.as_str() {
        "pass" | "fail" | "waiting" | "warning" => entry.state.clone(),
        _ => detected_state.to_string(),
    }
}

fn task_csv_match(unit_folder: &Path, task: &tasks::AutomationTask) -> TaskCsvMatch {
    if let Some(mapped_path) = mapped_csv_match(unit_folder, &task.id) {
        let readable = csv_file_is_readable(&mapped_path);
        return TaskCsvMatch {
            reason: if readable {
                format!("matched configured CSV pattern: {}", mapped_path.display())
            } else {
                format!(
                    "configured CSV is not readable yet: {}",
                    mapped_path.display()
                )
            },
            processable: readable,
            path: Some(mapped_path),
        };
    }

    let Some((step, fragments)) = built_in_csv_requirement(task) else {
        return TaskCsvMatch {
            path: None,
            processable: false,
            reason: "task has no CSV processor requirement".to_string(),
        };
    };

    match find_latest_csv(unit_folder, step, &fragments) {
        Some(path) => {
            let readable = csv_file_is_readable(&path);
            TaskCsvMatch {
                reason: if readable {
                    format!("matched required STEP{step} CSV: {}", path.display())
                } else {
                    format!(
                        "required STEP{step} CSV is not readable yet: {}",
                        path.display()
                    )
                },
                processable: readable,
                path: Some(path),
            }
        }
        None => TaskCsvMatch {
            path: None,
            processable: false,
            reason: format!(
                "no processable STEP{step} CSV found with required fragment(s): {}",
                fragments.join(", ")
            ),
        },
    }
}

fn mapped_csv_match(unit_folder: &Path, task_id: &str) -> Option<PathBuf> {
    let profile = load_layout_profile().ok()?;
    let task = profile
        .task_groups
        .iter()
        .flat_map(|group| group.tasks.iter())
        .find(|task| task.id == task_id && !task.mappings.is_empty())?;

    mapped::find_latest_file_by_pattern(unit_folder, &task.csv_pattern)
}

fn built_in_csv_requirement(task: &tasks::AutomationTask) -> Option<(u16, Vec<String>)> {
    use tasks::TaskKind;

    match task.kind {
        TaskKind::Transformer { voltage } => Some((
            match voltage {
                tasks::VoltageSet::V208 => 14,
                tasks::VoltageSet::V415 => 43,
            },
            vec!["TRANSFORMER_TEST_DATA_AVG".to_string()],
        )),
        TaskKind::System { voltage, load } => Some((
            tasks::step_for_system(voltage, load),
            vec!["SYSTEM_ACCURACY_TEST_DATA_AVG".to_string()],
        )),
        TaskKind::Breaker {
            voltage,
            breaker,
            load,
        } => Some((
            tasks::step_for_breaker(voltage, breaker, load),
            vec![format!("SUB_FEED_{breaker:02}_ACCURACY_TEST_DATA_AVG")],
        )),
        TaskKind::SystemBurnIn => Some((72, vec!["SYSTEM_ACCURACY_TEST_DATA_AVG".to_string()])),
        TaskKind::BreakerBurnIn { breaker } => Some((
            72 + u16::from(breaker),
            vec![format!("SUB_FEED_{breaker:02}_ACCURACY_TEST_DATA_AVG")],
        )),
    }
}

#[derive(Debug, Clone)]
struct CsvFileInfo {
    path: PathBuf,
    modified: SystemTime,
    created_ms: Option<u64>,
    readable: bool,
}

fn latest_csv_for_steps(
    steps: &[u16],
    steps_to_paths: &HashMap<u16, Vec<PathBuf>>,
) -> Option<CsvFileInfo> {
    steps
        .iter()
        .filter_map(|step| steps_to_paths.get(step))
        .filter_map(|paths| latest_csv_for_paths(paths))
        .max_by_key(|info| info.modified)
}

fn timer_start_millis_for_steps(
    steps: &[u16],
    steps_to_paths: &HashMap<u16, Vec<PathBuf>>,
) -> Option<u64> {
    steps.iter().find_map(|step| {
        steps_to_paths
            .get(step)
            .and_then(|paths| latest_csv_for_paths(paths))
            .and_then(|info| info.created_ms)
    })
}

fn latest_csv_for_paths(paths: &[PathBuf]) -> Option<CsvFileInfo> {
    paths
        .iter()
        .filter_map(|path| {
            let metadata = path.metadata().ok()?;
            let modified = metadata.modified().ok()?;
            let created_ms = csv_start_time_millis(&metadata);
            let readable = csv_file_is_readable(path);

            Some(CsvFileInfo {
                path: path.clone(),
                modified,
                created_ms,
                readable,
            })
        })
        .max_by_key(|info| info.modified)
}

fn csv_file_is_readable(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut buffer = [0_u8; 1];

    file.read(&mut buffer).is_ok()
}

fn is_locked_file_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33 | 1224))
        || matches!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock
        )
}

fn is_unit_state_locked_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33 | 1224))
        || error.kind() == std::io::ErrorKind::WouldBlock
}

fn csv_start_time_millis(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .created()
        .or_else(|_| metadata.modified())
        .ok()
        .and_then(system_time_millis)
}

fn system_time_millis(time: SystemTime) -> Option<u64> {
    let millis = time.duration_since(UNIX_EPOCH).ok()?.as_millis();

    u64::try_from(millis).ok()
}

fn to_task_process_result(task_id: String, result: ProcessorResult) -> TaskProcessResult {
    TaskProcessResult {
        task_id,
        state: result.state,
        code: result.code,
        message: result.message,
        log: result.log,
        report_path: result.report_path,
        print_report_path: result.print_report_path,
        failure: result.failure,
        source_csv_path: result.source_csv_path,
        csv_fingerprint: result.csv_fingerprint,
    }
}

fn process_task_with_profile_mapping(
    task_id: &str,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
) -> Option<ProcessorResult> {
    let profile = load_layout_profile().ok()?;
    let task = profile
        .task_groups
        .iter()
        .flat_map(|group| group.tasks.iter())
        .find(|task| task.id == task_id)?;

    if task.mappings.is_empty() {
        return None;
    }

    Some(mapped::process_task(
        &profile,
        task,
        unit_folder,
        already_processed_fingerprint,
    ))
}

fn validate_report_path(unit_folder: &str, path: &str) -> Result<PathBuf, AutomationError> {
    let unit_folder = canonicalize_path(unit_folder)
        .map_err(|error| AutomationError::OpenReport(error.to_string()))?;
    let report_path =
        canonicalize_path(path).map_err(|error| AutomationError::OpenReport(error.to_string()))?;

    if !unit_folder.is_dir() {
        return Err(AutomationError::OpenReport(format!(
            "unit folder does not exist: {}",
            unit_folder.display()
        )));
    }

    if !report_path.is_file() {
        return Err(AutomationError::OpenReport(format!(
            "report file does not exist: {}",
            report_path.display()
        )));
    }

    if !report_path.starts_with(&unit_folder) {
        return Err(AutomationError::OpenReport(format!(
            "report is outside the selected unit folder: {}",
            report_path.display()
        )));
    }

    let allowed = report_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "xlsx" | "xlsm" | "xls"
            )
        });

    if !allowed {
        return Err(AutomationError::OpenReport(format!(
            "report file must be an Excel workbook: {}",
            report_path.display()
        )));
    }

    Ok(report_path)
}

fn canonicalize_path(path: &str) -> std::io::Result<PathBuf> {
    fs::canonicalize(PathBuf::from(path))
}

fn validate_sheet_name(sheet: &str) -> Result<(), AutomationError> {
    let trimmed = sheet.trim();

    if trimmed.is_empty() || trimmed.len() > 31 {
        return Err(AutomationError::OpenReport(format!(
            "invalid Excel sheet name: {sheet}"
        )));
    }

    if trimmed
        .chars()
        .any(|ch| matches!(ch, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
    {
        return Err(AutomationError::OpenReport(format!(
            "invalid Excel sheet name: {sheet}"
        )));
    }

    Ok(())
}

fn validate_cell_reference(cell: &str) -> Result<(), AutomationError> {
    let cell_re = Regex::new(r"^[A-Z]{1,3}[1-9][0-9]{0,6}$").expect("cell regex is valid");

    if cell_re.is_match(cell.trim()) {
        return Ok(());
    }

    Err(AutomationError::OpenReport(format!(
        "invalid Excel cell reference: {cell}"
    )))
}

#[cfg(target_os = "windows")]
fn open_path_with_default_app(path: &Path) -> Result<(), AutomationError> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map_err(|error| {
            AutomationError::OpenReport(format!(
                "failed to open report {}: {error}",
                path.display()
            ))
        })?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn open_path_with_default_app(path: &Path) -> Result<(), AutomationError> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|error| {
            AutomationError::OpenReport(format!(
                "failed to open report {}: {error}",
                path.display()
            ))
        })?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn open_excel_at_location(path: &Path, sheet: &str, cell: &str) -> Result<(), AutomationError> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$path = $env:PDU_REPORT_PATH
$sheet = $env:PDU_REPORT_SHEET
$cell = $env:PDU_REPORT_CELL
try {
  $excel = [Runtime.InteropServices.Marshal]::GetActiveObject('Excel.Application')
} catch {
  $excel = New-Object -ComObject Excel.Application
}
$excel.Visible = $true
$workbook = $null
foreach ($candidate in @($excel.Workbooks)) {
  if ([string]::Equals($candidate.FullName, $path, [System.StringComparison]::OrdinalIgnoreCase)) {
    $workbook = $candidate
    break
  }
}
if ($null -eq $workbook) {
  $workbook = $excel.Workbooks.Open($path)
}
$worksheet = $workbook.Worksheets.Item($sheet)
$worksheet.Activate()
$range = $worksheet.Range($cell)
$range.Select()
if ($excel.ActiveWindow -ne $null) {
  $excel.ActiveWindow.ScrollRow = [Math]::Max(1, $range.Row - 5)
  $excel.ActiveWindow.ScrollColumn = [Math]::Max(1, $range.Column - 3)
}
"#;

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            SCRIPT,
        ])
        .env("PDU_REPORT_PATH", path)
        .env("PDU_REPORT_SHEET", sheet)
        .env("PDU_REPORT_CELL", cell)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| {
            AutomationError::OpenReport(format!(
                "failed to open Excel location {}!{} in {}: {error}",
                sheet,
                cell,
                path.display()
            ))
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };

    Err(AutomationError::OpenReport(format!(
        "failed to open Excel location {sheet}!{cell} in {}{}",
        path.display(),
        if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        }
    )))
}

#[cfg(not(target_os = "windows"))]
fn open_excel_at_location(path: &Path, _sheet: &str, _cell: &str) -> Result<(), AutomationError> {
    open_path_with_default_app(path)
}

#[cfg(target_os = "windows")]
fn open_excel_print_dialog(path: &Path) -> Result<(), AutomationError> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$path = $env:PDU_REPORT_PATH
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class PduExcelWindowFocus {
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@
try {
  $excel = [Runtime.InteropServices.Marshal]::GetActiveObject('Excel.Application')
} catch {
  $excel = New-Object -ComObject Excel.Application
}
$excel.Visible = $true
$workbook = $null
foreach ($candidate in @($excel.Workbooks)) {
  if ([string]::Equals($candidate.FullName, $path, [System.StringComparison]::OrdinalIgnoreCase)) {
    $workbook = $candidate
    break
  }
}
if ($null -eq $workbook) {
  $workbook = $excel.Workbooks.Open($path)
}
$workbook.Activate()
try {
  $workbook.Windows.Item(1).Activate()
} catch {
  Write-Output ("Activating workbook window failed: " + $_.Exception.Message)
}

function Focus-ExcelWindow {
  try {
    $hwnd = [IntPtr]$excel.Hwnd
    if ($hwnd -ne [IntPtr]::Zero) {
      [void][PduExcelWindowFocus]::ShowWindow($hwnd, 9)
      [void][PduExcelWindowFocus]::BringWindowToTop($hwnd)
      [void][PduExcelWindowFocus]::SetForegroundWindow($hwnd)
    }
  } catch {
    Write-Output ("Focusing Excel window failed: " + $_.Exception.Message)
  }
}

try {
  $activeSheetSelected = $false
  foreach ($worksheet in @($workbook.Worksheets)) {
    if ($worksheet.Visible -eq -1) {
      if (-not $activeSheetSelected) {
        $worksheet.Select($true)
        $activeSheetSelected = $true
      } else {
        $worksheet.Select($false)
      }
    }
  }

  if (-not $activeSheetSelected) {
    throw 'The workbook has no visible worksheets to print.'
  }
} catch {
  Write-Output ("Selecting all worksheets failed: " + $_.Exception.Message)
}

$executeMsoError = $null
try {
  Focus-ExcelWindow
  $excel.CommandBars.ExecuteMso('PrintPreviewAndPrint')
  Start-Sleep -Milliseconds 200
  Focus-ExcelWindow
  exit 0
} catch {
  $executeMsoError = $_.Exception.Message
}

$missing = [Type]::Missing
try {
  Focus-ExcelWindow
  [void]$workbook.PrintOut($missing, $missing, $missing, $true, $missing, $false, $true, $missing, $false)
  Start-Sleep -Milliseconds 200
  Focus-ExcelWindow
  exit 0
} catch {
  throw ("PrintPreviewAndPrint failed: " + $executeMsoError + "; workbook print preview failed: " + $_.Exception.Message)
}
"#;

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            SCRIPT,
        ])
        .env("PDU_REPORT_PATH", path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| {
            AutomationError::OpenReport(format!(
                "failed to open Excel print dialog for {}: {error}",
                path.display()
            ))
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };

    Err(AutomationError::OpenReport(format!(
        "failed to open Excel print dialog for {}{}",
        path.display(),
        if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        }
    )))
}

#[cfg(not(target_os = "windows"))]
fn open_excel_print_dialog(path: &Path) -> Result<(), AutomationError> {
    Err(AutomationError::OpenReport(format!(
        "Excel print dialog is only supported on Windows with Excel installed: {}",
        path.display()
    )))
}

#[cfg(test)]
mod smoke_tests {
    use std::fs;
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::path::Path;

    use tempfile::TempDir;
    use walkdir::WalkDir;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipArchive, ZipWriter};

    use super::*;

    #[test]
    fn report_open_validation_accepts_workbook_inside_unit_folder() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join("report.xlsx");
        File::create(&report).expect("report file");

        let validated = validate_report_path(
            &unit_folder.display().to_string(),
            &report.display().to_string(),
        )
        .expect("report should be valid");

        assert_eq!(
            validated,
            fs::canonicalize(report).expect("canonical report")
        );
    }

    #[test]
    fn report_open_validation_rejects_workbook_outside_unit_folder() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        let other_folder = temp.path().join("other");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        fs::create_dir_all(&other_folder).expect("other folder");
        let report = other_folder.join("report.xlsx");
        File::create(&report).expect("report file");

        let error = validate_report_path(
            &unit_folder.display().to_string(),
            &report.display().to_string(),
        )
        .expect_err("outside report should be rejected");

        assert!(error
            .to_string()
            .contains("outside the selected unit folder"));
    }

    #[test]
    fn timer_start_prefers_first_detection_step_for_multistep_tasks() {
        let temp = TempDir::new().expect("temp dir");
        let step71 = temp.path().join("unit_STEP71_system.csv");
        let step72 = temp.path().join("unit_STEP72_system.csv");

        fs::write(&step71, "a,b\n1,2\n").expect("write step71");
        fs::write(&step72, "a,b\n3,4\n").expect("write step72");

        let mut steps_to_paths = HashMap::<u16, Vec<PathBuf>>::new();
        steps_to_paths.insert(71, vec![step71.clone()]);
        steps_to_paths.insert(72, vec![step72]);

        let expected = latest_csv_for_paths(&[step71])
            .and_then(|info| info.created_ms)
            .expect("step71 should have a start time");

        assert_eq!(
            timer_start_millis_for_steps(&[71, 72], &steps_to_paths),
            Some(expected)
        );
    }

    #[test]
    fn transformer_setup_rejects_blank_transformer_sn() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");

        let error = setup_unit_folder_with_transformer_sn(
            unit_folder.display().to_string(),
            Some("262343000072".to_string()),
            "   ".to_string(),
        )
        .expect_err("blank transformer SN should fail");

        assert_eq!(error.code, "blank_transformer_sn");
        assert!(error.message.contains("Transformer SN is required"));
    }

    #[test]
    fn transformer_sn_save_rejects_missing_selected_unit_folder() {
        let temp = TempDir::new().expect("temp dir");
        let missing_unit_folder = temp.path().join("262343000072");
        let unit_folder = temp.path().join("262343000073");
        fs::create_dir_all(&unit_folder).expect("unit folder");

        let blank_error = save_transformer_sn("   ".to_string(), "TX-12345".to_string())
            .expect_err("blank selected folder should fail");
        assert_eq!(blank_error.code, "unit_folder_missing");

        let blank_sn_error =
            save_transformer_sn(unit_folder.display().to_string(), "   ".to_string())
                .expect_err("blank transformer SN should fail");
        assert_eq!(blank_sn_error.code, "blank_transformer_sn");

        let error = save_transformer_sn(
            missing_unit_folder.display().to_string(),
            "TX-12345".to_string(),
        )
        .expect_err("missing selected folder should fail");

        assert_eq!(error.code, "unit_folder_missing");
        assert!(error.message.contains("selected unit folder"));

        let missing_report_error =
            save_transformer_sn(unit_folder.display().to_string(), "TX-12345".to_string())
                .expect_err("missing report should fail");
        assert_eq!(missing_report_error.code, "main_report_missing");
    }

    #[test]
    fn transformer_setup_maps_report_errors_to_stable_codes() {
        let locked = AutomationCommandError::from_report_error(ReportError::Io(
            io::Error::from_raw_os_error(32),
        ));
        assert_eq!(locked.code, "workbook_locked");

        let missing = AutomationCommandError::from_report_error(ReportError::MainReportMissing(
            "C:/PDU500/262343000072".to_string(),
        ));
        assert_eq!(missing.code, "main_report_missing");

        let missing_sheet = AutomationCommandError::from_report_error(ReportError::SheetMissing(
            "Test Summary".to_string(),
        ));
        assert_eq!(missing_sheet.code, "report_sheet_missing");
        assert!(missing_sheet.message.contains("Test Summary"));
    }

    #[test]
    fn final_operator_name_rejects_blank_operator_name() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");

        let error = save_final_operator_name(unit_folder.display().to_string(), "   ".to_string())
            .expect_err("blank operator name should fail");

        assert_eq!(error.code, "blank_operator_name");
        assert!(error.message.contains("Operator name is required"));
    }

    #[test]
    fn final_operator_name_missing_print_report_returns_clear_error() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");

        let error = save_final_operator_name(unit_folder.display().to_string(), "Sean".to_string())
            .expect_err("missing print report should fail");

        assert_eq!(error.code, "print_report_missing");
        assert!(error.message.contains("print report workbook"));
    }

    #[test]
    fn final_operator_name_locked_workbook_maps_to_clear_error() {
        let locked = AutomationCommandError::from_print_report_error(ReportError::Io(
            io::Error::from_raw_os_error(32),
        ));

        assert_eq!(locked.code, "workbook_locked");
        assert!(locked.message.contains("print report workbook is locked"));
    }

    #[test]
    fn final_operator_name_missing_sheet_maps_to_clear_error() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let main_workbook = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        write_minimal_main_workbook(&main_workbook);
        let workbook = unit_folder.join(reports::PRINT_TEMPLATE_NAME);
        write_minimal_print_workbook(&workbook, "Wrong Sheet");
        write_ready_unit_state(&unit_folder);

        let error = save_final_operator_name(unit_folder.display().to_string(), "Sean".to_string())
            .expect_err("missing final operator sheet should fail");

        assert_eq!(error.code, "report_sheet_missing");
        assert!(error.message.contains(reports::FINAL_OPERATOR_SHEET));
    }

    #[test]
    fn verification_failure_does_not_patch_workbook() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        fs::write(&report, b"ORIGINAL REPORT").expect("write sentinel report");
        write_system_accuracy_csv(
            &unit_folder.join("unit_STEP15_SYSTEM_ACCURACY_TEST_DATA_AVG.csv"),
            true,
        );

        let result = process_task(
            unit_folder.display().to_string(),
            "208v-system-100% Load".to_string(),
        )
        .expect("task should return result");

        assert_eq!(result.state, "fail", "{}", result.message);
        assert_eq!(fs::read(&report).expect("read report"), b"ORIGINAL REPORT");
    }

    #[test]
    fn print_validation_blocks_incomplete_tasks_and_final_operator_write() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let main_workbook = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        write_minimal_main_workbook(&main_workbook);
        write_minimal_print_workbook(
            &unit_folder.join(reports::PRINT_TEMPLATE_NAME),
            reports::FINAL_OPERATOR_SHEET,
        );

        let readiness = validate_ready_for_print(unit_folder.display().to_string())
            .expect("validation should return blockers");

        assert!(!readiness.ready);
        assert!(readiness
            .blocking_issues
            .iter()
            .any(|issue| issue.task_id.as_deref() == Some("208v-transformer")));

        let error = save_final_operator_name(unit_folder.display().to_string(), "Sean".to_string())
            .expect_err("incomplete task state should block final operator write");

        assert_eq!(error.code, "print_validation_failed");
    }

    #[test]
    fn scan_rejects_corrupt_unit_state_sidecar() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        fs::write(unit_state::state_path(&unit_folder), "{not valid json")
            .expect("write corrupt unit state");

        let error =
            scan_unit_folder(unit_folder.display().to_string()).expect_err("scan should fail");

        match error {
            AutomationError::UnitState(error) => {
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
                assert!(error.to_string().contains(unit_state::UNIT_STATE_FILE));
            }
            other => panic!("expected unit state error, got {other:?}"),
        }
    }

    #[test]
    fn print_validation_rejects_corrupt_unit_state_sidecar() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let main_workbook = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        write_minimal_main_workbook(&main_workbook);
        write_minimal_print_workbook(
            &unit_folder.join(reports::PRINT_TEMPLATE_NAME),
            reports::FINAL_OPERATOR_SHEET,
        );
        fs::write(unit_state::state_path(&unit_folder), "{not valid json")
            .expect("write corrupt unit state");

        let error = validate_ready_for_print(unit_folder.display().to_string())
            .expect_err("print readiness should fail");

        assert_eq!(error.code, "unit_state_corrupt");
        assert!(error.message.contains("unit_state.json"));
        assert!(error
            .details
            .as_deref()
            .is_some_and(|details| details.contains(unit_state::UNIT_STATE_FILE)));
    }

    #[test]
    fn process_task_rejects_corrupt_unit_state_before_processing() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        fs::write(unit_state::state_path(&unit_folder), "{not valid json")
            .expect("write corrupt unit state");

        let error = process_task(
            unit_folder.display().to_string(),
            "208v-transformer".to_string(),
        )
        .expect_err("processing should fail before using corrupt state");

        match error {
            AutomationError::UnitState(error) => {
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
                assert!(error.to_string().contains(unit_state::UNIT_STATE_FILE));
            }
            other => panic!("expected unit state error, got {other:?}"),
        }
    }

    #[test]
    fn scan_restores_task_state_from_unit_state_sidecar() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let mut state = unit_state::UnitState::default();
        state.tasks.insert(
            "208v-transformer".to_string(),
            unit_state::UnitTaskState {
                task_id: "208v-transformer".to_string(),
                state: "pass".to_string(),
                code: Some(0),
                source_csv_path: None,
                csv_fingerprint: Some("test-fingerprint".to_string()),
                processed_at: Some(unit_state::now_string()),
                result: Some("processed before restart".to_string()),
                accepted: unit_state::TaskAcceptance::default(),
                audit_log: Vec::new(),
            },
        );
        unit_state::save_unit_state(&unit_folder, &state).expect("write state");

        let summary = scan_unit_folder(unit_folder.display().to_string()).expect("scan");
        let transformer = summary
            .tasks
            .iter()
            .find(|task| task.task_id == "208v-transformer")
            .expect("transformer task");

        assert_eq!(transformer.state, "pass");
        assert_eq!(
            transformer.csv_fingerprint.as_deref(),
            Some("test-fingerprint")
        );
    }

    #[test]
    fn same_fingerprint_pass_short_circuits_without_workbook_patch() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let csv_path = unit_folder.join("unit_STEP15_SYSTEM_ACCURACY_TEST_DATA_AVG.csv");
        write_system_accuracy_csv(&csv_path, false);
        let fingerprint = csv_data::csv_fingerprint(&csv_path).expect("fingerprint");
        let mut state = unit_state::UnitState::default();
        state.tasks.insert(
            "208v-system-100% Load".to_string(),
            unit_state::UnitTaskState {
                task_id: "208v-system-100% Load".to_string(),
                state: "pass".to_string(),
                code: Some(0),
                source_csv_path: Some(csv_path.display().to_string()),
                csv_fingerprint: Some(fingerprint),
                processed_at: Some(unit_state::now_string()),
                result: Some("already processed".to_string()),
                accepted: unit_state::TaskAcceptance::default(),
                audit_log: Vec::new(),
            },
        );
        unit_state::save_unit_state(&unit_folder, &state).expect("write state");

        let result = process_task(
            unit_folder.display().to_string(),
            "208v-system-100% Load".to_string(),
        )
        .expect("task should short-circuit");

        assert_eq!(result.state, "pass");
        assert!(result
            .message
            .contains("already processed from the same CSV"));
        assert_eq!(result.report_path, None);
    }

    #[test]
    #[ignore = "requires a real PDU sample unit folder; set PDU_SAMPLE_UNIT_FOLDER"]
    fn real_sample_processes_representative_tasks_without_touching_source() {
        let sample = std::env::var("PDU_SAMPLE_UNIT_FOLDER")
            .unwrap_or_else(|_| "C:/PDU500/262343000072".to_string());
        let sample = Path::new(&sample);

        assert!(
            sample.is_dir(),
            "sample folder missing: {}",
            sample.display()
        );

        let temp = TempDir::new().expect("temp dir");
        let unit_copy = temp.path().join("unit");
        copy_dir(sample, &unit_copy);

        let summary =
            setup_unit_folder(unit_copy.display().to_string()).expect("setup should work");

        assert_eq!(summary.serial_number.as_deref(), Some("262343000072"));
        assert!(summary.report_path.is_some());
        assert!(summary.detected_count >= 60);

        for task_id in [
            "208v-transformer",
            "208v-system-100% Load",
            "208v-breaker-1-100% Load",
            "415v-system-100% Load",
            "415v-breaker-1-100% Load",
            "system-burn-in",
            "breaker-burn-in-1",
        ] {
            let result =
                process_task(unit_copy.display().to_string(), task_id.to_string()).unwrap();

            assert_ne!(result.code, 2, "{task_id}: {}", result.message);
        }

        let report_path = summary.report_path.expect("main report path");
        assert_workbook_package_is_valid_after_patch(Path::new(&report_path));
    }

    fn write_minimal_print_workbook(path: &Path, sheet_name: &str) {
        let file = File::create(path).expect("create workbook");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", options)
            .expect("content types");
        zip.write_all(
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        )
        .expect("write content types");

        zip.start_file("xl/workbook.xml", options)
            .expect("workbook xml");
        zip.write_all(
            format!(
                r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="{sheet_name}" sheetId="1" r:id="rId1"/></sheets></workbook>"#
            )
            .as_bytes(),
        )
        .expect("write workbook");

        zip.start_file("xl/_rels/workbook.xml.rels", options)
            .expect("workbook rels");
        zip.write_all(
            br#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        )
        .expect("write rels");

        zip.start_file("xl/worksheets/sheet1.xml", options)
            .expect("sheet xml");
        zip.write_all(
            br#"<worksheet><sheetData><row r="39"><c r="A39"><v>1</v></c></row></sheetData></worksheet>"#,
        )
        .expect("write sheet");

        zip.finish().expect("finish workbook");
    }

    fn write_minimal_main_workbook(path: &Path) {
        let file = File::create(path).expect("create workbook");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", options)
            .expect("content types");
        zip.write_all(
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        )
        .expect("write content types");

        zip.start_file("xl/workbook.xml", options)
            .expect("workbook xml");
        zip.write_all(
            br#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Test Summary" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        )
        .expect("write workbook");

        zip.start_file("xl/_rels/workbook.xml.rels", options)
            .expect("workbook rels");
        zip.write_all(
            br#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        )
        .expect("write rels");

        zip.start_file("xl/worksheets/sheet1.xml", options)
            .expect("sheet xml");
        zip.write_all(
            br#"<worksheet><sheetData><row r="1"><c r="D1" t="inlineStr"><is><t>TX-READY</t></is></c></row></sheetData></worksheet>"#,
        )
        .expect("write sheet");

        zip.finish().expect("finish workbook");
    }

    fn write_ready_unit_state(unit_folder: &Path) {
        let mut state = unit_state::UnitState::default();

        for task in automation_tasks() {
            state.tasks.insert(
                task.id.clone(),
                unit_state::UnitTaskState {
                    task_id: task.id,
                    state: "pass".to_string(),
                    code: Some(0),
                    source_csv_path: None,
                    csv_fingerprint: None,
                    processed_at: Some(unit_state::now_string()),
                    result: Some("test ready state".to_string()),
                    accepted: unit_state::TaskAcceptance::default(),
                    audit_log: Vec::new(),
                },
            );
        }

        unit_state::save_unit_state(unit_folder, &state).expect("write ready unit state");
    }

    fn write_system_accuracy_csv(path: &Path, force_failure: bool) {
        let column_count = csv_data::excel_col_to_index("EO").expect("EO index") + 1;
        let mut headers = Vec::new();
        let mut row = vec!["1".to_string(); column_count];

        for index in 0..column_count {
            headers.push(format!("col{index}"));
        }

        if force_failure {
            let detect_voltage_a = csv_data::excel_col_to_index("CH").expect("CH index");
            row[detect_voltage_a] = "2".to_string();
        }

        fs::write(path, format!("{}\n{}\n", headers.join(","), row.join(","))).expect("write csv");
    }

    fn copy_dir(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).expect("create destination");

        for entry in WalkDir::new(source).into_iter().filter_map(Result::ok) {
            let relative = entry
                .path()
                .strip_prefix(source)
                .expect("entry should be under source");
            let target = destination.join(relative);

            if entry.file_type().is_dir() {
                fs::create_dir_all(&target).expect("create directory");
            } else {
                fs::copy(entry.path(), &target).expect("copy file");
            }
        }
    }

    fn assert_workbook_package_is_valid_after_patch(path: &Path) {
        let mut archive = ZipArchive::new(File::open(path).expect("open workbook")).unwrap();
        let mut workbook_xml = String::new();

        assert!(
            archive.by_name("xl/calcChain.xml").is_err(),
            "stale calcChain should be removed"
        );

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("zip entry");
            let name = entry.name().to_string();

            if !(name.ends_with(".xml") || name.ends_with(".rels")) {
                continue;
            }

            let mut xml = String::new();
            entry.read_to_string(&mut xml).expect("XML should be UTF-8");
            roxmltree::Document::parse(&xml)
                .unwrap_or_else(|error| panic!("{name} should be valid XML: {error}"));

            if name == "xl/workbook.xml" {
                workbook_xml = xml;
            }
        }

        assert!(workbook_xml.contains(r#"calcMode="auto""#));
        assert!(workbook_xml.contains(r#"fullCalcOnLoad="1""#));
        assert!(workbook_xml.contains(r#"forceFullCalc="1""#));
    }
}
