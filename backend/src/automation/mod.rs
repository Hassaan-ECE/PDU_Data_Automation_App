mod csv_data;
mod mapped;
mod processors;
mod reports;
pub mod tasks;
mod unit_candidates;
pub(crate) mod unit_state;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
use std::process::Stdio;
#[cfg(target_os = "windows")]
use std::thread;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Serialize;
use thiserror::Error;

use crate::config::{load_layout_profile, LayoutProfileError, ReportLayoutProfile, TaskDefinition};

use self::csv_data::{csv_fingerprint, csv_metadata_matches_fingerprint};
use self::processors::{
    FailureDetail, ProcessorResult, ProcessorTaskOutput, ACCURACY_CHECK_FAILURE_TITLE,
};
use self::reports::{
    inspect_reports_with_config, patch_workbooks_transactional,
    read_transformer_serial_number_with_config, require_print_report_with_config,
    setup_reports_with_serial_number_template_root_and_config,
    setup_reports_with_template_root_and_config, write_final_operator_name_with_config,
    write_transformer_serial_number_with_config, ReportError, ReportFileConfig, ReportSetup,
    WorkbookPatch,
};
use self::tasks::{automation_tasks, find_task};
pub(crate) use self::unit_candidates::resolve_unit_serial_number;
pub use self::unit_candidates::{LatestUnitCandidateResult, UnitCandidate};
use self::unit_state::TaskStateSeed;

