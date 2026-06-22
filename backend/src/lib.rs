pub mod automation;
pub mod config;

use serde::Serialize;
use std::sync::OnceLock;
use std::time::Instant;

static PROCESS_START: OnceLock<Instant> = OnceLock::new();
static WINDOW_SETUP_UPTIME_MS: OnceLock<u128> = OnceLock::new();

#[derive(Debug, Serialize)]
struct BackendStatus {
    app_name: String,
    version: String,
    backend: String,
    process_uptime_ms: u128,
    window_setup_uptime_ms: Option<u128>,
}

#[tauri::command]
fn get_app_status() -> BackendStatus {
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
fn load_layout_profile() -> Result<config::ProfileLoadSummary, String> {
    config::load_layout_profile()
        .map(|profile| profile.to_load_summary())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn setup_unit_folder(unit_folder: String) -> Result<automation::UnitFolderSummary, String> {
    automation::setup_unit_folder(unit_folder).map_err(|error| error.to_string())
}

#[tauri::command]
fn find_latest_unit_candidate() -> automation::LatestUnitCandidateResult {
    automation::find_latest_unit_candidate()
}

#[tauri::command]
fn setup_unit_folder_with_transformer_sn(
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
fn save_transformer_sn(
    unit_folder: String,
    transformer_sn: String,
) -> Result<(), automation::AutomationCommandError> {
    automation::save_transformer_sn(unit_folder, transformer_sn)
}

#[tauri::command]
fn scan_unit_folder(unit_folder: String) -> Result<automation::UnitFolderSummary, String> {
    automation::scan_unit_folder(unit_folder).map_err(|error| error.to_string())
}

#[tauri::command]
fn process_automation_task(
    unit_folder: String,
    task_id: String,
) -> Result<automation::TaskProcessResult, String> {
    automation::process_task(unit_folder, task_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn open_report_path(unit_folder: String, path: String) -> Result<(), String> {
    automation::open_report_path(unit_folder, path).map_err(|error| error.to_string())
}

#[tauri::command]
fn open_report_location(
    unit_folder: String,
    path: String,
    sheet: String,
    cell: String,
) -> Result<(), String> {
    automation::open_report_location(unit_folder, path, sheet, cell)
        .map_err(|error| error.to_string())
}

pub fn run() {
    let process_start = *PROCESS_START.get_or_init(Instant::now);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |_app| {
            let _ = WINDOW_SETUP_UPTIME_MS.set(process_start.elapsed().as_millis());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_status,
            load_layout_profile,
            setup_unit_folder,
            find_latest_unit_candidate,
            setup_unit_folder_with_transformer_sn,
            save_transformer_sn,
            scan_unit_folder,
            process_automation_task,
            open_report_path,
            open_report_location
        ])
        .run(tauri::generate_context!())
        .expect("error while running PDU Data Automation");
}
