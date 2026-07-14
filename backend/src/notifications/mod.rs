//! Teams notification primitives kept independent from the automation workflow.

mod app_settings;
mod config;
mod floor_settings;
mod message;
mod shift_log;
mod stations;
mod summary;
mod teams;
mod worker;

pub use app_settings::{
    app_settings_path, catalog_from_local, change_password, change_settings_password,
    configured_shared_path_pointer, ensure_configured_shared_layout, load_app_settings,
    load_app_settings_from, load_app_settings_with_floor, load_runtime_resolved_config,
    save_app_settings, save_app_settings_request, save_app_settings_request_to,
    save_app_settings_to, set_app_config_dir, verify_password, verify_settings_password,
    AppNotificationSettings, AppNotificationSettingsView, AppSettingsError, CatalogCreateRequest,
    ChangeSettingsPasswordRequest, FloorSyncStatus, SaveAppNotificationSettingsRequest,
    SettingsSaveScope, ShiftWindow, StationCatalogEntry, APP_SETTINGS_SCHEMA_VERSION,
    DEFAULT_SETTINGS_PASSWORD, SETTINGS_FILE_NAME,
};
pub use floor_settings::{
    load_or_seed_floor_settings, resolve_floor_settings_file, try_load_floor_settings,
    update_floor_settings_with_lock, FloorSettings, FloorSettingsError, FloorStation,
    FLOOR_SETTINGS_FILE_NAME, FLOOR_SETTINGS_SCHEMA_V1, FLOOR_SETTINGS_SCHEMA_VERSION,
};
pub use stations::{
    is_known_station_id, known_stations_owned, station_name_for_id, StationRole,
    DEFAULT_SUMMARY_POSTER_STATION_ID, KNOWN_STATIONS,
};
pub use summary::{
    post_shift_summary, preview_shift_summary, PostShiftSummaryRequest, ShiftSummaryPreview,
    ShiftSummaryResult,
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
    append_event as append_shift_log_event, ensure_floor_station_directory,
    ensure_shared_root_layout, format_floor_summary, load_shift_log, mark_summary_and_clear,
    resolve_shift_log_file, shared_root_directory, shared_station_ids, LoggedEvent, ShiftLog,
    ShiftLogError, ShiftLogEventKind, SHIFT_LOG_FILE_NAME, SHIFT_LOG_SCHEMA_VERSION,
    STATIONS_DIR_NAME,
};
pub use teams::{TeamsClient, TeamsError, TransportFailure};
pub use worker::{NotificationRuntimeStatus, NotificationService};