#[derive(Debug, Error)]
pub enum AutomationError {
    #[error("{0}")]
    Report(#[from] ReportError),
    #[error("unit state could not be read or written: {0}")]
    UnitState(#[from] io::Error),
    #[error("{0}")]
    LayoutProfile(#[from] LayoutProfileError),
    #[error("unknown automation task id: {0}")]
    UnknownTask(String),
    #[error("{0}")]
    TaskAcceptance(String),
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
                "workbook_busy",
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
                "workbook_busy",
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

    fn from_layout_profile_error(error: LayoutProfileError) -> Self {
        let code = match &error {
            LayoutProfileError::ReadFailed { .. } => "layout_profile_read_failed",
            LayoutProfileError::InvalidJson { .. } => "layout_profile_invalid_json",
            LayoutProfileError::InvalidProfile { .. } => "layout_profile_invalid",
        };

        Self::report(
            code,
            "The report layout profile could not be loaded. Fix the configured layout profile before continuing.",
            error.to_string(),
        )
    }

    pub(crate) fn from_automation_error(error: AutomationError) -> Self {
        match error {
            AutomationError::Report(error) => Self::from_report_error(error),
            AutomationError::UnitState(error) => Self::from_unit_state_error(error),
            AutomationError::LayoutProfile(error) => Self::from_layout_profile_error(error),
            AutomationError::UnknownTask(task_id) => Self::validation(
                "unknown_task",
                format!("Unknown automation task id: {task_id}"),
            ),
            AutomationError::TaskAcceptance(message) => {
                Self::validation("task_not_acceptable", message)
            }
            AutomationError::OpenReport(message) => Self::report(
                "automation_failed",
                "The requested automation action could not be completed.",
                message,
            ),
        }
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
    pub process_ready: bool,
    pub wait_phase: TaskWaitPhase,
    pub phase_deadline_ms: Option<u64>,
    pub pending_duration_seconds: u64,
    pub nominal_duration_seconds: u64,
    pub match_reason: String,
    pub source_csv_path: Option<String>,
    pub csv_fingerprint: Option<String>,
    pub processed_at: Option<String>,
    pub result: Option<String>,
    pub accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskWaitPhase {
    AwaitingCsv,
    Timing,
    Soaking,
    WaitingStep72,
    Capturing,
    WaitingUnlock,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TaskReadiness {
    process_ready: bool,
    wait_phase: TaskWaitPhase,
    phase_deadline_ms: Option<u64>,
    pending_duration_seconds: u64,
    nominal_duration_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskProcessResult {
    pub task_id: String,
    pub state: String,
    pub code: u8,
    pub continue_sequence: bool,
    pub message: String,
    pub log: Vec<String>,
    pub report_path: Option<String>,
    pub print_report_path: Option<String>,
    pub failure: Option<FailureDetail>,
    pub source_csv_path: Option<String>,
    pub csv_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskBatchProcessResult {
    pub results: Vec<TaskProcessResult>,
    pub committed: bool,
    pub committed_count: usize,
    pub stopped_task_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskBatchProgress {
    pub unit_folder: String,
    pub task_id: String,
    pub state: String,
    pub message: String,
    pub index: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloseReportWorkbookResult {
    pub closed: bool,
    pub path: String,
    pub message: String,
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
    let profile = load_layout_profile()?;
    let template_root = profile_template_root(&profile);
    let report_config = report_file_config(&profile);
    let report_setup =
        setup_reports_with_template_root_and_config(&unit_folder, &template_root, &report_config)?;

    build_summary_with_profile(&unit_folder, report_setup, &profile)
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
    let profile =
        load_layout_profile().map_err(AutomationCommandError::from_layout_profile_error)?;
    let template_root = profile_template_root(&profile);
    let report_config = report_file_config(&profile);
    let report_setup = setup_reports_with_serial_number_template_root_and_config(
        &unit_folder,
        unit_serial_number,
        &template_root,
        &report_config,
    )
    .map_err(AutomationCommandError::from_report_error)?;

    write_transformer_serial_number_with_config(&unit_folder, transformer_sn, &report_config)
        .map_err(AutomationCommandError::from_report_error)?;

    build_summary_with_profile(&unit_folder, report_setup, &profile)
        .map_err(AutomationCommandError::from_automation_error)
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

    let profile =
        load_layout_profile().map_err(AutomationCommandError::from_layout_profile_error)?;
    let report_config = report_file_config(&profile);

    write_transformer_serial_number_with_config(&unit_folder, transformer_sn, &report_config)
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

    let profile =
        load_layout_profile().map_err(AutomationCommandError::from_layout_profile_error)?;
    let report_config = report_file_config(&profile);
    let print_report_path =
        write_final_operator_name_with_config(&unit_folder, operator_name, &report_config)
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

    let profile =
        load_layout_profile().map_err(AutomationCommandError::from_layout_profile_error)?;
    let report_config = report_file_config(&profile);
    let print_report_path = require_print_report_with_config(&unit_folder, &report_config)
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

    let profile =
        load_layout_profile().map_err(AutomationCommandError::from_layout_profile_error)?;
    let report_config = report_file_config(&profile);
    require_print_report_with_config(unit_folder, &report_config)
        .map_err(AutomationCommandError::from_print_report_error)?;

    if read_transformer_serial_number_with_config(unit_folder, &report_config)
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
    let profile = load_layout_profile()?;
    let report_config = report_file_config(&profile);
    let report_setup = inspect_reports_with_config(&unit_folder, &report_config)?;

    build_summary_with_profile(&unit_folder, report_setup, &profile)
}

pub fn accept_task_failure(
    unit_folder: String,
    task_id: String,
) -> Result<UnitFolderSummary, AutomationError> {
    find_task(&task_id).ok_or_else(|| AutomationError::UnknownTask(task_id.clone()))?;
    let unit_folder_path = PathBuf::from(&unit_folder);
    unit_state::accept_task_failure(
        &unit_folder_path,
        &task_id,
        "operator",
        "Operator marked a negligible failed result as pass.",
    )
    .map_err(|error| match error.kind() {
        io::ErrorKind::InvalidInput | io::ErrorKind::NotFound => {
            AutomationError::TaskAcceptance(error.to_string())
        }
        _ => AutomationError::UnitState(error),
    })?;

    scan_unit_folder(unit_folder)
}

pub fn process_task(
    unit_folder: String,
    task_id: String,
) -> Result<TaskProcessResult, AutomationError> {
    process_task_at(unit_folder, task_id, current_time_millis())
}

/// Deterministic-clock entry point used by integration tests.
///
/// Production Tauri commands call [`process_task`], which always supplies the
/// current system clock and cannot bypass readiness.
#[doc(hidden)]
pub fn process_task_at(
    unit_folder: String,
    task_id: String,
    now_ms: u64,
) -> Result<TaskProcessResult, AutomationError> {
    let unit_folder = PathBuf::from(unit_folder);
    let task = find_task(&task_id).ok_or_else(|| AutomationError::UnknownTask(task_id.clone()))?;
    let state = unit_state::load_or_default(&unit_folder)?;
    if let Some(result) =
        already_processed_result_from_state(&unit_folder, &task_id, &task.label, &state)
    {
        return Ok(to_task_process_result(task_id, result));
    }

    let profile = load_layout_profile()?;
    if let Some(waiting) = process_preflight(&task, &profile, &unit_folder, now_ms)? {
        return Ok(waiting);
    }
    let report_config = report_file_config(&profile);
    let already_processed_fingerprint = state
        .tasks
        .get(&task_id)
        .and_then(|entry| entry.already_processed_fingerprint())
        .map(ToOwned::to_owned);
    let result = match process_task_with_profile_mapping(
        &profile,
        &task_id,
        &unit_folder,
        already_processed_fingerprint.as_deref(),
    )? {
        Some(result) => result,
        None => processors::process_task(
            &task,
            &unit_folder,
            already_processed_fingerprint.as_deref(),
            &report_config,
        )?,
    };

    unit_state::record_processor_result(&unit_folder, &task_id, &result)?;

    Ok(to_task_process_result(task_id, result))
}

pub fn process_tasks(
    unit_folder: String,
    task_ids: Vec<String>,
) -> Result<TaskBatchProcessResult, AutomationError> {
    process_tasks_at(unit_folder, task_ids, current_time_millis())
}

fn process_tasks_at(
    unit_folder: String,
    task_ids: Vec<String>,
    now_ms: u64,
) -> Result<TaskBatchProcessResult, AutomationError> {
    process_tasks_with_progress_at(unit_folder, task_ids, now_ms, |_| {})
}

pub fn process_tasks_with_progress<F>(
    unit_folder: String,
    task_ids: Vec<String>,
    on_progress: F,
) -> Result<TaskBatchProcessResult, AutomationError>
where
    F: FnMut(TaskBatchProgress),
{
    process_tasks_with_progress_at(unit_folder, task_ids, current_time_millis(), on_progress)
}

fn process_tasks_with_progress_at<F>(
    unit_folder: String,
    task_ids: Vec<String>,
    now_ms: u64,
    mut on_progress: F,
) -> Result<TaskBatchProcessResult, AutomationError>
where
    F: FnMut(TaskBatchProgress),
{
    let unit_folder = PathBuf::from(unit_folder);
    let unit_folder_display = unit_folder.display().to_string();
    let total = task_ids.len();
    let profile = load_layout_profile()?;
    let report_config = report_file_config(&profile);
    let state = unit_state::load_or_default(&unit_folder)?;
    let mut results = Vec::<TaskProcessResult>::new();
    let mut pending = Vec::<PendingTaskOutput>::new();
    let mut stopped_task_id = None;
    let mut committed = true;

    for (task_position, task_id) in task_ids.into_iter().enumerate() {
        let task =
            find_task(&task_id).ok_or_else(|| AutomationError::UnknownTask(task_id.clone()))?;

        on_progress(TaskBatchProgress {
            unit_folder: unit_folder_display.clone(),
            task_id: task_id.clone(),
            state: "processing".to_string(),
            message: format!("Processing {}", task.label),
            index: task_position + 1,
            total,
        });

        let output = if let Some(result) =
            already_processed_result_from_state(&unit_folder, &task_id, &task.label, &state)
        {
            ProcessorTaskOutput::result_only(result)
        } else {
            if let Some(waiting) = process_preflight(&task, &profile, &unit_folder, now_ms)? {
                if let Some(stopped) = flush_pending_task_outputs(
                    &unit_folder,
                    &mut pending,
                    &mut results,
                    &mut on_progress,
                    &unit_folder_display,
                    total,
                )? {
                    stopped_task_id = Some(stopped);
                    committed = false;
                    break;
                }

                stopped_task_id = Some(task_id.clone());
                results.push(waiting.clone());
                on_progress(TaskBatchProgress {
                    unit_folder: unit_folder_display.clone(),
                    task_id: waiting.task_id,
                    state: waiting.state,
                    message: waiting.message,
                    index: results.len(),
                    total,
                });
                break;
            }

            let already_processed_fingerprint = state
                .tasks
                .get(&task_id)
                .and_then(|entry| entry.already_processed_fingerprint())
                .map(ToOwned::to_owned);

            compute_task_output(
                &profile,
                &task,
                &task_id,
                &unit_folder,
                already_processed_fingerprint.as_deref(),
                &report_config,
            )?
        };

        let continue_sequence = processor_result_continues_sequence(&output.result);

        if output.result.state == "pass" {
            pending.push(PendingTaskOutput {
                task_id,
                result: output.result,
                patches: output.patches,
            });
            continue;
        }

        if !output.patches.is_empty() {
            let stopped_after_commit = task_id.clone();
            pending.push(PendingTaskOutput {
                task_id,
                result: output.result,
                patches: output.patches,
            });

            if continue_sequence {
                continue;
            }

            if let Some(stopped) = flush_pending_task_outputs(
                &unit_folder,
                &mut pending,
                &mut results,
                &mut on_progress,
                &unit_folder_display,
                total,
            )? {
                stopped_task_id = Some(stopped);
                committed = false;
                break;
            }

            stopped_task_id = Some(stopped_after_commit);
            break;
        }

        if let Some(stopped) = flush_pending_task_outputs(
            &unit_folder,
            &mut pending,
            &mut results,
            &mut on_progress,
            &unit_folder_display,
            total,
        )? {
            stopped_task_id = Some(stopped);
            committed = false;
            break;
        }

        unit_state::record_processor_result(&unit_folder, &task_id, &output.result)?;
        stopped_task_id = Some(task_id.clone());
        let result = to_task_process_result(task_id, output.result);
        results.push(result.clone());
        on_progress(TaskBatchProgress {
            unit_folder: unit_folder_display.clone(),
            task_id: result.task_id,
            state: result.state,
            message: result.message,
            index: results.len(),
            total,
        });
        break;
    }

    if stopped_task_id.is_none() {
        if let Some(stopped) = flush_pending_task_outputs(
            &unit_folder,
            &mut pending,
            &mut results,
            &mut on_progress,
            &unit_folder_display,
            total,
        )? {
            stopped_task_id = Some(stopped);
            committed = false;
        }
    }

    let committed_count = results
        .iter()
        .filter(|result| result.state == "pass")
        .count();
    let failed_count = results
        .iter()
        .filter(|result| result.state == "fail")
        .count();
    let processed_count = results.len();
    let message = if let Some(task_id) = &stopped_task_id {
        format!(
            "Batch stopped at {task_id} after finalizing {committed_count} task{}.",
            if committed_count == 1 { "" } else { "s" }
        )
    } else if failed_count > 0 {
        format!(
            "Batch processed {processed_count} tasks ({committed_count} passed, {failed_count} failed)."
        )
    } else {
        format!(
            "Batch processed {committed_count} task{}.",
            if committed_count == 1 { "" } else { "s" }
        )
    };

    Ok(TaskBatchProcessResult {
        results,
        committed,
        committed_count,
        stopped_task_id,
        message,
    })
}

struct PendingTaskOutput {
    task_id: String,
    result: ProcessorResult,
    patches: Vec<WorkbookPatch>,
}

fn flush_pending_task_outputs(
    unit_folder: &Path,
    pending: &mut Vec<PendingTaskOutput>,
    results: &mut Vec<TaskProcessResult>,
    on_progress: &mut impl FnMut(TaskBatchProgress),
    unit_folder_display: &str,
    total: usize,
) -> Result<Option<String>, AutomationError> {
    if pending.is_empty() {
        return Ok(None);
    }

    let patches = pending
        .iter()
        .flat_map(|output| output.patches.iter().cloned())
        .collect::<Vec<_>>();

    if let Err(error) = patch_workbooks_transactional(&patches) {
        if is_external_workbook_lock_error(&error) {
            return Err(AutomationError::Report(error));
        }

        let failed_index = pending
            .iter()
            .position(|output| !output.patches.is_empty())
            .unwrap_or(0);
        let prior_records = pending
            .iter()
            .take(failed_index)
            .map(|output| (output.task_id.clone(), output.result.clone()))
            .collect::<Vec<_>>();

        if !prior_records.is_empty() {
            unit_state::record_processor_results(unit_folder, &prior_records)?;

            for output in pending.iter().take(failed_index) {
                let result = to_task_process_result(output.task_id.clone(), output.result.clone());
                results.push(result.clone());
                on_progress(TaskBatchProgress {
                    unit_folder: unit_folder_display.to_string(),
                    task_id: result.task_id,
                    state: result.state,
                    message: result.message,
                    index: results.len(),
                    total,
                });
            }
        }

        let failed = pending
            .get(failed_index)
            .expect("pending should contain at least one output");
        let failed_task_id = failed.task_id.clone();
        let result = report_commit_failure_result(&failed.result, error);

        unit_state::record_processor_result(unit_folder, &failed_task_id, &result)?;
        let result = to_task_process_result(failed_task_id.clone(), result);
        results.push(result.clone());
        on_progress(TaskBatchProgress {
            unit_folder: unit_folder_display.to_string(),
            task_id: result.task_id,
            state: result.state,
            message: result.message,
            index: results.len(),
            total,
        });
        pending.clear();

        return Ok(Some(failed_task_id));
    }

    let records = pending
        .iter()
        .map(|output| (output.task_id.clone(), output.result.clone()))
        .collect::<Vec<_>>();

    unit_state::record_processor_results(unit_folder, &records)?;

    for output in pending.drain(..) {
        let result = to_task_process_result(output.task_id, output.result);
        results.push(result.clone());
        on_progress(TaskBatchProgress {
            unit_folder: unit_folder_display.to_string(),
            task_id: result.task_id,
            state: result.state,
            message: result.message,
            index: results.len(),
            total,
        });
    }

    Ok(None)
}

fn report_commit_failure_result(
    attempted: &ProcessorResult,
    error: ReportError,
) -> ProcessorResult {
    let message = format!("Report commit failed: {error}");
    let report_path = attempted.report_path.clone();
    let print_report_path = attempted.print_report_path.clone();

    ProcessorResult {
        state: "fail".to_string(),
        code: 1,
        message: message.clone(),
        log: vec![format!("[commit] {message}")],
        report_path,
        print_report_path,
        failure: Some(FailureDetail {
            title: "Report Commit Failed".to_string(),
            message,
            location: None,
        }),
        source_csv_path: attempted.source_csv_path.clone(),
        csv_fingerprint: attempted.csv_fingerprint.clone(),
    }
}

fn already_processed_result_from_state(
    unit_folder: &Path,
    task_id: &str,
    task_label: &str,
    state: &unit_state::UnitState,
) -> Option<ProcessorResult> {
    let entry = state.tasks.get(task_id)?;
    let expected_fingerprint = entry.already_processed_fingerprint()?;
    let source_path = entry.source_csv_path.as_deref()?;
    let source_path = stored_source_csv_path(unit_folder, source_path)?;
    let current_fingerprint =
        if csv_metadata_matches_fingerprint(&source_path, expected_fingerprint).ok()? {
            expected_fingerprint.to_string()
        } else {
            csv_fingerprint(&source_path).ok()?
        };

    if current_fingerprint != expected_fingerprint {
        return None;
    }

    Some(ProcessorResult {
        state: "pass".to_string(),
        code: 0,
        message: format!(
            "{task_label} already processed from the same CSV; no workbook changes were applied"
        ),
        log: vec![format!(
            "[idempotent] {task_label} already processed from {} ({current_fingerprint})",
            source_path.display()
        )],
        report_path: None,
        print_report_path: None,
        failure: None,
        source_csv_path: Some(source_path.display().to_string()),
        csv_fingerprint: Some(current_fingerprint),
    })
}

fn stored_source_csv_path(unit_folder: &Path, source_csv_path: &str) -> Option<PathBuf> {
    let source_path = PathBuf::from(source_csv_path);
    let source_path = if source_path.is_absolute() {
        source_path
    } else {
        unit_folder.join(source_path)
    };

    if !source_path.is_file() {
        return None;
    }

    let unit_folder = fs::canonicalize(unit_folder).ok()?;
    let source_path = fs::canonicalize(source_path).ok()?;

    source_path.starts_with(unit_folder).then_some(source_path)
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

pub fn close_report_workbook(
    unit_folder: String,
    path: String,
) -> Result<CloseReportWorkbookResult, AutomationCommandError> {
    let report_path = if path.trim().is_empty() {
        let unit_folder_path = PathBuf::from(&unit_folder);
        let profile =
            load_layout_profile().map_err(AutomationCommandError::from_layout_profile_error)?;
        let report_config = report_file_config(&profile);
        let discovered_path =
            self::reports::require_main_report_with_config(&unit_folder_path, &report_config)
                .map_err(AutomationCommandError::from_report_error)?;

        validate_report_path(&unit_folder, &discovered_path.display().to_string())
            .map_err(AutomationCommandError::from_automation_error)?
    } else {
        validate_report_path(&unit_folder, &path)
            .map_err(AutomationCommandError::from_automation_error)?
    };
    let closed = close_excel_workbook(&report_path).map_err(|error| {
        AutomationCommandError::report(
            "workbook_close_failed",
            "The report workbook could not be closed automatically.",
            error.to_string(),
        )
    })?;

    Ok(CloseReportWorkbookResult {
        closed,
        path: report_path.display().to_string(),
        message: if closed {
            "The report workbook was saved and closed.".to_string()
        } else {
            "The report workbook was already closed.".to_string()
        },
    })
}

fn build_summary_with_profile(
    unit_folder: &Path,
    report_setup: ReportSetup,
    profile: &ReportLayoutProfile,
) -> Result<UnitFolderSummary, AutomationError> {
    let mut detected_task_ids = HashSet::<String>::new();
    let now_ms = system_time_millis(SystemTime::now()).unwrap_or_default();
    let csv_index = CsvScanIndex::scan(unit_folder);
    let automation_tasks = automation_tasks();
    let csv_matches = automation_tasks
        .iter()
        .map(|task| (task.id.clone(), task_csv_match(task, profile, &csv_index)))
        .collect::<HashMap<_, _>>();
    let seeds = automation_tasks
        .iter()
        .map(|task| {
            let detected_for_task = csv_index.detected_steps_for_task(&task.detection_steps);
            let csv_match = csv_matches
                .get(&task.id)
                .expect("csv match should exist for task");

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
            let detected_for_task = csv_index.detected_steps_for_task(&task.detection_steps);
            let latest_csv_info = csv_index.latest_for_steps(&task.detection_steps);
            let latest_csv = latest_csv_info
                .as_ref()
                .map(|info| info.path.display().to_string());
            let latest_csv_created_ms = latest_csv_info.as_ref().and_then(|info| info.created_ms);
            let latest_csv_readable = latest_csv_info.as_ref().map(|info| info.readable);
            let timer_start_ms = csv_index
                .timer_start_millis_for_steps(&task.detection_steps)
                .or(latest_csv_created_ms);
            let detected_state = if detected_for_task.is_empty() {
                "off"
            } else {
                detected_task_ids.insert(task.id.clone());
                "detected"
            };
            let csv_match = csv_matches
                .get(&task.id)
                .expect("csv match should exist for task");
            let readiness = evaluate_task_readiness(&task, &csv_index, csv_match, now_ms);
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
                process_ready: readiness.process_ready,
                wait_phase: readiness.wait_phase,
                phase_deadline_ms: readiness.phase_deadline_ms,
                pending_duration_seconds: readiness.pending_duration_seconds,
                nominal_duration_seconds: readiness.nominal_duration_seconds,
                match_reason: csv_match.reason.clone(),
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

fn profile_template_root(profile: &ReportLayoutProfile) -> PathBuf {
    PathBuf::from(&profile.templates.default_template_root)
}

fn report_file_config(profile: &ReportLayoutProfile) -> ReportFileConfig {
    let defaults = ReportFileConfig::legacy_defaults();
    let main_report_pattern = profile
        .workbooks
        .get("main")
        .map(|workbook| workbook.file_pattern.clone())
        .unwrap_or(defaults.main_report_pattern);
    let print_report_pattern = profile
        .workbooks
        .get("print")
        .map(|workbook| workbook.file_pattern.clone())
        .unwrap_or(defaults.print_report_pattern);

    ReportFileConfig::new(
        profile.templates.main_report_template.clone(),
        profile.templates.print_report_template.clone(),
        main_report_pattern,
        print_report_pattern,
    )
}

#[derive(Debug, Clone)]
struct TaskCsvMatch {
    path: Option<PathBuf>,
    created_ms: Option<u64>,
    processable: bool,
    reason: String,
}

const TRANSFORMER_DURATION_SECONDS: u64 = 60;
const DEFAULT_TASK_DURATION_SECONDS: u64 = 180;
const SYSTEM_BURN_IN_SOAK_SECONDS: u64 = 7_200;
const SYSTEM_BURN_IN_CAPTURE_SECONDS: u64 = 60;

fn duration_ms(seconds: u64) -> u64 {
    seconds.saturating_mul(1_000)
}

fn task_nominal_duration_seconds(task: &tasks::AutomationTask) -> u64 {
    match task.kind {
        tasks::TaskKind::Transformer { .. } => TRANSFORMER_DURATION_SECONDS,
        tasks::TaskKind::SystemBurnIn => {
            SYSTEM_BURN_IN_SOAK_SECONDS + SYSTEM_BURN_IN_CAPTURE_SECONDS
        }
        _ => DEFAULT_TASK_DURATION_SECONDS,
    }
}

fn generic_readiness(
    task: &tasks::AutomationTask,
    csv_match: &TaskCsvMatch,
    now_ms: u64,
) -> TaskReadiness {
    let nominal_duration_seconds = task_nominal_duration_seconds(task);
    let Some(start_ms) = csv_match.created_ms else {
        return TaskReadiness {
            process_ready: false,
            wait_phase: TaskWaitPhase::AwaitingCsv,
            phase_deadline_ms: None,
            pending_duration_seconds: nominal_duration_seconds,
            nominal_duration_seconds,
        };
    };
    let phase_deadline_ms = start_ms.saturating_add(duration_ms(nominal_duration_seconds));
    let (process_ready, wait_phase) = if now_ms < phase_deadline_ms {
        (false, TaskWaitPhase::Timing)
    } else if !csv_match.processable {
        (false, TaskWaitPhase::WaitingUnlock)
    } else {
        (true, TaskWaitPhase::Ready)
    };

    TaskReadiness {
        process_ready,
        wait_phase,
        phase_deadline_ms: Some(phase_deadline_ms),
        pending_duration_seconds: 0,
        nominal_duration_seconds,
    }
}

fn burn_in_readiness(csv_index: &CsvScanIndex, now_ms: u64) -> TaskReadiness {
    let nominal_duration_seconds = SYSTEM_BURN_IN_SOAK_SECONDS + SYSTEM_BURN_IN_CAPTURE_SECONDS;
    let Some(step71) = csv_index.latest_for_step(71) else {
        return TaskReadiness {
            process_ready: false,
            wait_phase: TaskWaitPhase::AwaitingCsv,
            phase_deadline_ms: None,
            pending_duration_seconds: nominal_duration_seconds,
            nominal_duration_seconds,
        };
    };
    let Some(step71_start_ms) = step71.created_ms else {
        return TaskReadiness {
            process_ready: false,
            wait_phase: TaskWaitPhase::AwaitingCsv,
            phase_deadline_ms: None,
            pending_duration_seconds: nominal_duration_seconds,
            nominal_duration_seconds,
        };
    };
    let soak_deadline_ms = step71_start_ms.saturating_add(duration_ms(SYSTEM_BURN_IN_SOAK_SECONDS));
    let step72 =
        csv_index.latest_for_step_fragments(72, &["SYSTEM_ACCURACY_TEST_DATA_AVG".to_string()]);
    let Some(step72) = step72 else {
        return TaskReadiness {
            process_ready: false,
            wait_phase: if now_ms < soak_deadline_ms {
                TaskWaitPhase::Soaking
            } else {
                TaskWaitPhase::WaitingStep72
            },
            phase_deadline_ms: (now_ms < soak_deadline_ms).then_some(soak_deadline_ms),
            pending_duration_seconds: SYSTEM_BURN_IN_CAPTURE_SECONDS,
            nominal_duration_seconds,
        };
    };
    let Some(step72_start_ms) = step72.created_ms else {
        return TaskReadiness {
            process_ready: false,
            wait_phase: TaskWaitPhase::WaitingStep72,
            phase_deadline_ms: None,
            pending_duration_seconds: SYSTEM_BURN_IN_CAPTURE_SECONDS,
            nominal_duration_seconds,
        };
    };
    let capture_deadline_ms =
        step72_start_ms.saturating_add(duration_ms(SYSTEM_BURN_IN_CAPTURE_SECONDS));
    let phase_deadline_ms = soak_deadline_ms.max(capture_deadline_ms);
    let wait_phase = if now_ms < phase_deadline_ms {
        if soak_deadline_ms >= capture_deadline_ms {
            TaskWaitPhase::Soaking
        } else {
            TaskWaitPhase::Capturing
        }
    } else if !step72.readable {
        TaskWaitPhase::WaitingUnlock
    } else {
        TaskWaitPhase::Ready
    };

    TaskReadiness {
        process_ready: wait_phase == TaskWaitPhase::Ready,
        wait_phase,
        phase_deadline_ms: Some(phase_deadline_ms),
        pending_duration_seconds: 0,
        nominal_duration_seconds,
    }
}

fn evaluate_task_readiness(
    task: &tasks::AutomationTask,
    csv_index: &CsvScanIndex,
    csv_match: &TaskCsvMatch,
    now_ms: u64,
) -> TaskReadiness {
    if matches!(task.kind, tasks::TaskKind::SystemBurnIn) {
        burn_in_readiness(csv_index, now_ms)
    } else {
        generic_readiness(task, csv_match, now_ms)
    }
}

fn readiness_wait_message(task: &tasks::AutomationTask, readiness: TaskReadiness) -> String {
    match readiness.wait_phase {
        TaskWaitPhase::AwaitingCsv => format!("Waiting for {} CSV.", task.label),
        TaskWaitPhase::Timing => format!("Waiting for {} CSV timer.", task.label),
        TaskWaitPhase::Soaking => "System Burn-In STEP71 soak is still running.".to_string(),
        TaskWaitPhase::WaitingStep72 => {
            "STEP71 soak is complete; waiting for matching STEP72 burn-in data.".to_string()
        }
        TaskWaitPhase::Capturing => {
            "STEP72 burn-in capture is stabilizing for one minute.".to_string()
        }
        TaskWaitPhase::WaitingUnlock => "Required CSV is still locked or unreadable.".to_string(),
        TaskWaitPhase::Ready => "Ready.".to_string(),
    }
}

fn process_preflight(
    task: &tasks::AutomationTask,
    profile: &ReportLayoutProfile,
    unit_folder: &Path,
    now_ms: u64,
) -> Result<Option<TaskProcessResult>, AutomationError> {
    let csv_index = CsvScanIndex::scan(unit_folder);
    let csv_match = task_csv_match(task, profile, &csv_index);
    let readiness = evaluate_task_readiness(task, &csv_index, &csv_match, now_ms);

    if readiness.process_ready {
        return Ok(None);
    }

    Ok(Some(TaskProcessResult {
        task_id: task.id.clone(),
        state: "waiting".to_string(),
        code: 2,
        continue_sequence: false,
        message: readiness_wait_message(task, readiness),
        log: vec![csv_match.reason],
        report_path: None,
        print_report_path: None,
        failure: None,
        source_csv_path: csv_match.path.map(|path| path.display().to_string()),
        csv_fingerprint: None,
    }))
}

#[derive(Debug, Clone)]
struct CsvScanEntry {
    path: PathBuf,
    step: Option<u16>,
    file_name_upper: String,
    modified: SystemTime,
    created_ms: Option<u64>,
    readable: bool,
}

#[derive(Debug, Clone)]
struct CsvScanIndex {
    entries: Vec<CsvScanEntry>,
}

impl CsvScanIndex {
    fn scan(root: &Path) -> Self {
        let step_re = Regex::new(r"_STEP(\d+)_").expect("step regex is valid");
        let entries = walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter_map(|entry| {
                let path = entry.into_path();

                if !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
                {
                    return None;
                }

                let file_name = path.file_name()?.to_str()?;
                let file_name_upper = file_name.to_ascii_uppercase();
                let step = step_re
                    .captures(file_name)
                    .and_then(|captures| captures.get(1))
                    .and_then(|match_| match_.as_str().parse::<u16>().ok());
                let metadata = path.metadata().ok();
                let modified = metadata
                    .as_ref()
                    .and_then(|metadata| metadata.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                let created_ms = metadata.as_ref().and_then(csv_start_time_millis);
                let readable = csv_file_is_readable(&path);

                Some(CsvScanEntry {
                    path,
                    step,
                    file_name_upper,
                    modified,
                    created_ms,
                    readable,
                })
            })
            .collect();

        Self { entries }
    }

    fn detected_steps_for_task(&self, steps: &[u16]) -> Vec<u16> {
        steps
            .iter()
            .copied()
            .filter(|step| self.has_step(*step))
            .collect()
    }

    fn has_step(&self, step: u16) -> bool {
        self.entries.iter().any(|entry| entry.step == Some(step))
    }

    fn latest_for_steps(&self, steps: &[u16]) -> Option<&CsvScanEntry> {
        steps
            .iter()
            .filter_map(|step| self.latest_for_step(*step))
            .max_by_key(|entry| entry.modified)
    }

    fn timer_start_millis_for_steps(&self, steps: &[u16]) -> Option<u64> {
        steps.iter().find_map(|step| {
            self.latest_for_step(*step)
                .and_then(|entry| entry.created_ms)
        })
    }

    fn latest_for_step(&self, step: u16) -> Option<&CsvScanEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.step == Some(step))
            .max_by_key(|entry| entry.modified)
    }

    fn latest_for_step_fragments(
        &self,
        step: u16,
        required_fragments: &[String],
    ) -> Option<&CsvScanEntry> {
        let required_fragments = required_fragments
            .iter()
            .map(|fragment| fragment.to_ascii_uppercase())
            .collect::<Vec<_>>();

        self.entries
            .iter()
            .filter(|entry| entry.step == Some(step))
            .filter(|entry| {
                required_fragments
                    .iter()
                    .all(|fragment| entry.file_name_upper.contains(fragment))
            })
            .max_by_key(|entry| entry.modified)
    }

    fn latest_by_pattern(&self, pattern: &str) -> Option<&CsvScanEntry> {
        let pattern = wildcard_pattern_to_regex(pattern)?;

        self.entries
            .iter()
            .filter(|entry| {
                entry
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| pattern.is_match(name))
            })
            .max_by_key(|entry| entry.modified)
    }
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

fn task_csv_match(
    task: &tasks::AutomationTask,
    profile: &ReportLayoutProfile,
    csv_index: &CsvScanIndex,
) -> TaskCsvMatch {
    if let Some(mapped_csv) = mapped_csv_match(profile, csv_index, &task.id) {
        return TaskCsvMatch {
            reason: if mapped_csv.readable {
                format!(
                    "matched configured CSV pattern: {}",
                    mapped_csv.path.display()
                )
            } else {
                format!(
                    "configured CSV is not readable yet: {}",
                    mapped_csv.path.display()
                )
            },
            processable: mapped_csv.readable,
            created_ms: mapped_csv.created_ms,
            path: Some(mapped_csv.path.clone()),
        };
    }

    let Some((step, fragments)) = built_in_csv_requirement(task) else {
        return TaskCsvMatch {
            path: None,
            created_ms: None,
            processable: false,
            reason: "task has no CSV processor requirement".to_string(),
        };
    };

    match csv_index.latest_for_step_fragments(step, &fragments) {
        Some(csv) => TaskCsvMatch {
            reason: if csv.readable {
                format!("matched required STEP{step} CSV: {}", csv.path.display())
            } else {
                format!(
                    "required STEP{step} CSV is not readable yet: {}",
                    csv.path.display()
                )
            },
            processable: csv.readable,
            created_ms: csv.created_ms,
            path: Some(csv.path.clone()),
        },
        None => TaskCsvMatch {
            path: None,
            created_ms: None,
            processable: false,
            reason: format!(
                "no processable STEP{step} CSV found with required fragment(s): {}",
                fragments.join(", ")
            ),
        },
    }
}

fn mapped_csv_match(
    profile: &ReportLayoutProfile,
    csv_index: &CsvScanIndex,
    task_id: &str,
) -> Option<CsvScanEntry> {
    let task = profile_task(profile, task_id).filter(|task| !task.mappings.is_empty())?;

    csv_index.latest_by_pattern(&task.csv_pattern).cloned()
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

fn csv_file_is_readable(path: &Path) -> bool {
    fs::File::open(path).is_ok()
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

fn current_time_millis() -> u64 {
    system_time_millis(SystemTime::now()).unwrap_or_default()
}

fn to_task_process_result(task_id: String, result: ProcessorResult) -> TaskProcessResult {
    let continue_sequence = processor_result_continues_sequence(&result);

    TaskProcessResult {
        task_id,
        state: result.state,
        code: result.code,
        continue_sequence,
        message: result.message,
        log: result.log,
        report_path: result.report_path,
        print_report_path: result.print_report_path,
        failure: result.failure,
        source_csv_path: result.source_csv_path,
        csv_fingerprint: result.csv_fingerprint,
    }
}

fn is_external_workbook_lock_error(error: &ReportError) -> bool {
    match error {
        ReportError::Io(source) => is_locked_file_error(source),
        ReportError::Zip(zip::result::ZipError::Io(source)) => is_locked_file_error(source),
        _ => false,
    }
}

fn processor_result_continues_sequence(result: &ProcessorResult) -> bool {
    result.state == "fail"
        && result
            .failure
            .as_ref()
            .is_some_and(|failure| failure.title == ACCURACY_CHECK_FAILURE_TITLE)
}

fn process_task_with_profile_mapping(
    profile: &ReportLayoutProfile,
    task_id: &str,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
) -> Result<Option<ProcessorResult>, ReportError> {
    let Some(task) = profile_task(profile, task_id) else {
        return Ok(None);
    };

    if task.mappings.is_empty() {
        return Ok(None);
    }

    mapped::process_task(profile, task, unit_folder, already_processed_fingerprint).map(Some)
}

fn compute_task_output(
    profile: &ReportLayoutProfile,
    task: &tasks::AutomationTask,
    task_id: &str,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
    report_config: &ReportFileConfig,
) -> Result<ProcessorTaskOutput, ReportError> {
    if let Some(output) = compute_task_with_profile_mapping(
        profile,
        task_id,
        unit_folder,
        already_processed_fingerprint,
    )? {
        return Ok(output);
    }

    processors::compute_task(
        task,
        unit_folder,
        already_processed_fingerprint,
        report_config,
    )
}

fn compute_task_with_profile_mapping(
    profile: &ReportLayoutProfile,
    task_id: &str,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
) -> Result<Option<ProcessorTaskOutput>, ReportError> {
    let Some(task) = profile_task(profile, task_id) else {
        return Ok(None);
    };

    if task.mappings.is_empty() {
        return Ok(None);
    }

    mapped::compute_task(profile, task, unit_folder, already_processed_fingerprint).map(Some)
}

fn profile_task<'a>(profile: &'a ReportLayoutProfile, task_id: &str) -> Option<&'a TaskDefinition> {
    profile
        .task_groups
        .iter()
        .flat_map(|group| group.tasks.iter())
        .find(|task| task.id == task_id)
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
const OPEN_EXCEL_AT_LOCATION_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$rawPath = $env:PDU_REPORT_PATH
$sheet = $env:PDU_REPORT_SHEET
$cell = $env:PDU_REPORT_CELL
$statusPath = $env:PDU_REPORT_STATUS_PATH
$launchedExcel = $false
$excel = $null
$workbook = $null
$windowStateBefore = $null
$targetWindowState = -4137

Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class PduExcelLocationFocus {
  public delegate bool EnumWindowProc(IntPtr hWnd, IntPtr parameter);
  [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowProc callback, IntPtr parameter);
  [DllImport("user32.dll")] private static extern bool EnumChildWindows(IntPtr parent, EnumWindowProc callback, IntPtr parameter);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetClassName(IntPtr hWnd, StringBuilder className, int maxCount);
  [DllImport("oleacc.dll")] public static extern int AccessibleObjectFromWindow(IntPtr hWnd, uint objectId, ref Guid interfaceId, [MarshalAs(UnmanagedType.Interface)] out object nativeObject);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);

  private static string WindowClassName(IntPtr hWnd) {
    var className = new StringBuilder(256);
    GetClassName(hWnd, className, className.Capacity);
    return className.ToString();
  }

  public static IntPtr[] FindExcelDocumentWindows() {
    var documentWindows = new List<IntPtr>();
    EnumWindows((topLevelWindow, parameter) => {
      if (!string.Equals(WindowClassName(topLevelWindow), "XLMAIN", StringComparison.OrdinalIgnoreCase)) {
        return true;
      }

      EnumChildWindows(topLevelWindow, (childWindow, childParameter) => {
        if (string.Equals(WindowClassName(childWindow), "EXCEL7", StringComparison.OrdinalIgnoreCase)) {
          documentWindows.Add(childWindow);
        }
        return true;
      }, IntPtr.Zero);
      return true;
    }, IntPtr.Zero);
    return documentWindows.ToArray();
  }
}
"@

function Write-LocationStatus([string]$value) {
  [System.IO.File]::WriteAllText($statusPath, $value, [System.Text.Encoding]::UTF8)
}

function Normalize-PduPath([string]$value) {
  if ([string]::IsNullOrWhiteSpace($value)) {
    return $value
  }
  if ($value.StartsWith('\\?\UNC\', [System.StringComparison]::OrdinalIgnoreCase)) {
    return '\\' + $value.Substring(8)
  }
  if ($value.StartsWith('\\?\', [System.StringComparison]::OrdinalIgnoreCase)) {
    return $value.Substring(4)
  }
  try {
    return [System.IO.Path]::GetFullPath($value)
  } catch {
    return $value
  }
}

function Release-PduComObject($value) {
  if ($null -eq $value -or -not [Runtime.InteropServices.Marshal]::IsComObject($value)) {
    return
  }

  try {
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($value)
  } catch {
  }
}

function Get-ExcelWorkbookBinding([string]$targetPath) {
  $interfaceId = [Guid]'00020400-0000-0000-C000-000000000046'
  $nativeObjectId = [uint32]4294967280

  foreach ($documentWindowHandle in [PduExcelLocationFocus]::FindExcelDocumentWindows()) {
    $nativeWindow = $null
    $excelApplication = $null
    $candidate = $null

    try {
      $result = [PduExcelLocationFocus]::AccessibleObjectFromWindow(
        $documentWindowHandle,
        $nativeObjectId,
        [ref]$interfaceId,
        [ref]$nativeWindow
      )

      if ($result -eq 0 -and $null -ne $nativeWindow) {
        $excelApplication = $nativeWindow.Application
        for ($index = 1; $index -le $excelApplication.Workbooks.Count; $index += 1) {
          $candidate = $excelApplication.Workbooks.Item($index)
          $candidatePath = Normalize-PduPath $candidate.FullName
          if ([string]::Equals($candidatePath, $targetPath, [System.StringComparison]::OrdinalIgnoreCase)) {
            return [PSCustomObject]@{
              Excel = $excelApplication
              Workbook = $candidate
              NativeWindow = $nativeWindow
            }
          }

          Release-PduComObject $candidate
          $candidate = $null
        }
      }
    } catch {
    }

    Release-PduComObject $candidate
    Release-PduComObject $excelApplication
    Release-PduComObject $nativeWindow
  }

  return $null
}

function Focus-ExcelWindow($excelApplication) {
  try {
    $hwnd = [IntPtr]$excelApplication.Hwnd
    if ($hwnd -ne [IntPtr]::Zero) {
      [void][PduExcelLocationFocus]::BringWindowToTop($hwnd)
      [void][PduExcelLocationFocus]::SetForegroundWindow($hwnd)
    }
  } catch {
  }
}

$path = Normalize-PduPath $rawPath

function Select-ReportLocation($excelApplication, $workbookToFocus, [string]$sheetName, [string]$cellReference) {
  [void]$workbookToFocus.Activate()
  try {
    [void]$workbookToFocus.Windows.Item(1).Activate()
  } catch {
  }
  $worksheetToFocus = $workbookToFocus.Worksheets.Item($sheetName)
  [void]$worksheetToFocus.Activate()
  $rangeToFocus = $worksheetToFocus.Range($cellReference)
  [void]$excelApplication.Goto($rangeToFocus, $false)
  try {
    $activeWindow = $excelApplication.ActiveWindow
    if ($null -ne $activeWindow) {
      $visibleRows = [int]$activeWindow.VisibleRange.Rows.Count
      $visibleColumns = [int]$activeWindow.VisibleRange.Columns.Count
      $rowOffset = [int][Math]::Floor($visibleRows / 2)
      $columnOffset = [int][Math]::Floor($visibleColumns / 2)
      $activeWindow.ScrollRow = [Math]::Max(1, [int]$rangeToFocus.Row - $rowOffset)
      $activeWindow.ScrollColumn = [Math]::Max(1, [int]$rangeToFocus.Column - $columnOffset)
    }
  } catch {
  }
}

function Test-ReportLocation($excelApplication, [string]$targetPath, [string]$sheetName, [string]$cellReference) {
  try {
    if ($null -eq $excelApplication.ActiveWorkbook -or
        $null -eq $excelApplication.ActiveSheet -or
        $null -eq $excelApplication.Selection) {
      return $false
    }

    $activeWorkbookPath = Normalize-PduPath $excelApplication.ActiveWorkbook.FullName
    $activeSheetName = [string]$excelApplication.ActiveSheet.Name
    $activeCellAddress = [string]$excelApplication.Selection.Address($false, $false)

    return [string]::Equals($activeWorkbookPath, $targetPath, [System.StringComparison]::OrdinalIgnoreCase) -and
      [string]::Equals($activeSheetName, $sheetName, [System.StringComparison]::OrdinalIgnoreCase) -and
      [string]::Equals($activeCellAddress, $cellReference, [System.StringComparison]::OrdinalIgnoreCase)
  } catch {
    return $false
  }
}

try {
  $binding = Get-ExcelWorkbookBinding $path
  if ($null -eq $binding) {
    Start-Process -FilePath $path | Out-Null
    $launchedExcel = $true
    $launchDeadline = [DateTime]::UtcNow.AddSeconds(20)

    while ($null -eq $binding -and [DateTime]::UtcNow -lt $launchDeadline) {
      Start-Sleep -Milliseconds 250
      $binding = Get-ExcelWorkbookBinding $path
    }
  }

  if ($null -eq $binding) {
    throw "Excel did not expose the requested workbook window for cell selection."
  }

  $excel = $binding.Excel
  $workbook = $binding.Workbook
  $nativeWindow = $binding.NativeWindow
  try {
    $windowStateBefore = [int]$excel.WindowState
  } catch {
  }

  $excel.Visible = $true
  if (-not $launchedExcel -and $null -ne $windowStateBefore -and $windowStateBefore -ne -4140) {
    $targetWindowState = $windowStateBefore
  }
  try {
    $excel.WindowState = $targetWindowState
  } catch {
  }
  $sheet = $sheet.Trim()
  $cell = $cell.Trim().ToUpperInvariant()

  $readyDeadline = [DateTime]::UtcNow.AddSeconds(15)
  while (-not $excel.Ready -and [DateTime]::UtcNow -lt $readyDeadline) {
    Start-Sleep -Milliseconds 250
  }

  Start-Sleep -Milliseconds 250
  if (-not (Test-ReportLocation $excel $path $sheet $cell)) {
    Select-ReportLocation $excel $workbook $sheet $cell
    Start-Sleep -Milliseconds 250
  }

  if (-not (Test-ReportLocation $excel $path $sheet $cell)) {
    Start-Sleep -Milliseconds 500
    Select-ReportLocation $excel $workbook $sheet $cell
    Start-Sleep -Milliseconds 250
  }

  if (-not (Test-ReportLocation $excel $path $sheet $cell)) {
    throw "Excel opened the workbook but did not stay focused on $sheet!$cell."
  }

  try {
    if ([int]$excel.WindowState -ne $targetWindowState) {
      $excel.WindowState = $targetWindowState
    }
  } catch {
  }
  Focus-ExcelWindow $excel
  Write-LocationStatus 'ok'
  Release-PduComObject $workbook
  Release-PduComObject $nativeWindow
  Release-PduComObject $excel
} catch {
  Write-LocationStatus ("error:" + $_.Exception.Message)
  Release-PduComObject $workbook
  Release-PduComObject $nativeWindow
  Release-PduComObject $excel
  exit 1
}
"#;

#[cfg(target_os = "windows")]
const CLOSE_EXCEL_WORKBOOK_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$rawPath = $env:PDU_REPORT_PATH

Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class PduExcelWorkbookClose {
  public delegate bool EnumWindowProc(IntPtr hWnd, IntPtr parameter);
  [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowProc callback, IntPtr parameter);
  [DllImport("user32.dll")] private static extern bool EnumChildWindows(IntPtr parent, EnumWindowProc callback, IntPtr parameter);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int GetClassName(IntPtr hWnd, StringBuilder className, int maxCount);
  [DllImport("oleacc.dll")] public static extern int AccessibleObjectFromWindow(IntPtr hWnd, uint objectId, ref Guid interfaceId, [MarshalAs(UnmanagedType.Interface)] out object nativeObject);

  private static string WindowClassName(IntPtr hWnd) {
    var className = new StringBuilder(256);
    GetClassName(hWnd, className, className.Capacity);
    return className.ToString();
  }

  public static IntPtr[] FindExcelDocumentWindows() {
    var documentWindows = new List<IntPtr>();
    EnumWindows((topLevelWindow, parameter) => {
      if (!string.Equals(WindowClassName(topLevelWindow), "XLMAIN", StringComparison.OrdinalIgnoreCase)) {
        return true;
      }

      EnumChildWindows(topLevelWindow, (childWindow, childParameter) => {
        if (string.Equals(WindowClassName(childWindow), "EXCEL7", StringComparison.OrdinalIgnoreCase)) {
          documentWindows.Add(childWindow);
        }
        return true;
      }, IntPtr.Zero);
      return true;
    }, IntPtr.Zero);
    return documentWindows.ToArray();
  }
}
"@

function Normalize-PduPath([string]$value) {
  if ([string]::IsNullOrWhiteSpace($value)) {
    return $value
  }
  if ($value.StartsWith('\\?\UNC\', [System.StringComparison]::OrdinalIgnoreCase)) {
    return '\\' + $value.Substring(8)
  }
  if ($value.StartsWith('\\?\', [System.StringComparison]::OrdinalIgnoreCase)) {
    return $value.Substring(4)
  }
  try {
    return [System.IO.Path]::GetFullPath($value)
  } catch {
    return $value
  }
}

function Release-PduComObject($value) {
  if ($null -eq $value -or -not [Runtime.InteropServices.Marshal]::IsComObject($value)) {
    return
  }

  try {
    [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($value)
  } catch {
  }
}

$path = Normalize-PduPath $rawPath
$interfaceId = [Guid]'00020400-0000-0000-C000-000000000046'
$nativeObjectId = [uint32]4294967280
$closed = $false

foreach ($documentWindowHandle in [PduExcelWorkbookClose]::FindExcelDocumentWindows()) {
  $nativeWindow = $null
  $excel = $null
  $candidate = $null

  try {
    $result = [PduExcelWorkbookClose]::AccessibleObjectFromWindow(
      $documentWindowHandle,
      $nativeObjectId,
      [ref]$interfaceId,
      [ref]$nativeWindow
    )

    if ($result -eq 0 -and $null -ne $nativeWindow) {
      $excel = $nativeWindow.Application
      for ($index = 1; $index -le $excel.Workbooks.Count; $index += 1) {
        $candidate = $excel.Workbooks.Item($index)
        $candidatePath = Normalize-PduPath $candidate.FullName
        if ([string]::Equals($candidatePath, $path, [System.StringComparison]::OrdinalIgnoreCase)) {
          $displayAlertsBefore = [bool]$excel.DisplayAlerts
          try {
            $excel.DisplayAlerts = $false
            [void]$candidate.Close($true)
            $closed = $true
            if ([int]$excel.Workbooks.Count -eq 0) {
              $excel.Quit()
            }
          } finally {
            try {
              $excel.DisplayAlerts = $displayAlertsBefore
            } catch {
            }
          }
          break
        }

        Release-PduComObject $candidate
        $candidate = $null
      }
    }
  } finally {
    Release-PduComObject $candidate
    Release-PduComObject $excel
    Release-PduComObject $nativeWindow
  }

  if ($closed) {
    break
  }
}

if ($closed) {
  Write-Output 'closed'
} else {
  Write-Output 'not-open'
}
"#;

#[cfg(target_os = "windows")]
fn close_excel_workbook(path: &Path) -> Result<bool, AutomationError> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Sta",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            CLOSE_EXCEL_WORKBOOK_SCRIPT,
        ])
        .env("PDU_REPORT_PATH", path)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            AutomationError::OpenReport(format!(
                "failed to start the Excel workbook close helper for {}: {error}",
                path.display()
            ))
        })?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AutomationError::OpenReport(format!(
            "failed to close the Excel workbook {}{}",
            path.display(),
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        )));
    }

    let status = String::from_utf8_lossy(&output.stdout);
    let status = status.trim_start_matches('\u{feff}').trim();
    match status.lines().last().unwrap_or_default().trim() {
        "closed" => Ok(true),
        "not-open" => Ok(false),
        other => Err(AutomationError::OpenReport(format!(
            "Excel workbook close helper returned an unexpected status for {}: {other}",
            path.display()
        ))),
    }
}

#[cfg(not(target_os = "windows"))]
fn close_excel_workbook(_path: &Path) -> Result<bool, AutomationError> {
    Ok(false)
}

#[cfg(target_os = "windows")]
fn open_excel_at_location(path: &Path, sheet: &str, cell: &str) -> Result<(), AutomationError> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let status_path = std::env::temp_dir().join(format!(
        "pdu-excel-location-{}-{}.status",
        std::process::id(),
        current_time_millis()
    ));
    let _ = fs::remove_file(&status_path);
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Sta",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            OPEN_EXCEL_AT_LOCATION_SCRIPT,
        ])
        .env("PDU_REPORT_PATH", path)
        .env("PDU_REPORT_SHEET", sheet)
        .env("PDU_REPORT_CELL", cell)
        .env("PDU_REPORT_STATUS_PATH", &status_path)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            AutomationError::OpenReport(format!(
                "failed to open Excel location {}!{} in {}: {error}",
                sheet,
                cell,
                path.display()
            ))
        })?;

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(status) = fs::read_to_string(&status_path) {
            let _ = fs::remove_file(&status_path);
            let status = status.trim_start_matches('\u{feff}').trim();

            if status == "ok" {
                return Ok(());
            }

            let _ = child.kill();
            let detail = status.strip_prefix("error:").unwrap_or(status);
            return Err(AutomationError::OpenReport(format!(
                "failed to open Excel location {sheet}!{cell} in {}: {detail}",
                path.display()
            )));
        }

        if let Ok(Some(status)) = child.try_wait() {
            let _ = fs::remove_file(&status_path);
            return Err(AutomationError::OpenReport(format!(
                "Excel location helper exited with {status} before selecting {sheet}!{cell} in {}",
                path.display()
            )));
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = fs::remove_file(&status_path);
            return Err(AutomationError::OpenReport(format!(
                "Excel did not confirm selection of {sheet}!{cell} in {} within 30 seconds",
                path.display()
            )));
        }

        thread::sleep(Duration::from_millis(100));
    }
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

    #[cfg(target_os = "windows")]
    use std::fs::OpenOptions;
    #[cfg(target_os = "windows")]
    use std::os::windows::fs::OpenOptionsExt;

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
    fn close_report_workbook_discovers_main_report_when_path_missing() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        write_minimal_workbook(&report, &["Test Summary"]);

        let result = close_report_workbook(unit_folder.display().to_string(), String::new())
            .expect("main report should be discovered");

        assert!(!result.closed);
        assert_eq!(
            PathBuf::from(result.path),
            fs::canonicalize(report).expect("canonical report")
        );
    }

    #[test]
    fn timer_start_prefers_first_detection_step_for_multistep_tasks() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let step71 = unit_folder.join("unit_STEP71_system.csv");
        let step72 = unit_folder.join("unit_STEP72_system.csv");

        fs::write(&step71, "a,b\n1,2\n").expect("write step71");
        fs::write(&step72, "a,b\n3,4\n").expect("write step72");

        let csv_index = CsvScanIndex::scan(&unit_folder);
        let expected = csv_index
            .latest_for_step(71)
            .and_then(|info| info.created_ms)
            .expect("step71 should have a start time");

        assert_eq!(
            csv_index.timer_start_millis_for_steps(&[71, 72]),
            Some(expected)
        );
    }

    #[test]
    fn generic_readiness_keeps_the_csv_creation_deadline_across_rescans() {
        let task = find_task("208v-transformer").expect("transformer task");
        let csv_match = TaskCsvMatch {
            path: Some(PathBuf::from("STEP14.csv")),
            created_ms: Some(1_000),
            processable: true,
            reason: "test CSV".to_string(),
        };
        let empty_index = CsvScanIndex {
            entries: Vec::new(),
        };

        let first = evaluate_task_readiness(&task, &empty_index, &csv_match, 2_000);
        let rescanned = evaluate_task_readiness(&task, &empty_index, &csv_match, 5_000);

        assert_eq!(first.phase_deadline_ms, Some(61_000));
        assert_eq!(rescanned.phase_deadline_ms, first.phase_deadline_ms);
        assert_eq!(rescanned.wait_phase, TaskWaitPhase::Timing);
        assert!(!rescanned.process_ready);
    }

    #[test]
    fn burn_in_requires_step72_after_the_two_hour_soak() {
        let index = CsvScanIndex {
            entries: vec![scan_entry(71, "UNIT_STEP71_SYSTEM.csv", 1_000, true)],
        };
        let task = find_task("system-burn-in").expect("burn-in task");
        let csv_match = TaskCsvMatch {
            path: None,
            created_ms: None,
            processable: false,
            reason: "STEP72 missing".to_string(),
        };

        let readiness = evaluate_task_readiness(&task, &index, &csv_match, 7_202_000);

        assert_eq!(readiness.wait_phase, TaskWaitPhase::WaitingStep72);
        assert_eq!(readiness.phase_deadline_ms, None);
        assert_eq!(readiness.pending_duration_seconds, 60);
        assert!(!readiness.process_ready);
    }

    #[test]
    fn burn_in_waits_for_both_the_step71_soak_and_step72_capture() {
        let index = CsvScanIndex {
            entries: vec![
                scan_entry(71, "UNIT_STEP71_SYSTEM.csv", 1_000, true),
                scan_entry(
                    72,
                    "UNIT_STEP72_SYSTEM_ACCURACY_TEST_DATA_AVG.csv",
                    7_191_000,
                    true,
                ),
            ],
        };
        let task = find_task("system-burn-in").expect("burn-in task");
        let csv_match = TaskCsvMatch {
            path: None,
            created_ms: None,
            processable: false,
            reason: "test match".to_string(),
        };

        let readiness = evaluate_task_readiness(&task, &index, &csv_match, 7_205_000);

        assert_eq!(readiness.wait_phase, TaskWaitPhase::Capturing);
        assert_eq!(readiness.phase_deadline_ms, Some(7_251_000));
        assert!(!readiness.process_ready);
    }

    #[test]
    fn burn_in_preflight_returns_retryable_wait_without_unit_state_write() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        fs::write(unit_folder.join("UNIT_STEP71_SYSTEM.csv"), "a,b\n1,2\n").expect("write STEP71");
        let profile = load_layout_profile().expect("layout profile");
        let task = find_task("system-burn-in").expect("burn-in task");

        let result = process_preflight(&task, &profile, &unit_folder, u64::MAX)
            .expect("preflight")
            .expect("STEP72 should still be required");

        assert_eq!(result.state, "waiting");
        assert_eq!(result.code, 2);
        assert!(result.message.contains("STEP72"));
        assert!(unit_state::load_unit_state(&unit_folder)
            .expect("load unit state")
            .is_none());
    }

    #[test]
    fn batch_stops_at_not_ready_burn_in_without_persisting_wait() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        fs::write(unit_folder.join("UNIT_STEP71_SYSTEM.csv"), "a,b\n1,2\n").expect("write STEP71");

        let batch = process_tasks(
            unit_folder.display().to_string(),
            vec!["system-burn-in".to_string()],
        )
        .expect("batch should return a retryable wait");

        assert_eq!(batch.committed_count, 0);
        assert_eq!(batch.stopped_task_id.as_deref(), Some("system-burn-in"));
        assert_eq!(batch.results.len(), 1);
        assert_eq!(batch.results[0].state, "waiting");
        assert!(unit_state::load_unit_state(&unit_folder)
            .expect("load unit state")
            .is_none());
    }

    fn scan_entry(step: u16, file_name: &str, created_ms: u64, readable: bool) -> CsvScanEntry {
        CsvScanEntry {
            path: PathBuf::from(file_name),
            step: Some(step),
            file_name_upper: file_name.to_ascii_uppercase(),
            modified: UNIX_EPOCH + std::time::Duration::from_millis(created_ms),
            created_ms: Some(created_ms),
            readable,
        }
    }

    #[test]
    fn csv_scan_index_matches_configured_patterns_without_rescanning() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let older = unit_folder.join("unit_STEP14_TRANSFORMER_TEST_DATA_AVG_old.csv");
        let newer = unit_folder.join("unit_STEP14_TRANSFORMER_TEST_DATA_AVG_new.csv");
        let unrelated = unit_folder.join("unit_STEP14_OTHER.csv");

        fs::write(&older, "a,b\n1,2\n").expect("write older");
        std::thread::sleep(std::time::Duration::from_millis(5));
        fs::write(&newer, "a,b\n3,4\n").expect("write newer");
        fs::write(&unrelated, "a,b\n5,6\n").expect("write unrelated");

        let csv_index = CsvScanIndex::scan(&unit_folder);
        let pattern_match = csv_index
            .latest_by_pattern("*STEP14*TRANSFORMER_TEST_DATA_AVG*.csv")
            .expect("pattern should match latest transformer CSV");
        let built_in_match = csv_index
            .latest_for_step_fragments(14, &["TRANSFORMER_TEST_DATA_AVG".to_string()])
            .expect("built-in fragments should match latest transformer CSV");

        assert_eq!(pattern_match.path, newer);
        assert_eq!(built_in_match.path, newer);
        assert_eq!(csv_index.detected_steps_for_task(&[14]), vec![14]);
    }

