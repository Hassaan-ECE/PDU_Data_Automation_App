use serde::Serialize;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{Emitter, State};

use crate::{automation, config, notifications};

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
pub fn get_notification_status(
    notifications: State<'_, notifications::NotificationService>,
) -> notifications::NotificationRuntimeStatus {
    notifications.status()
}

#[tauri::command]
pub fn send_notification_test(notifications: State<'_, notifications::NotificationService>) {
    notifications.enqueue_test_ping();
}

#[tauri::command]
pub fn preview_shift_summary(
    shift_label: Option<String>,
) -> Result<notifications::ShiftSummaryPreview, String> {
    notifications::preview_shift_summary(shift_label.as_deref()).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn post_shift_summary(
    notification_service: State<'_, notifications::NotificationService>,
    request: notifications::PostShiftSummaryRequest,
) -> Result<notifications::ShiftSummaryResult, String> {
    let result = notifications::post_shift_summary(&request).map_err(|error| error.to_string())?;
    notification_service.mark_configuration_changed();
    Ok(result)
}

#[tauri::command]
pub fn get_app_notification_settings() -> Result<notifications::AppNotificationSettingsView, String>
{
    let (settings, floor) =
        notifications::load_app_settings_with_floor().map_err(|error| error.to_string())?;
    Ok(notifications::AppNotificationSettingsView::from_merged(
        &settings,
        floor.as_ref(),
    ))
}

#[tauri::command]
pub fn verify_settings_password(password: String) -> Result<bool, String> {
    notifications::verify_settings_password(&password).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_app_notification_settings(
    notification_service: State<'_, notifications::NotificationService>,
    mut request: notifications::SaveAppNotificationSettingsRequest,
) -> Result<notifications::AppNotificationSettingsView, String> {
    let station_id = request.station_id.trim().to_string();
    if !notifications::is_known_station_id(&station_id) {
        return Err("Select Test Station 1–4 or PDU Lab".to_string());
    }

    // Keep any Advanced catalog rename; only fill blank names from defaults.
    if request.station_name.trim().is_empty() {
        let renamed = request
            .stations
            .iter()
            .find(|entry| entry.id.trim() == station_id)
            .map(|entry| entry.name.trim().to_string())
            .filter(|name| !name.is_empty());
        request.station_name =
            renamed.unwrap_or_else(|| notifications::station_name_for_id(&station_id).to_string());
    }
    request.station_id = station_id;
    let saved =
        notifications::save_app_settings_request(&request).map_err(|error| error.to_string())?;
    notification_service.mark_configuration_changed();
    // Create stations/* + shared root after the settings write so a layout
    // failure does not block keeping the chosen folder. Worker appends also
    // re-ensure the layout on the next Problem/Complete delivery.
    if let Err(error) = notifications::ensure_configured_shared_layout(
        &notifications::load_app_settings().map_err(|error| error.to_string())?,
    ) {
        return Err(format!(
            "Settings saved, but the shared folder layout could not be prepared: {error}"
        ));
    }
    Ok(saved)
}

#[tauri::command]
pub fn change_settings_password(
    request: notifications::ChangeSettingsPasswordRequest,
) -> Result<(), String> {
    notifications::change_settings_password(&request).map_err(|error| error.to_string())
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
    notifications: State<'_, notifications::NotificationService>,
    unit_folder: String,
    unit_serial_number: Option<String>,
    transformer_sn: String,
) -> Result<automation::UnitFolderSummary, automation::AutomationCommandError> {
    let notification_folder = unit_folder.clone();
    let result = automation::setup_unit_folder_with_transformer_sn(
        unit_folder,
        unit_serial_number,
        transformer_sn,
    );
    if result.is_ok() {
        notifications.enqueue_complete_check(notification_folder);
    }
    result
}

#[tauri::command]
pub fn save_transformer_sn(
    notifications: State<'_, notifications::NotificationService>,
    unit_folder: String,
    transformer_sn: String,
) -> Result<(), automation::AutomationCommandError> {
    let notification_folder = unit_folder.clone();
    let result = automation::save_transformer_sn(unit_folder, transformer_sn);
    if result.is_ok() {
        notifications.enqueue_complete_check(notification_folder);
    }
    result
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
pub fn accept_automation_task_failure(
    notifications: State<'_, notifications::NotificationService>,
    unit_folder: String,
    task_id: String,
) -> Result<automation::UnitFolderSummary, String> {
    let notification_folder = unit_folder.clone();
    let result = automation::accept_task_failure(unit_folder, task_id);
    if result.is_ok() {
        notifications.enqueue_complete_check(notification_folder);
    }
    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub fn process_automation_task(
    notifications: State<'_, notifications::NotificationService>,
    unit_folder: String,
    task_id: String,
) -> Result<automation::TaskProcessResult, automation::AutomationCommandError> {
    let notification_folder = unit_folder.clone();
    let result = automation::process_task(unit_folder, task_id);
    if let Ok(processed) = &result {
        notifications.enqueue_task_results(notification_folder, vec![processed.clone()]);
    }
    result.map_err(automation::AutomationCommandError::from_automation_error)
}

#[tauri::command]
pub fn process_automation_tasks(
    app: tauri::AppHandle,
    notifications: State<'_, notifications::NotificationService>,
    unit_folder: String,
    task_ids: Vec<String>,
) -> Result<automation::TaskBatchProcessResult, automation::AutomationCommandError> {
    let notification_folder = unit_folder.clone();
    let result = automation::process_tasks_with_progress(unit_folder, task_ids, |progress| {
        let _ = app.emit(AUTOMATION_TASK_BATCH_PROGRESS_EVENT, progress);
    });
    if let Ok(batch) = &result {
        notifications.enqueue_task_results(notification_folder, batch.results.clone());
    }
    result.map_err(automation::AutomationCommandError::from_automation_error)
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

#[tauri::command]
pub fn close_report_workbook(
    unit_folder: String,
    path: String,
) -> Result<automation::CloseReportWorkbookResult, automation::AutomationCommandError> {
    automation::close_report_workbook(unit_folder, path)
}
