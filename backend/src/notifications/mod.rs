//! Teams notification primitives kept independent from the automation workflow.

mod app_settings;
mod config;
mod message;
mod shift_log;
mod teams;
mod worker;

pub use app_settings::{
    app_settings_path, change_password, change_settings_password, ensure_configured_shared_layout,
    load_app_settings, load_app_settings_from, load_runtime_resolved_config, save_app_settings,
    save_app_settings_request, save_app_settings_request_to, save_app_settings_to,
    set_app_config_dir, station_name_for_id, verify_password, verify_settings_password,
    AppNotificationSettings, AppNotificationSettingsView, AppSettingsError,
    ChangeSettingsPasswordRequest, SaveAppNotificationSettingsRequest, APP_SETTINGS_SCHEMA_VERSION,
    DEFAULT_SETTINGS_PASSWORD, SETTINGS_FILE_NAME,
};

pub use config::{
    can_send, default_station_path, load_config, load_config_from, station_path_candidates,
    ConfigError, EventToggles, ResolvedConfig, SettingsFile, StationFile, StationSettings,
    NOTIFICATIONS_STATION_PATH_ENV, STABLE_STATION_PATH,
};
pub use message::{
    build_payload, build_teams_payload, format_event_message, format_event_message_now,
    now_timestamp, EventKind, MessageSection, NotificationEvent, NotificationMessage,
};
pub use shift_log::{
    append_event as append_shift_log_event, ensure_shared_root_layout, load_shift_log,
    resolve_shift_log_file, shared_root_directory, LoggedEvent, ShiftLog, ShiftLogError,
    ShiftLogEventKind, SHARED_STATION_IDS, SHIFT_LOG_FILE_NAME, SHIFT_LOG_SCHEMA_VERSION,
    STATIONS_DIR_NAME,
};
pub use teams::{TeamsClient, TeamsError, TransportFailure};
pub use worker::{NotificationRuntimeStatus, NotificationService};
