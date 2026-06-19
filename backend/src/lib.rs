pub mod automation;
pub mod config;

use serde::Serialize;

#[derive(Debug, Serialize)]
struct BackendStatus {
    app_name: String,
    version: String,
    backend: String,
}

#[tauri::command]
fn get_app_status() -> BackendStatus {
    BackendStatus {
        app_name: "PDU Data Automation".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        backend: "tauri-rust".to_string(),
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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_app_status,
            load_layout_profile,
            setup_unit_folder,
            scan_unit_folder,
            process_automation_task,
            open_report_path,
            open_report_location
        ])
        .run(tauri::generate_context!())
        .expect("error while running PDU Data Automation");
}
