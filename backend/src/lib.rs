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
fn load_example_layout_profile() -> Result<config::ProfileLoadSummary, String> {
    config::load_example_profile()
        .map(|profile| profile.to_load_summary())
        .map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_app_status,
            load_example_layout_profile
        ])
        .run(tauri::generate_context!())
        .expect("error while running PDU Data Automation");
}
