pub mod automation;
pub mod commands;
pub mod config;
pub mod notifications;

use tauri::Manager;

pub fn run() {
    let process_start = commands::process_start();

    tauri::Builder::default()
        .manage(notifications::NotificationService::start())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            if let Ok(resource_dir) = app.path().resource_dir() {
                config::set_runtime_resource_dir(resource_dir);
            }
            if let Ok(config_dir) = app.path().app_config_dir() {
                notifications::set_app_config_dir(config_dir);
                // Loading once creates the schema-v1 defaults on a fresh install.
                // Any I/O error stays soft and will be surfaced by runtime status.
                if let Err(error) = notifications::load_app_settings() {
                    eprintln!("Notification settings initialization failed: {error}");
                }
            }
            commands::mark_window_setup_elapsed(process_start.elapsed());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::get_notification_status,
            commands::get_app_notification_settings,
            commands::save_app_notification_settings,
            commands::verify_settings_password,
            commands::change_settings_password,
            commands::send_notification_test,
            commands::preview_shift_summary,
            commands::post_shift_summary,
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
            commands::process_automation_tasks,
            commands::open_report_path,
            commands::open_report_location
        ])
        .run(tauri::generate_context!())
        .expect("error while running PDU Data Automation");
}
