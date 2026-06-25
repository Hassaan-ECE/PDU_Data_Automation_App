pub mod automation;
pub mod commands;
pub mod config;

pub fn run() {
    let process_start = commands::process_start();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |_app| {
            commands::mark_window_setup_elapsed(process_start.elapsed());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::load_layout_profile,
            commands::setup_unit_folder,
            commands::find_latest_unit_candidate,
            commands::setup_unit_folder_with_transformer_sn,
            commands::save_transformer_sn,
            commands::save_final_operator_name,
            commands::open_print_report_dialog,
            commands::validate_ready_for_print,
            commands::scan_unit_folder,
            commands::process_automation_task,
            commands::open_report_path,
            commands::open_report_location
        ])
        .run(tauri::generate_context!())
        .expect("error while running PDU Data Automation");
}