    #[cfg(windows)]
    #[test]
    fn csv_scan_index_marks_exclusively_locked_csv_unreadable() {
        use std::os::windows::fs::OpenOptionsExt;

        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let csv_path = unit_folder.join("unit_STEP14_TRANSFORMER_TEST_DATA_AVG.csv");
        fs::write(&csv_path, "a,b\n1,2\n").expect("write csv");
        let _locked_file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&csv_path)
            .expect("open CSV with exclusive sharing");

        let csv_index = CsvScanIndex::scan(&unit_folder);
        let csv = csv_index
            .latest_by_pattern("*STEP14*TRANSFORMER_TEST_DATA_AVG*.csv")
            .expect("locked CSV should still be detected by name");

        assert!(!csv.readable);
        assert_eq!(csv_index.detected_steps_for_task(&[14]), vec![14]);
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
    fn layout_profile_errors_map_to_stable_codes() {
        let read_failed =
            AutomationCommandError::from_layout_profile_error(LayoutProfileError::ReadFailed {
                path: "C:/PDU500/config/report-layouts/missing.json".to_string(),
                source: io::Error::new(io::ErrorKind::NotFound, "missing"),
            });
        assert_eq!(read_failed.code, "layout_profile_read_failed");
        assert!(read_failed.message.contains("layout profile"));

        let invalid_json =
            ReportLayoutProfile::from_json("{not valid json").expect_err("invalid JSON");
        let invalid_json = AutomationCommandError::from_layout_profile_error(invalid_json);
        assert_eq!(invalid_json.code, "layout_profile_invalid_json");

        let invalid_profile =
            AutomationCommandError::from_layout_profile_error(LayoutProfileError::InvalidProfile {
                path: "C:/PDU500/config/report-layouts/pdu500.rev02.layout.json".to_string(),
                details: "unsupported schema_version".to_string(),
            });
        assert_eq!(invalid_profile.code, "layout_profile_invalid");
        assert!(invalid_profile
            .details
            .as_deref()
            .is_some_and(|details| details.contains("unsupported schema_version")));
    }

