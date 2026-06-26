use serde::Serialize;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::Emitter;

use crate::{automation, config};

static PROCESS_START: OnceLock<Instant> = OnceLock::new();
static WINDOW_SETUP_UPTIME_MS: OnceLock<u128> = OnceLock::new();
pub const AUTOMATION_TASK_BATCH_PROGRESS_EVENT: &str = "automation-task-batch-progress";

#[derive(Debug, Serialize)]
pub struct BackendStatus {
    app_name: String,
    version: String,
    backend: String,
    process_uptime_ms: u128,
    window_setup_uptime_ms: Option<u128>,
}

pub fn process_start() -> Instant {
    *PROCESS_START.get_or_init(Instant::now)
}

pub fn mark_window_setup_elapsed(elapsed: Duration) {
    let _ = WINDOW_SETUP_UPTIME_MS.set(elapsed.as_millis());
}

#[tauri::command]
pub fn get_app_status() -> BackendStatus {
    let process_start = PROCESS_START.get_or_init(Instant::now);

    BackendStatus {
        app_name: "PDU Data Automation".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        backend: "tauri-rust".to_string(),
        process_uptime_ms: process_start.elapsed().as_millis(),
        window_setup_uptime_ms: WINDOW_SETUP_UPTIME_MS.get().copied(),
    }
}

#[tauri::command]
pub fn load_layout_profile() -> Result<config::ProfileLoadSummary, String> {
    config::load_layout_profile()
        .map(|profile| profile.to_load_summary())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn setup_unit_folder(unit_folder: String) -> Result<automation::UnitFolderSummary, String> {
    automation::setup_unit_folder(unit_folder).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn find_latest_unit_candidate() -> automation::LatestUnitCandidateResult {
    automation::find_latest_unit_candidate()
}

#[tauri::command]
pub fn setup_unit_folder_with_transformer_sn(
    unit_folder: String,
    unit_serial_number: Option<String>,
    transformer_sn: String,
) -> Result<automation::UnitFolderSummary, automation::AutomationCommandError> {
    automation::setup_unit_folder_with_transformer_sn(
        unit_folder,
        unit_serial_number,
        transformer_sn,
    )
}

#[tauri::command]
pub fn save_transformer_sn(
    unit_folder: String,
    transformer_sn: String,
) -> Result<(), automation::AutomationCommandError> {
    automation::save_transformer_sn(unit_folder, transformer_sn)
}

#[tauri::command]
pub fn save_final_operator_name(
    unit_folder: String,
    operator_name: String,
) -> Result<String, automation::AutomationCommandError> {
    automation::save_final_operator_name(unit_folder, operator_name)
}

#[tauri::command]
pub fn open_print_report_dialog(
    unit_folder: String,
) -> Result<(), automation::AutomationCommandError> {
    automation::open_print_report_dialog(unit_folder)
}

#[tauri::command]
pub fn validate_ready_for_print(
    unit_folder: String,
) -> Result<automation::PrintReadinessResult, automation::AutomationCommandError> {
    automation::validate_ready_for_print(unit_folder)
}

#[tauri::command]
pub fn scan_unit_folder(unit_folder: String) -> Result<automation::UnitFolderSummary, String> {
    automation::scan_unit_folder(unit_folder).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn process_automation_task(
    unit_folder: String,
    task_id: String,
) -> Result<automation::TaskProcessResult, String> {
    automation::process_task(unit_folder, task_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn process_automation_tasks(
    app: tauri::AppHandle,
    unit_folder: String,
    task_ids: Vec<String>,
) -> Result<automation::TaskBatchProcessResult, String> {
    automation::process_tasks_with_progress(unit_folder, task_ids, |progress| {
        let _ = app.emit(AUTOMATION_TASK_BATCH_PROGRESS_EVENT, progress);
    })
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_report_path(unit_folder: String, path: String) -> Result<(), String> {
    automation::open_report_path(unit_folder, path).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_report_location(
    unit_folder: String,
    path: String,
    sheet: String,
    cell: String,
) -> Result<(), String> {
    automation::open_report_location(unit_folder, path, sheet, cell)
        .map_err(|error| error.to_string())
}
