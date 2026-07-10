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

use regex::Regex;
use serde::Serialize;
use thiserror::Error;

use crate::config::{load_layout_profile, LayoutProfileError, ReportLayoutProfile, TaskDefinition};

use self::csv_data::{csv_fingerprint, csv_metadata_matches_fingerprint};
use self::processors::{FailureDetail, ProcessorResult, ProcessorTaskOutput};
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

    fn from_automation_error(error: AutomationError) -> Self {
        match error {
            AutomationError::Report(error) => Self::from_report_error(error),
            AutomationError::UnitState(error) => Self::from_unit_state_error(error),
            AutomationError::LayoutProfile(error) => Self::from_layout_profile_error(error),
            AutomationError::UnknownTask(task_id) => Self::validation(
                "unknown_task",
                format!("Unknown automation task id: {task_id}"),
            ),
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

pub fn process_task(
    unit_folder: String,
    task_id: String,
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
    let report_config = report_file_config(&profile);
    let already_processed_fingerprint = state
        .tasks
        .get(&task_id)
        .and_then(|entry| entry.already_processed_fingerprint())
        .map(ToOwned::to_owned);
    let result = process_task_with_profile_mapping(
        &profile,
        &task_id,
        &unit_folder,
        already_processed_fingerprint.as_deref(),
    )
    .unwrap_or_else(|| {
        processors::process_task(
            &task,
            &unit_folder,
            already_processed_fingerprint.as_deref(),
            &report_config,
        )
    });

    unit_state::record_processor_result(&unit_folder, &task_id, &result)?;

    Ok(to_task_process_result(task_id, result))
}

pub fn process_tasks(
    unit_folder: String,
    task_ids: Vec<String>,
) -> Result<TaskBatchProcessResult, AutomationError> {
    process_tasks_with_progress(unit_folder, task_ids, |_| {})
}

pub fn process_tasks_with_progress<F>(
    unit_folder: String,
    task_ids: Vec<String>,
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

    for task_id in task_ids {
        let task =
            find_task(&task_id).ok_or_else(|| AutomationError::UnknownTask(task_id.clone()))?;

        let output = if let Some(result) =
            already_processed_result_from_state(&unit_folder, &task_id, &task.label, &state)
        {
            ProcessorTaskOutput::result_only(result)
        } else {
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
            )
        };

        if output.result.state == "pass" {
            pending.push(PendingTaskOutput {
                task_id,
                result: output.result,
                patches: output.patches,
            });
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
    let message = if let Some(task_id) = &stopped_task_id {
        format!(
            "Batch stopped at {task_id} after finalizing {committed_count} task{}.",
            if committed_count == 1 { "" } else { "s" }
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

fn build_summary_with_profile(
    unit_folder: &Path,
    report_setup: ReportSetup,
    profile: &ReportLayoutProfile,
) -> Result<UnitFolderSummary, AutomationError> {
    let mut detected_task_ids = HashSet::<String>::new();
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
    processable: bool,
    reason: String,
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
            path: Some(mapped_csv.path.clone()),
        };
    }

    let Some((step, fragments)) = built_in_csv_requirement(task) else {
        return TaskCsvMatch {
            path: None,
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
            path: Some(csv.path.clone()),
        },
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
    profile: &ReportLayoutProfile,
    task_id: &str,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
) -> Option<ProcessorResult> {
    let task = profile_task(profile, task_id)?;

    if task.mappings.is_empty() {
        return None;
    }

    Some(mapped::process_task(
        profile,
        task,
        unit_folder,
        already_processed_fingerprint,
    ))
}

fn compute_task_output(
    profile: &ReportLayoutProfile,
    task: &tasks::AutomationTask,
    task_id: &str,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
    report_config: &ReportFileConfig,
) -> ProcessorTaskOutput {
    compute_task_with_profile_mapping(profile, task_id, unit_folder, already_processed_fingerprint)
        .unwrap_or_else(|| {
            processors::compute_task(
                task,
                unit_folder,
                already_processed_fingerprint,
                report_config,
            )
        })
}

fn compute_task_with_profile_mapping(
    profile: &ReportLayoutProfile,
    task_id: &str,
    unit_folder: &Path,
    already_processed_fingerprint: Option<&str>,
) -> Option<ProcessorTaskOutput> {
    let task = profile_task(profile, task_id)?;

    if task.mappings.is_empty() {
        return None;
    }

    Some(mapped::compute_task(
        profile,
        task,
        unit_folder,
        already_processed_fingerprint,
    ))
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

        assert!(result.is_none());
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

        let batch = process_tasks(
            unit_folder.display().to_string(),
            vec![
                "208v-system-50% Load".to_string(),
                "208v-system-20% Load".to_string(),
            ],
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

        let batch = process_tasks(
            unit_folder.display().to_string(),
            vec![
                "208v-system-50% Load".to_string(),
                "208v-system-20% Load".to_string(),
            ],
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

        let batch = process_tasks(
            unit_folder.display().to_string(),
            vec![
                "208v-system-50% Load".to_string(),
                "208v-system-20% Load".to_string(),
            ],
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
    fn batch_commits_prior_passes_before_stopping_on_verification_failure() {
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

        let batch = process_tasks(
            unit_folder.display().to_string(),
            vec![
                "208v-system-50% Load".to_string(),
                "208v-system-20% Load".to_string(),
            ],
        )
        .expect("batch should stop on failed verification");

        assert_eq!(batch.results.len(), 2);
        assert!(batch.committed);
        assert_eq!(batch.committed_count, 1);
        assert_eq!(
            batch.stopped_task_id.as_deref(),
            Some("208v-system-20% Load")
        );
        assert_eq!(batch.results[0].state, "pass");
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