    #[test]
    fn valid_profile_without_mapping_still_allows_built_in_processor_fallback() {
        let profile = ReportLayoutProfile::from_json(include_str!(
            "../../../config/report-layouts/pdu500.rev02.layout.json"
        ))
        .expect("default profile should parse");

        let result = process_task_with_profile_mapping(
            &profile,
            "208v-system-100% Load",
            Path::new("C:/PDU500/262343000072"),
            None,
        );

        assert!(result.expect("mapping lookup should not fail").is_none());
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
    fn system_verification_failure_writes_values_and_remains_failed() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        write_minimal_workbook(&report, &["System Test - 480_208"]);
        write_system_accuracy_csv(
            &unit_folder.join("unit_STEP17_SYSTEM_ACCURACY_TEST_DATA_AVG.csv"),
            true,
        );

        let result = process_task_at(
            unit_folder.display().to_string(),
            "208v-system-20% Load".to_string(),
            u64::MAX,
        )
        .expect("task should return result");

        assert_eq!(result.state, "fail", "{}", result.message);
        assert!(result.continue_sequence);
        let sheet_xml = worksheet_xml(&report, "xl/worksheets/sheet1.xml");
        assert_numeric_cell(&sheet_xml, "E57", "1");
        assert_numeric_cell(&sheet_xml, "F57", "2");
        assert_numeric_cell(&sheet_xml, "G57", "-50");

        let state = unit_state::load_or_default(&unit_folder).expect("state");
        assert_eq!(
            state
                .tasks
                .get("208v-system-20% Load")
                .map(|entry| entry.state.as_str()),
            Some("fail")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn single_task_locked_report_returns_workbook_locked_command_error() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        write_minimal_workbook(&report, &["System Test - 480_208"]);
        write_system_accuracy_csv(
            &unit_folder.join("unit_STEP17_SYSTEM_ACCURACY_TEST_DATA_AVG.csv"),
            false,
        );
        let _exclusive_report_handle = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&report)
            .expect("lock report exclusively");

        let error = process_task_at(
            unit_folder.display().to_string(),
            "208v-system-20% Load".to_string(),
            u64::MAX,
        )
        .expect_err("locked report should reject the task command");
        let command_error = AutomationCommandError::from_automation_error(error);

        assert_eq!(command_error.code, "workbook_locked");
        assert!(command_error
            .details
            .as_deref()
            .is_some_and(|details| details.contains("os error 32")));
        let state = unit_state::load_or_default(&unit_folder).expect("state");
        assert!(!state.tasks.contains_key("208v-system-20% Load"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn excel_location_launcher_uses_shell_owned_excel_session() {
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("AccessibleObjectFromWindow"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("FindExcelDocumentWindows"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("Start-Process -FilePath $path"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("Get-ExcelWorkbookBinding $path"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("$workbookToFocus.Activate()"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("function Test-ReportLocation"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT
            .contains("if (-not (Test-ReportLocation $excel $path $sheet $cell))"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("Goto($rangeToFocus, $false)"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("VisibleRange.Rows.Count"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("$excel.WindowState = $targetWindowState"));
        assert!(
            OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("return [System.IO.Path]::GetFullPath($value)")
        );
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("return $value"));
        assert!(!OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("ShowWindow"));
        assert!(!OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("for ($attempt"));
        assert!(!OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("GetActiveObject"));
        assert!(!OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("BindToMoniker"));
        assert!(!OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("New-Object -ComObject Excel.Application"));
        assert!(!OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("$excel.Quit()"));
        assert!(!OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("while ($true)"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("Write-LocationStatus 'ok'"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("FinalReleaseComObject($value)"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("Release-PduComObject $excel"));
        assert!(OPEN_EXCEL_AT_LOCATION_SCRIPT.contains("SetForegroundWindow"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn excel_workbook_close_targets_only_the_matching_workbook() {
        assert!(CLOSE_EXCEL_WORKBOOK_SCRIPT.contains("AccessibleObjectFromWindow"));
        assert!(CLOSE_EXCEL_WORKBOOK_SCRIPT.contains("FindExcelDocumentWindows"));
        assert!(CLOSE_EXCEL_WORKBOOK_SCRIPT.contains("$candidate.Close($true)"));
        assert!(CLOSE_EXCEL_WORKBOOK_SCRIPT.contains("[int]$excel.Workbooks.Count -eq 0"));
        assert!(CLOSE_EXCEL_WORKBOOK_SCRIPT.contains("$excel.Quit()"));
        assert!(CLOSE_EXCEL_WORKBOOK_SCRIPT.contains("$candidate.FullName"));
        assert!(CLOSE_EXCEL_WORKBOOK_SCRIPT.contains("OrdinalIgnoreCase"));
        assert!(!CLOSE_EXCEL_WORKBOOK_SCRIPT.contains("GetActiveObject"));
        assert!(!CLOSE_EXCEL_WORKBOOK_SCRIPT.contains("BindToMoniker"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "opens real Excel; set PDU_EXCEL_LOCATION_SMOKE_WORKBOOK"]
    fn real_excel_location_launcher_selects_requested_cell() {
        let workbook_path = std::env::var_os("PDU_EXCEL_LOCATION_SMOKE_WORKBOOK")
            .map(PathBuf::from)
            .expect("set PDU_EXCEL_LOCATION_SMOKE_WORKBOOK to a disposable workbook copy");
        let sheet = std::env::var("PDU_EXCEL_LOCATION_SMOKE_SHEET")
            .unwrap_or_else(|_| "Test Summary".to_string());
        let cell =
            std::env::var("PDU_EXCEL_LOCATION_SMOKE_CELL").unwrap_or_else(|_| "A1".to_string());

        open_excel_at_location(&workbook_path, &sheet, &cell)
            .expect("Excel should confirm the requested sheet and cell selection");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "closes a real Excel workbook; set PDU_EXCEL_CLOSE_SMOKE_WORKBOOK"]
    fn real_excel_workbook_close_closes_requested_workbook() {
        let workbook_path = std::env::var_os("PDU_EXCEL_CLOSE_SMOKE_WORKBOOK")
            .map(PathBuf::from)
            .expect("set PDU_EXCEL_CLOSE_SMOKE_WORKBOOK to an open disposable workbook");

        assert!(close_excel_workbook(&workbook_path)
            .expect("Excel should close the requested disposable workbook"));
    }

    #[test]
    fn breaker_verification_failure_can_be_accepted_and_fails_again_when_reprocessed() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        write_minimal_workbook(&report, &["Subfeed - 480_208"]);
        write_failing_breaker_accuracy_csv(
            &unit_folder.join("unit_STEP23_SUB_FEED_02_ACCURACY_TEST_DATA_AVG.csv"),
        );

        let result = process_task_at(
            unit_folder.display().to_string(),
            "208v-breaker-2-20% Load".to_string(),
            u64::MAX,
        )
        .expect("task should return result");

        assert_eq!(result.state, "fail", "{}", result.message);
        assert!(result.continue_sequence);
        assert_eq!(
            result
                .failure
                .as_ref()
                .and_then(|failure| failure.location.as_ref())
                .map(|location| location.cell.as_str()),
            Some("I31")
        );
        let sheet_xml = worksheet_xml(&report, "xl/worksheets/sheet1.xml");
        assert_numeric_cell(&sheet_xml, "G31", "1");
        assert_numeric_cell(&sheet_xml, "H31", "2");
        assert_numeric_cell(&sheet_xml, "I31", "100");

        let state = unit_state::load_or_default(&unit_folder).expect("state");
        assert_eq!(
            state
                .tasks
                .get("208v-breaker-2-20% Load")
                .map(|entry| entry.state.as_str()),
            Some("fail")
        );

        let accepted = accept_task_failure(
            unit_folder.display().to_string(),
            "208v-breaker-2-20% Load".to_string(),
        )
        .expect("operator should be able to accept the accuracy failure");
        let accepted_task = accepted
            .tasks
            .iter()
            .find(|task| task.task_id == "208v-breaker-2-20% Load")
            .expect("accepted task summary");
        assert_eq!(accepted_task.state, "pass");
        assert!(accepted_task.accepted);

        let rerun = process_task_at(
            unit_folder.display().to_string(),
            "208v-breaker-2-20% Load".to_string(),
            u64::MAX,
        )
        .expect("accepted data should be processed again");
        assert_eq!(rerun.state, "fail");

        let rerun_summary = scan_unit_folder(unit_folder.display().to_string())
            .expect("summary after rerun failure");
        let rerun_task = rerun_summary
            .tasks
            .iter()
            .find(|task| task.task_id == "208v-breaker-2-20% Load")
            .expect("rerun task summary");
        assert_eq!(rerun_task.state, "fail");
        assert!(!rerun_task.accepted);
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
    fn batch_processes_passing_tasks_and_records_after_commit() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        write_minimal_workbook(&report, &["System Test - 480_208"]);
        write_system_accuracy_csv(
            &unit_folder.join("unit_STEP16_SYSTEM_ACCURACY_TEST_DATA_AVG.csv"),
            false,
        );
        write_system_accuracy_csv(
            &unit_folder.join("unit_STEP17_SYSTEM_ACCURACY_TEST_DATA_AVG.csv"),
            false,
        );

        let batch = process_tasks_at(
            unit_folder.display().to_string(),
            vec![
                "208v-system-50% Load".to_string(),
                "208v-system-20% Load".to_string(),
            ],
            u64::MAX,
        )
        .expect("batch should process");

        assert_eq!(batch.results.len(), 2, "{batch:?}");
        assert!(batch.committed);
        assert_eq!(batch.committed_count, 2);
        assert_eq!(batch.stopped_task_id, None);
        assert!(batch.results.iter().all(|result| result.state == "pass"));

        let state = unit_state::load_or_default(&unit_folder).expect("state");
        assert_eq!(
            state
                .tasks
                .get("208v-system-50% Load")
                .map(|entry| entry.state.as_str()),
            Some("pass")
        );
        assert_eq!(
            state
                .tasks
                .get("208v-system-20% Load")
                .map(|entry| entry.state.as_str()),
            Some("pass")
        );
        assert_workbook_package_is_valid_after_patch(&report);
    }

    #[test]
    fn batch_commit_failure_does_not_record_tentative_passes() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        fs::write(&report, b"not an xlsx zip").expect("write corrupt workbook");
        write_system_accuracy_csv(
            &unit_folder.join("unit_STEP16_SYSTEM_ACCURACY_TEST_DATA_AVG.csv"),
            false,
        );
        write_system_accuracy_csv(
            &unit_folder.join("unit_STEP17_SYSTEM_ACCURACY_TEST_DATA_AVG.csv"),
            false,
        );

        let batch = process_tasks_at(
            unit_folder.display().to_string(),
            vec![
                "208v-system-50% Load".to_string(),
                "208v-system-20% Load".to_string(),
            ],
            u64::MAX,
        )
        .expect("batch should return commit failure result");

        assert_eq!(batch.results.len(), 1);
        assert!(!batch.committed);
        assert_eq!(batch.committed_count, 0);
        assert_eq!(
            batch.stopped_task_id.as_deref(),
            Some("208v-system-50% Load")
        );
        assert_eq!(batch.results[0].state, "fail");
        assert!(batch.results[0].message.contains("Report commit failed"));

        let state = unit_state::load_or_default(&unit_folder).expect("state");
        assert_eq!(
            state
                .tasks
                .get("208v-system-50% Load")
                .map(|entry| entry.state.as_str()),
            Some("fail")
        );
        assert!(!state.tasks.contains_key("208v-system-20% Load"));
    }

    #[test]
    fn batch_commit_failure_keeps_prior_idempotent_pass() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        fs::write(&report, b"not an xlsx zip").expect("write corrupt workbook");
        let idempotent_csv = unit_folder.join("unit_STEP16_SYSTEM_ACCURACY_TEST_DATA_AVG.csv");
        write_system_accuracy_csv(&idempotent_csv, false);
        let idempotent_fingerprint =
            csv_data::csv_fingerprint(&idempotent_csv).expect("fingerprint");
        write_system_accuracy_csv(
            &unit_folder.join("unit_STEP17_SYSTEM_ACCURACY_TEST_DATA_AVG.csv"),
            false,
        );
        let mut state = unit_state::UnitState::default();
        state.tasks.insert(
            "208v-system-50% Load".to_string(),
            unit_state::UnitTaskState {
                task_id: "208v-system-50% Load".to_string(),
                state: "pass".to_string(),
                code: Some(0),
                source_csv_path: Some(idempotent_csv.display().to_string()),
                csv_fingerprint: Some(idempotent_fingerprint),
                processed_at: Some(unit_state::now_string()),
                result: Some("already processed".to_string()),
                accepted: unit_state::TaskAcceptance::default(),
                audit_log: Vec::new(),
            },
        );
        unit_state::save_unit_state(&unit_folder, &state).expect("write state");

        let batch = process_tasks_at(
            unit_folder.display().to_string(),
            vec![
                "208v-system-50% Load".to_string(),
                "208v-system-20% Load".to_string(),
            ],
            u64::MAX,
        )
        .expect("batch should return commit failure result");

        assert_eq!(batch.results.len(), 2);
        assert!(!batch.committed);
        assert_eq!(batch.committed_count, 1);
        assert_eq!(
            batch.stopped_task_id.as_deref(),
            Some("208v-system-20% Load")
        );
        assert_eq!(batch.results[0].task_id, "208v-system-50% Load");
        assert_eq!(batch.results[0].state, "pass");
        assert_eq!(batch.results[1].task_id, "208v-system-20% Load");
        assert_eq!(batch.results[1].state, "fail");

        let state = unit_state::load_or_default(&unit_folder).expect("state");
        assert_eq!(
            state
                .tasks
                .get("208v-system-50% Load")
                .map(|entry| entry.state.as_str()),
            Some("pass")
        );
        assert_eq!(
            state
                .tasks
                .get("208v-system-20% Load")
                .map(|entry| entry.state.as_str()),
            Some("fail")
        );
    }

    #[test]
    fn batch_commits_verification_failure_and_continues() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("262343000072");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let report = unit_folder.join(format!(
            "{}262343000072.xlsx",
            reports::MAIN_REPORT_SN_PREFIX
        ));
        write_minimal_workbook(&report, &["System Test - 480_208"]);
        write_system_accuracy_csv(
            &unit_folder.join("unit_STEP16_SYSTEM_ACCURACY_TEST_DATA_AVG.csv"),
            false,
        );
        write_system_accuracy_csv(
            &unit_folder.join("unit_STEP17_SYSTEM_ACCURACY_TEST_DATA_AVG.csv"),
            true,
        );

        let mut progress = Vec::new();
        let batch = process_tasks_with_progress_at(
            unit_folder.display().to_string(),
            vec![
                "208v-system-20% Load".to_string(),
                "208v-system-50% Load".to_string(),
            ],
            u64::MAX,
            |event| progress.push(event),
        )
        .expect("batch should continue after failed verification");

        assert_eq!(batch.results.len(), 2);
        assert!(batch.committed);
        assert_eq!(batch.committed_count, 1);
        assert_eq!(batch.stopped_task_id, None);
        assert_eq!(batch.results[0].state, "fail");
        assert!(batch.results[0].continue_sequence);
        assert_eq!(batch.results[1].state, "pass");
        assert!(!batch.results[1].continue_sequence);
        assert!(batch.message.contains("1 failed"));

        let failed_commit_index = progress
            .iter()
            .position(|event| event.task_id == "208v-system-20% Load" && event.state == "fail")
            .expect("failed task should emit committed progress");
        let next_processing_index = progress
            .iter()
            .position(|event| {
                event.task_id == "208v-system-50% Load" && event.state == "processing"
            })
            .expect("next task should emit processing progress");
        assert!(next_processing_index < failed_commit_index);

        let state = unit_state::load_or_default(&unit_folder).expect("state");
        assert_eq!(
            state
                .tasks
                .get("208v-system-50% Load")
                .map(|entry| entry.state.as_str()),
            Some("pass")
        );
        assert_eq!(
            state
                .tasks
                .get("208v-system-20% Load")
                .map(|entry| entry.state.as_str()),
            Some("fail")
        );
        let sheet_xml = worksheet_xml(&report, "xl/worksheets/sheet1.xml");
        assert_numeric_cell(&sheet_xml, "G57", "-50");
        assert_numeric_cell(&sheet_xml, "G37", "0");
        assert_workbook_package_is_valid_after_patch(&report);
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

        assert!(
            summary.serial_number.is_some(),
            "sample should produce a serial number"
        );
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
            let result = process_task_at(
                unit_copy.display().to_string(),
                task_id.to_string(),
                u64::MAX,
            )
            .unwrap();

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

    fn write_minimal_workbook(path: &Path, sheet_names: &[&str]) {
        let file = File::create(path).expect("create workbook");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", options)
            .expect("content types");
        zip.write_all(minimal_content_types_xml(sheet_names.len()).as_bytes())
            .expect("write content types");

        zip.start_file("xl/workbook.xml", options)
            .expect("workbook xml");
        zip.write_all(minimal_workbook_xml(sheet_names).as_bytes())
            .expect("write workbook");

        zip.start_file("xl/_rels/workbook.xml.rels", options)
            .expect("workbook rels");
        zip.write_all(minimal_workbook_rels_xml(sheet_names.len()).as_bytes())
            .expect("write rels");

        for index in 1..=sheet_names.len() {
            zip.start_file(format!("xl/worksheets/sheet{index}.xml"), options)
                .expect("sheet xml");
            zip.write_all(br#"<worksheet><sheetData></sheetData></worksheet>"#)
                .expect("write sheet");
        }

        zip.finish().expect("finish workbook");
    }

    fn minimal_content_types_xml(sheet_count: usize) -> String {
        let mut xml = String::from(
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        );

        for index in 1..=sheet_count {
            xml.push_str(&format!(
                r#"<Override PartName="/xl/worksheets/sheet{index}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
            ));
        }

        xml.push_str("</Types>");
        xml
    }

    fn minimal_workbook_xml(sheet_names: &[&str]) -> String {
        let mut xml = String::from(
            r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#,
        );

        for (index, name) in sheet_names.iter().enumerate() {
            let sheet_id = index + 1;
            xml.push_str(&format!(
                r#"<sheet name="{name}" sheetId="{sheet_id}" r:id="rId{sheet_id}"/>"#
            ));
        }

        xml.push_str("</sheets></workbook>");
        xml
    }

    fn minimal_workbook_rels_xml(sheet_count: usize) -> String {
        let mut xml = String::from("<Relationships>");

        for index in 1..=sheet_count {
            xml.push_str(&format!(
                r#"<Relationship Id="rId{index}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{index}.xml"/>"#
            ));
        }

        xml.push_str("</Relationships>");
        xml
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

        for column in ["AC", "AD", "BQ", "BR", "DE", "DF", "EL", "EM"] {
            let index = csv_data::excel_col_to_index(column).expect("power column");
            row[index] = "1000".to_string();
        }

        if force_failure {
            let detect_voltage_a = csv_data::excel_col_to_index("CH").expect("CH index");
            row[detect_voltage_a] = "2".to_string();
        }

        fs::write(path, format!("{}\n{}\n", headers.join(","), row.join(","))).expect("write csv");
    }

    fn write_failing_breaker_accuracy_csv(path: &Path) {
        let column_count = csv_data::excel_col_to_index("EG").expect("EG index") + 1;
        let headers = (0..column_count)
            .map(|index| format!("col{index}"))
            .collect::<Vec<_>>();
        let mut row = vec!["1".to_string(); column_count];
        let detect_voltage_a = csv_data::excel_col_to_index("DO").expect("DO index");
        row[detect_voltage_a] = "2".to_string();

        fs::write(path, format!("{}\n{}\n", headers.join(","), row.join(","))).expect("write csv");
    }

    fn worksheet_xml(path: &Path, sheet_path: &str) -> String {
        let mut archive = ZipArchive::new(File::open(path).expect("open workbook")).expect("zip");
        let mut xml = String::new();
        archive
            .by_name(sheet_path)
            .expect("sheet")
            .read_to_string(&mut xml)
            .expect("read sheet");
        xml
    }

    fn assert_numeric_cell(xml: &str, cell: &str, value: &str) {
        let expected = format!(r#"<c r="{cell}"><v>{value}</v></c>"#);
        assert!(
            xml.contains(&expected),
            "expected cell value {expected} in worksheet XML:\n{xml}"
        );
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
