//! App-owned notification settings stored under Tauri's `app_config_dir`.
//!
//! The public `*_from` helpers keep disk behavior testable without changing the
//! process-wide directory. Production calls use the directory installed once
//! during Tauri setup via [`set_app_config_dir`].
//!
//! When a shared notifications folder is configured, floor-wide fields (webhook,
//! password, shifts, poster, station display names, …) are authoritative in
//! `floor_settings.json` under that folder. Local AppData keeps this PC's
//! `station_id`, the shared-folder path pointer, and a last-known-good
//! `station_catalog` cache for offline summary labels.
//!
//! Saves are **scoped**: only the fields for the requested [`SettingsSaveScope`]
//! are patched. Connecting to an existing floor adopts it and never writes
//! stale form policy over the shared file.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::SystemTime;
use thiserror::Error;

use super::config::{EventToggles, ResolvedConfig};
use super::floor_settings::{
    generate_identity_id, try_load_floor_settings, validate_display_name, FloorSettings,
    FloorSettingsError, FloorStation, FLOOR_SETTINGS_SCHEMA_VERSION,
};
use super::shift_log;
use super::stations::{
    is_known_station_id, known_stations_owned, station_name_for_id, StationRole,
    DEFAULT_SUMMARY_POSTER_STATION_ID,
};

pub const APP_SETTINGS_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_SETTINGS_PASSWORD: &str = "0601";
pub const SETTINGS_FILE_NAME: &str = "notification_settings.json";

static CONFIG_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);
static SETTINGS_IO_LOCK: Mutex<()> = Mutex::new(());
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AppSettingsError {
    #[error("Could not read notification settings: {0}")]
    Read(String),
    #[error("Could not write notification settings: {0}")]
    Write(String),
    #[error("Invalid notification settings JSON: {0}")]
    Parse(String),
    #[error("Unsupported notification settings schema version {0}")]
    UnsupportedSchema(u32),
    #[error("Notification settings are busy; try again")]
    Busy,
    #[error("The application settings directory is unavailable")]
    ConfigDirectoryUnavailable,
    #[error("An interrupted notification settings write could not be recovered: {0}")]
    Recovery(String),
    #[error("Current password is incorrect")]
    WrongPassword,
    #[error("New password must not be empty")]
    EmptyPassword,
    #[error("New password and confirmation do not match")]
    PasswordMismatch,
    #[error("Floor password is incorrect")]
    WrongFloorPassword,
    #[error("station_id is empty")]
    EmptyStationId,
    #[error("Unknown station_id '{0}'")]
    UnknownStation(String),
    #[error("Connect a shared folder before adding identities")]
    SharedFolderRequiredForIdentity,
    #[error("Invalid shift window: {0}")]
    InvalidShift(String),
    #[error("{0}")]
    Floor(String),
}

impl From<FloorSettingsError> for AppSettingsError {
    fn from(error: FloorSettingsError) -> Self {
        match error {
            FloorSettingsError::Busy => AppSettingsError::Busy,
            FloorSettingsError::EmptyStationName
            | FloorSettingsError::StationNameTooLong
            | FloorSettingsError::InvalidStationName
            | FloorSettingsError::DuplicateStationName(_) => {
                AppSettingsError::Floor(error.to_string())
            }
            other => AppSettingsError::Floor(other.to_string()),
        }
    }
}

/// Which section of settings a save request is allowed to mutate.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSaveScope {
    /// Shifts, summary enabled (`events.summary`), Main poster, included stations.
    #[default]
    Operator,
    /// Shared identity catalog plus this PC's local identity selection.
    Identity,
    /// Teams delivery policy and Advanced notification event toggles.
    Teams,
    /// Webhook, destination, enabled, problem/complete/stuck toggles, idle
    /// timeout, station display names, this PC `station_id`. Retained for
    /// backward compatibility with combined-page callers.
    Advanced,
    /// Set shared path + adopt existing floor OR seed if missing; never clobber
    /// an existing floor with form policy.
    Connect,
    /// This-PC `station_id` only (and clearing path if emptied).
    Local,
}

/// Stable station id + editable display name exposed to the UI catalog.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct StationCatalogEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub role: StationRole,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct CatalogCreateRequest {
    pub name: String,
    #[serde(default)]
    pub role: StationRole,
    #[serde(default)]
    pub select_for_this_pc: bool,
}

/// Whether this PC is reading/writing floor-wide settings via the shared folder.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct FloorSyncStatus {
    pub configured: bool,
    pub source: String,
    pub updated_at: Option<String>,
    pub updated_by_station_id: Option<String>,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct ShiftWindow {
    #[serde(default)]
    pub label: String,
    /// Local 24h time `HH:MM`.
    #[serde(default)]
    pub start_time: String,
    /// Local 24h time `HH:MM`; values earlier than start wrap to the following day.
    #[serde(default)]
    pub end_time: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppNotificationSettings {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_password")]
    pub settings_password: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_destination_name")]
    pub teams_destination_name: String,
    #[serde(default)]
    pub teams_webhook_url: String,
    #[serde(default = "default_station_id")]
    pub station_id: String,
    #[serde(default = "default_station_name")]
    pub station_name: String,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_minutes: u32,
    #[serde(default = "default_app_events")]
    pub events: EventToggles,
    /// Empty means the optional floor-wide shift log is disabled.
    #[serde(default)]
    pub shared_shift_log_path: String,
    /// Optional shift windows (0–2 for v1). Times are local `HH:MM`.
    #[serde(default)]
    pub shifts: Vec<ShiftWindow>,
    /// Only this station_id may post the floor end-of-shift summary.
    #[serde(default = "default_summary_poster")]
    pub summary_poster_station_id: String,
    /// Stations listed on the end-of-shift card (empty = all known stations).
    #[serde(default = "default_summary_included")]
    pub summary_included_station_ids: Vec<String>,
    /// Last-known-good full floor station catalog (offline / missing floor).
    #[serde(default)]
    pub station_catalog: Vec<StationCatalogEntry>,
}

impl Default for AppNotificationSettings {
    fn default() -> Self {
        Self {
            schema_version: APP_SETTINGS_SCHEMA_VERSION,
            settings_password: default_password(),
            enabled: true,
            teams_destination_name: default_destination_name(),
            teams_webhook_url: String::new(),
            station_id: default_station_id(),
            station_name: default_station_name(),
            idle_timeout_minutes: default_idle_timeout(),
            events: default_app_events(),
            shared_shift_log_path: String::new(),
            shifts: Vec::new(),
            summary_poster_station_id: default_summary_poster(),
            summary_included_station_ids: default_summary_included(),
            station_catalog: Vec::new(),
        }
    }
}

impl fmt::Debug for AppNotificationSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppNotificationSettings")
            .field("schema_version", &self.schema_version)
            .field("settings_password", &"<redacted>")
            .field("enabled", &self.enabled)
            .field("teams_destination_name", &self.teams_destination_name)
            .field(
                "teams_webhook_url",
                &redacted_or_empty(&self.teams_webhook_url),
            )
            .field("station_id", &self.station_id)
            .field("station_name", &self.station_name)
            .field("idle_timeout_minutes", &self.idle_timeout_minutes)
            .field("events", &self.events)
            .field("shared_shift_log_path", &self.shared_shift_log_path)
            .field("shifts", &self.shifts)
            .field("summary_poster_station_id", &self.summary_poster_station_id)
            .field(
                "summary_included_station_ids",
                &self.summary_included_station_ids,
            )
            .field("station_catalog", &self.station_catalog)
            .finish()
    }
}

/// UI-safe settings. The webhook value is deliberately returned as an empty
/// string; `webhook_configured` lets the UI show a masked placeholder. Sending
/// an empty webhook back in a save request preserves the stored credential.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppNotificationSettingsView {
    pub enabled: bool,
    pub teams_destination_name: String,
    pub teams_webhook_url: String,
    pub webhook_configured: bool,
    pub station_id: String,
    pub station_name: String,
    pub idle_timeout_minutes: u32,
    pub events: EventToggles,
    pub shared_shift_log_path: String,
    pub shifts: Vec<ShiftWindow>,
    pub summary_poster_station_id: String,
    pub summary_included_station_ids: Vec<String>,
    pub is_summary_poster: bool,
    /// Floor catalog (fixed ids, editable display names).
    #[serde(default)]
    pub stations: Vec<StationCatalogEntry>,
    #[serde(default = "default_floor_sync_local")]
    pub floor_sync: FloorSyncStatus,
}

fn default_floor_sync_local() -> FloorSyncStatus {
    FloorSyncStatus {
        configured: false,
        source: "local".to_string(),
        updated_at: None,
        updated_by_station_id: None,
        message: "Shared folder not set — settings stay on this PC only.".to_string(),
    }
}

impl fmt::Debug for AppNotificationSettingsView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppNotificationSettingsView")
            .field("enabled", &self.enabled)
            .field("teams_destination_name", &self.teams_destination_name)
            .field(
                "teams_webhook_url",
                &redacted_or_empty(&self.teams_webhook_url),
            )
            .field("webhook_configured", &self.webhook_configured)
            .field("station_id", &self.station_id)
            .field("station_name", &self.station_name)
            .field("idle_timeout_minutes", &self.idle_timeout_minutes)
            .field("events", &self.events)
            .field("shared_shift_log_path", &self.shared_shift_log_path)
            .field("shifts", &self.shifts)
            .field("summary_poster_station_id", &self.summary_poster_station_id)
            .field(
                "summary_included_station_ids",
                &self.summary_included_station_ids,
            )
            .field("is_summary_poster", &self.is_summary_poster)
            .field("stations", &self.stations)
            .field("floor_sync", &self.floor_sync)
            .finish()
    }
}

impl From<&AppNotificationSettings> for AppNotificationSettingsView {
    fn from(settings: &AppNotificationSettings) -> Self {
        settings_view_from(settings, None)
    }
}

impl AppNotificationSettingsView {
    pub fn from_merged(settings: &AppNotificationSettings, floor: Option<&FloorSettings>) -> Self {
        settings_view_from(settings, floor)
    }
}

fn settings_view_from(
    settings: &AppNotificationSettings,
    floor: Option<&FloorSettings>,
) -> AppNotificationSettingsView {
    let poster = settings.summary_poster_station_id.trim();
    let poster = if poster.is_empty() {
        DEFAULT_SUMMARY_POSTER_STATION_ID
    } else {
        poster
    };
    let included = if settings.summary_included_station_ids.is_empty() {
        default_summary_included()
    } else {
        settings.summary_included_station_ids.clone()
    };
    let stations = match floor {
        Some(floor) => floor
            .stations
            .iter()
            .map(|s| StationCatalogEntry {
                id: s.id.clone(),
                name: s.name.clone(),
                role: s.role,
            })
            .collect(),
        None => catalog_from_local(settings),
    };
    let floor_sync = match floor {
        Some(floor) => FloorSyncStatus {
            configured: true,
            source: "floor".to_string(),
            updated_at: non_empty_opt(&floor.updated_at),
            updated_by_station_id: non_empty_opt(&floor.updated_by_station_id),
            message: "Syncing via shared folder.".to_string(),
        },
        None if settings.shared_shift_log_path.trim().is_empty() => default_floor_sync_local(),
        None => FloorSyncStatus {
            configured: true,
            source: "local-cache".to_string(),
            updated_at: None,
            updated_by_station_id: None,
            message: "Shared folder is set but floor settings are unavailable or stale; using local cache."
                .to_string(),
        },
    };
    AppNotificationSettingsView {
        enabled: settings.enabled,
        teams_destination_name: settings.teams_destination_name.clone(),
        teams_webhook_url: String::new(),
        webhook_configured: !settings.teams_webhook_url.trim().is_empty(),
        station_id: settings.station_id.clone(),
        station_name: settings.station_name.clone(),
        idle_timeout_minutes: settings.idle_timeout_minutes,
        events: settings.events.clone(),
        shared_shift_log_path: settings.shared_shift_log_path.clone(),
        shifts: settings.shifts.clone(),
        summary_poster_station_id: poster.to_string(),
        summary_included_station_ids: included,
        is_summary_poster: settings.station_id.trim() == poster,
        stations,
        floor_sync,
    }
}

/// Views: floor stations if present, else local `station_catalog`, else defaults.
pub fn catalog_from_local(settings: &AppNotificationSettings) -> Vec<StationCatalogEntry> {
    if !settings.station_catalog.is_empty() {
        return normalize_catalog_entries(&settings.station_catalog, settings);
    }
    known_stations_owned()
        .into_iter()
        .map(|(id, default_name)| {
            let name =
                if id == settings.station_id.trim() && !settings.station_name.trim().is_empty() {
                    settings.station_name.trim().to_string()
                } else {
                    default_name
                };
            StationCatalogEntry {
                id,
                name,
                role: StationRole::Floor,
            }
        })
        .collect()
}

fn normalize_catalog_entries(
    catalog: &[StationCatalogEntry],
    settings: &AppNotificationSettings,
) -> Vec<StationCatalogEntry> {
    let mut normalized = catalog
        .iter()
        .filter_map(|entry| {
            let id = entry.id.trim();
            let name = entry.name.trim();
            (!id.is_empty() && !name.is_empty()).then(|| StationCatalogEntry {
                id: id.to_string(),
                name: if id == settings.station_id.trim()
                    && !settings.station_name.trim().is_empty()
                {
                    settings.station_name.trim().to_string()
                } else {
                    name.to_string()
                },
                role: entry.role,
            })
        })
        .collect::<Vec<_>>();

    if normalized.is_empty() {
        normalized = known_stations_owned()
            .into_iter()
            .map(|(id, name)| StationCatalogEntry {
                id,
                name,
                role: StationRole::Floor,
            })
            .collect();
    }
    normalized
}

fn non_empty_opt(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Safe update shape: schema version and password cannot be overwritten by the
/// ordinary settings form. Empty webhook means "leave unchanged" unless the
/// explicit `clear_webhook` flag is set. Only fields for [`scope`] are applied.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveAppNotificationSettingsRequest {
    pub enabled: bool,
    pub teams_destination_name: String,
    #[serde(default)]
    pub teams_webhook_url: String,
    #[serde(default)]
    pub clear_webhook: bool,
    pub station_id: String,
    #[serde(default)]
    pub station_name: String,
    pub idle_timeout_minutes: u32,
    pub events: EventToggles,
    #[serde(default)]
    pub shared_shift_log_path: String,
    #[serde(default)]
    pub shifts: Vec<ShiftWindow>,
    #[serde(default)]
    pub summary_poster_station_id: String,
    #[serde(default)]
    pub summary_included_station_ids: Vec<String>,
    /// Optional station display-name updates (Advanced). Empty means leave catalog unchanged.
    #[serde(default)]
    pub stations: Vec<StationCatalogEntry>,
    /// Optional identity creation, applied under the shared floor lock.
    #[serde(default)]
    pub catalog_create: Option<CatalogCreateRequest>,
    /// Which fields this request may mutate (default: Operator).
    #[serde(default)]
    pub scope: SettingsSaveScope,
    /// Required when Connect scope targets an existing floor file.
    #[serde(default)]
    pub connect_password: String,
}

impl fmt::Debug for SaveAppNotificationSettingsRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SaveAppNotificationSettingsRequest")
            .field("enabled", &self.enabled)
            .field("teams_destination_name", &self.teams_destination_name)
            .field(
                "teams_webhook_url",
                &redacted_or_empty(&self.teams_webhook_url),
            )
            .field("clear_webhook", &self.clear_webhook)
            .field("station_id", &self.station_id)
            .field("station_name", &self.station_name)
            .field("idle_timeout_minutes", &self.idle_timeout_minutes)
            .field("events", &self.events)
            .field("shared_shift_log_path", &self.shared_shift_log_path)
            .field("shifts", &self.shifts)
            .field("summary_poster_station_id", &self.summary_poster_station_id)
            .field(
                "summary_included_station_ids",
                &self.summary_included_station_ids,
            )
            .field("stations", &self.stations)
            .field("catalog_create", &self.catalog_create)
            .field("scope", &self.scope)
            .field(
                "connect_password",
                if self.connect_password.is_empty() {
                    &"<empty>"
                } else {
                    &"<redacted>"
                },
            )
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeSettingsPasswordRequest {
    pub current_password: String,
    pub new_password: String,
    pub confirm_password: String,
}

impl fmt::Debug for ChangeSettingsPasswordRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChangeSettingsPasswordRequest")
            .field("current_password", &"<redacted>")
            .field("new_password", &"<redacted>")
            .field("confirm_password", &"<redacted>")
            .finish()
    }
}

/// Install Tauri's `app_config_dir` for all process-wide settings operations.
pub fn set_app_config_dir(path: PathBuf) {
    if path.as_os_str().is_empty() {
        return;
    }
    if let Ok(mut configured) = CONFIG_DIR.write() {
        *configured = Some(path);
    }
}

pub fn app_settings_path() -> PathBuf {
    CONFIG_DIR
        .read()
        .ok()
        .and_then(|configured| configured.clone())
        .map(|directory| directory.join(SETTINGS_FILE_NAME))
        .unwrap_or_else(|| PathBuf::from(SETTINGS_FILE_NAME))
}

/// Best-effort shared-path pointer from local settings only (no floor overlay).
/// Used by the floor poll loop to fingerprint before a merge load.
pub fn configured_shared_path_pointer() -> Option<String> {
    let path = app_settings_path();
    let raw = fs::read_to_string(path).ok()?;
    let settings: AppNotificationSettings = serde_json::from_str(&raw).ok()?;
    let shared = settings.shared_shift_log_path.trim().to_string();
    if shared.is_empty() {
        None
    } else {
        Some(shared)
    }
}

pub fn load_app_settings() -> Result<AppNotificationSettings, AppSettingsError> {
    load_app_settings_from(configured_app_settings_path()?)
}

/// Load settings with floor overlay when a shared folder is configured.
pub fn load_app_settings_with_floor(
) -> Result<(AppNotificationSettings, Option<FloorSettings>), AppSettingsError> {
    load_app_settings_with_floor_from(configured_app_settings_path()?)
}

pub fn load_app_settings_with_floor_from(
    path: impl AsRef<Path>,
) -> Result<(AppNotificationSettings, Option<FloorSettings>), AppSettingsError> {
    let _guard = settings_io_guard()?;
    let path = path.as_ref();
    let mut local = load_or_create_unlocked(path)?;
    let before = local.clone();
    let floor = sync_floor_unlocked(&mut local)?;
    if floor.is_some() && local != before {
        // Keep local cache aligned with floor for offline display after restarts.
        write_settings_unlocked(path, &local)?;
    }
    Ok((local, floor))
}

/// Load settings, atomically creating and persisting schema-v1 defaults when
/// the file does not yet exist. When a shared folder is set, floor fields overlay
/// the local file (read-only adopt; never seeds).
pub fn load_app_settings_from(
    path: impl AsRef<Path>,
) -> Result<AppNotificationSettings, AppSettingsError> {
    load_app_settings_with_floor_from(path).map(|(settings, _)| settings)
}

pub fn save_app_settings(settings: &AppNotificationSettings) -> Result<(), AppSettingsError> {
    save_app_settings_to(configured_app_settings_path()?, settings)
}

/// Persist a complete settings model. As a credential-safety measure, an empty
/// webhook preserves a non-empty webhook already on disk.
pub fn save_app_settings_to(
    path: impl AsRef<Path>,
    settings: &AppNotificationSettings,
) -> Result<(), AppSettingsError> {
    let _guard = settings_io_guard()?;
    let path = path.as_ref();
    let mut settings = settings.clone();
    if settings.teams_webhook_url.trim().is_empty() && path.is_file() {
        let existing = read_existing_unlocked(path)?;
        if !existing.teams_webhook_url.trim().is_empty() {
            settings.teams_webhook_url = existing.teams_webhook_url;
        }
    }
    validate_settings(&settings)?;
    write_settings_unlocked(path, &settings)
}

pub fn save_app_settings_request(
    request: &SaveAppNotificationSettingsRequest,
) -> Result<AppNotificationSettingsView, AppSettingsError> {
    let path = configured_app_settings_path()?;
    let _guard = settings_io_guard()?;
    let (settings, floor) = save_app_settings_request_unlocked(&path, request)?;
    Ok(settings_view_from(&settings, floor.as_ref()))
}

pub fn save_app_settings_request_to(
    path: impl AsRef<Path>,
    request: &SaveAppNotificationSettingsRequest,
) -> Result<AppNotificationSettings, AppSettingsError> {
    let _guard = settings_io_guard()?;
    save_app_settings_request_unlocked(path.as_ref(), request).map(|(settings, _)| settings)
}

fn save_app_settings_request_unlocked(
    path: &Path,
    request: &SaveAppNotificationSettingsRequest,
) -> Result<(AppNotificationSettings, Option<FloorSettings>), AppSettingsError> {
    let mut settings = load_or_create_unlocked(path)?;

    match request.scope {
        SettingsSaveScope::Connect => save_connect_unlocked(path, &mut settings, request),
        SettingsSaveScope::Local => save_local_unlocked(path, &mut settings, request),
        SettingsSaveScope::Operator => {
            save_scoped_policy_unlocked(path, &mut settings, request, SettingsSaveScope::Operator)
        }
        SettingsSaveScope::Identity => {
            save_scoped_policy_unlocked(path, &mut settings, request, SettingsSaveScope::Identity)
        }
        SettingsSaveScope::Teams => {
            save_scoped_policy_unlocked(path, &mut settings, request, SettingsSaveScope::Teams)
        }
        SettingsSaveScope::Advanced => {
            save_scoped_policy_unlocked(path, &mut settings, request, SettingsSaveScope::Advanced)
        }
    }
}

fn save_connect_unlocked(
    path: &Path,
    settings: &mut AppNotificationSettings,
    request: &SaveAppNotificationSettingsRequest,
) -> Result<(AppNotificationSettings, Option<FloorSettings>), AppSettingsError> {
    apply_station_id_from_request(settings, request)?;
    let new_path = request.shared_shift_log_path.trim().to_string();
    if new_path.is_empty() {
        settings.shared_shift_log_path.clear();
        write_settings_unlocked(path, settings)?;
        return Ok((settings.clone(), None));
    }

    settings.shared_shift_log_path = new_path.clone();

    // Fast path: existing floor → password check + adopt only (no floor rewrite).
    if let Some(floor) = try_load_floor_settings(&new_path)? {
        if request.connect_password != floor.settings_password {
            return Err(AppSettingsError::WrongFloorPassword);
        }
        floor.apply_to_local(settings);
        settings.shared_shift_log_path = new_path;
        write_settings_unlocked(path, settings)?;
        return Ok((settings.clone(), Some(floor)));
    }

    // Missing floor: seed under lock so a concurrent creator is detected.
    // Fold Advanced + Operator form fields into the seed so one Browse+Save after
    // editing names/webhook still publishes the intended policy.
    apply_scope_to_local(settings, request, SettingsSaveScope::Advanced)?;
    apply_scope_to_local(settings, request, SettingsSaveScope::Operator)?;
    let mut seed = FloorSettings::from_local_settings(settings);
    apply_scope_to_floor(&mut seed, request, SettingsSaveScope::Advanced)?;
    apply_scope_to_floor(&mut seed, request, SettingsSaveScope::Operator)?;
    seed.updated_by_station_id = settings.station_id.trim().to_string();
    seed.updated_at = floor_settings_now();

    let mut created_identity: Option<FloorStation> = None;
    let floor = super::floor_settings::update_floor_settings_with_lock(&new_path, |existing| {
        match existing {
            // Race: another station already seeded — keep their floor (adopt under lock).
            Some(existing_floor) => Ok(existing_floor),
            None => {
                let mut next = seed.clone();
                created_identity =
                    apply_catalog_patch(&mut next, &[], request.catalog_create.as_ref()).map_err(
                        |error| super::floor_settings::FloorSettingsError::Write(error.to_string()),
                    )?;
                Ok(next)
            }
        }
    })
    .map_err(AppSettingsError::from)?;

    // Our seed uses the local settings password. A race-adopted floor may differ —
    // then connect_password must match that floor password.
    if floor.settings_password != settings.settings_password
        && request.connect_password != floor.settings_password
    {
        return Err(AppSettingsError::WrongFloorPassword);
    }

    floor.apply_to_local(settings);
    if let Some(created) = &created_identity {
        if request
            .catalog_create
            .as_ref()
            .is_some_and(|create| create.select_for_this_pc)
        {
            settings.station_id = created.id.clone();
            settings.station_name = created.name.clone();
        }
        if created.role == StationRole::Floor {
            let _ = shift_log::ensure_floor_station_directory(&new_path, &created.id);
        }
    }
    settings.shared_shift_log_path = new_path;
    write_settings_unlocked(path, settings)?;
    Ok((settings.clone(), Some(floor)))
}

fn save_local_unlocked(
    path: &Path,
    settings: &mut AppNotificationSettings,
    request: &SaveAppNotificationSettingsRequest,
) -> Result<(AppNotificationSettings, Option<FloorSettings>), AppSettingsError> {
    apply_station_id_from_request(settings, request)?;
    if request.shared_shift_log_path.trim().is_empty() {
        settings.shared_shift_log_path.clear();
    }

    let floor = if settings.shared_shift_log_path.trim().is_empty() {
        None
    } else {
        match try_load_floor_settings(&settings.shared_shift_log_path)? {
            Some(floor) => {
                let station_id = settings.station_id.clone();
                floor.apply_to_local(settings);
                settings.station_id = station_id;
                settings.station_name = floor.station_name_for_id(&settings.station_id);
                Some(floor)
            }
            None => None,
        }
    };

    write_settings_unlocked(path, settings)?;
    Ok((settings.clone(), floor))
}

fn save_scoped_policy_unlocked(
    path: &Path,
    settings: &mut AppNotificationSettings,
    request: &SaveAppNotificationSettingsRequest,
    scope: SettingsSaveScope,
) -> Result<(AppNotificationSettings, Option<FloorSettings>), AppSettingsError> {
    let shared = settings.shared_shift_log_path.trim().to_string();

    if shared.is_empty() {
        if request.catalog_create.is_some() {
            return Err(AppSettingsError::SharedFolderRequiredForIdentity);
        }
        apply_scope_to_local(settings, request, scope)?;
        write_settings_unlocked(path, settings)?;
        return Ok((settings.clone(), None));
    }

    if matches!(
        scope,
        SettingsSaveScope::Identity | SettingsSaveScope::Advanced
    ) {
        apply_station_id_from_request(settings, request)?;
    }

    let station_id = settings.station_id.trim().to_string();
    let mut created_identity: Option<FloorStation> = None;
    let floor = super::floor_settings::update_floor_settings_with_lock(&shared, |existing| {
        let mut floor = existing.ok_or_else(|| {
            super::floor_settings::FloorSettingsError::Read(
                "Floor settings are unavailable; cannot save floor policy until the shared file is readable."
                    .to_string(),
            )
        })?;
        // Map apply errors into FloorSettingsError for the mutator.
        apply_scope_to_floor(&mut floor, request, scope).map_err(|error| {
            super::floor_settings::FloorSettingsError::Write(error.to_string())
        })?;
        if matches!(
            scope,
            SettingsSaveScope::Identity | SettingsSaveScope::Advanced
        ) {
            created_identity = apply_catalog_patch(
                &mut floor,
                &request.stations,
                request.catalog_create.as_ref(),
            )
            .map_err(|error| super::floor_settings::FloorSettingsError::Write(error.to_string()))?;
        }
        floor.updated_by_station_id = station_id.clone();
        floor.updated_at = floor_settings_now();
        Ok(floor)
    })
    .map_err(|error| match error {
        super::floor_settings::FloorSettingsError::Read(message)
            if message.contains("unavailable") =>
        {
            AppSettingsError::Floor(message)
        }
        other => AppSettingsError::from(other),
    })?;

    floor.apply_to_local(settings);
    if let Some(created) = &created_identity {
        if request
            .catalog_create
            .as_ref()
            .is_some_and(|create| create.select_for_this_pc)
        {
            settings.station_id = created.id.clone();
            settings.station_name = created.name.clone();
        }
        if created.role == StationRole::Floor {
            let _ = shift_log::ensure_floor_station_directory(&shared, &created.id);
        }
    }
    write_settings_unlocked(path, settings)?;
    Ok((settings.clone(), Some(floor)))
}

fn apply_station_id_from_request(
    settings: &mut AppNotificationSettings,
    request: &SaveAppNotificationSettingsRequest,
) -> Result<(), AppSettingsError> {
    let station_id = request.station_id.trim();
    if station_id.is_empty() {
        return Err(AppSettingsError::EmptyStationId);
    }
    if !is_known_station_id(station_id)
        && !settings
            .station_catalog
            .iter()
            .any(|entry| entry.id.trim() == station_id)
        && !request
            .stations
            .iter()
            .any(|entry| entry.id.trim() == station_id)
    {
        return Err(AppSettingsError::UnknownStation(station_id.to_string()));
    }
    settings.station_id = station_id.to_string();

    // Prefer catalog rename for this id when Advanced provides stations.
    let renamed = request
        .stations
        .iter()
        .find(|entry| entry.id.trim() == station_id)
        .map(|entry| entry.name.trim().to_string())
        .filter(|name| !name.is_empty());
    if let Some(name) = renamed {
        settings.station_name = name;
    } else if !request.station_name.trim().is_empty() {
        settings.station_name = request.station_name.trim().to_string();
    } else if let Some(entry) = settings
        .station_catalog
        .iter()
        .find(|entry| entry.id.trim() == station_id)
    {
        let name = entry.name.trim();
        if !name.is_empty() {
            settings.station_name = name.to_string();
        } else {
            settings.station_name = station_name_for_id(station_id).to_string();
        }
    } else {
        settings.station_name = station_name_for_id(station_id).to_string();
    }
    Ok(())
}

fn apply_scope_to_local(
    settings: &mut AppNotificationSettings,
    request: &SaveAppNotificationSettingsRequest,
    scope: SettingsSaveScope,
) -> Result<(), AppSettingsError> {
    match scope {
        SettingsSaveScope::Operator => {
            settings.shifts = normalize_shifts(&request.shifts)?;
            settings.events.summary = request.events.summary;
            let poster = request.summary_poster_station_id.trim();
            settings.summary_poster_station_id = if poster.is_empty() {
                default_summary_poster()
            } else if is_floor_station_in_local(settings, poster) {
                poster.to_string()
            } else {
                return Err(AppSettingsError::UnknownStation(poster.to_string()));
            };
            settings.summary_included_station_ids = normalize_included_station_ids_local(
                &request.summary_included_station_ids,
                settings,
            )?;
        }
        SettingsSaveScope::Identity => {
            apply_station_id_from_request(settings, request)?;
            apply_station_names_to_catalog(settings, &request.stations)?;
        }
        SettingsSaveScope::Teams => apply_teams_scope_to_local(settings, request),
        SettingsSaveScope::Advanced => {
            apply_teams_scope_to_local(settings, request);
            apply_station_id_from_request(settings, request)?;
            apply_station_names_to_catalog(settings, &request.stations)?;
        }
        SettingsSaveScope::Connect | SettingsSaveScope::Local => {}
    }
    validate_settings(settings)?;
    Ok(())
}

fn apply_scope_to_floor(
    floor: &mut FloorSettings,
    request: &SaveAppNotificationSettingsRequest,
    scope: SettingsSaveScope,
) -> Result<(), AppSettingsError> {
    match scope {
        SettingsSaveScope::Operator => {
            floor.shifts = normalize_shifts(&request.shifts)?;
            floor.events.summary = request.events.summary;
            let poster = request.summary_poster_station_id.trim();
            floor.summary_poster_station_id = if poster.is_empty() {
                default_summary_poster()
            } else if floor.is_floor_identity(poster) {
                poster.to_string()
            } else {
                return Err(AppSettingsError::UnknownStation(poster.to_string()));
            };
            floor.summary_included_station_ids =
                normalize_included_station_ids_floor(&request.summary_included_station_ids, floor)?;
        }
        SettingsSaveScope::Identity => {}
        SettingsSaveScope::Teams | SettingsSaveScope::Advanced => {
            apply_teams_scope_to_floor(floor, request)
        }
        SettingsSaveScope::Connect | SettingsSaveScope::Local => {}
    }
    super::floor_settings::validate_floor_settings(floor)?;
    Ok(())
}

fn apply_teams_scope_to_local(
    settings: &mut AppNotificationSettings,
    request: &SaveAppNotificationSettingsRequest,
) {
    settings.enabled = request.enabled;
    settings.teams_destination_name = match request.teams_destination_name.trim() {
        "" => default_destination_name(),
        value => value.to_string(),
    };
    apply_webhook_to_string(&mut settings.teams_webhook_url, request);
    settings.idle_timeout_minutes = request.idle_timeout_minutes;
    settings.events.problem = request.events.problem;
    settings.events.complete = request.events.complete;
    settings.events.changeover = request.events.changeover;
    settings.events.stuck = request.events.stuck;
}

fn apply_teams_scope_to_floor(
    floor: &mut FloorSettings,
    request: &SaveAppNotificationSettingsRequest,
) {
    floor.enabled = request.enabled;
    floor.teams_destination_name = match request.teams_destination_name.trim() {
        "" => default_destination_name(),
        value => value.to_string(),
    };
    apply_webhook_to_string(&mut floor.teams_webhook_url, request);
    floor.idle_timeout_minutes = request.idle_timeout_minutes;
    floor.events.problem = request.events.problem;
    floor.events.complete = request.events.complete;
    floor.events.changeover = request.events.changeover;
    floor.events.stuck = request.events.stuck;
}

fn apply_webhook_to_string(target: &mut String, request: &SaveAppNotificationSettingsRequest) {
    if request.clear_webhook {
        target.clear();
    } else if !request.teams_webhook_url.trim().is_empty() {
        *target = request.teams_webhook_url.trim().to_string();
    }
}

fn apply_station_names_to_floor(
    floor: &mut FloorSettings,
    stations: &[StationCatalogEntry],
) -> Result<(), AppSettingsError> {
    if stations.is_empty() {
        return Ok(());
    }
    for entry in stations {
        let id = entry.id.trim();
        if floor.station(id).is_none() {
            return Err(AppSettingsError::UnknownStation(id.to_string()));
        }
        let name = entry.name.trim();
        // Explicit blank for a known id being set → reject (no silent default).
        if name.is_empty() {
            return Err(FloorSettingsError::EmptyStationName.into());
        }
        validate_display_name(name)?;
        let slot = floor
            .stations
            .iter_mut()
            .find(|station| station.id == id)
            .expect("validated floor station exists");
        slot.name = name.to_string();
    }
    Ok(())
}

fn apply_catalog_patch(
    floor: &mut FloorSettings,
    renames: &[StationCatalogEntry],
    create: Option<&CatalogCreateRequest>,
) -> Result<Option<FloorStation>, AppSettingsError> {
    apply_station_names_to_floor(floor, renames)?;

    let created = if let Some(create) = create {
        let name = create.name.trim();
        validate_display_name(name)?;
        let millis = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let station = FloorStation {
            id: generate_identity_id(&floor.stations, millis),
            name: name.to_string(),
            role: create.role,
        };
        floor.schema_version = FLOOR_SETTINGS_SCHEMA_VERSION;
        floor.stations.push(station.clone());
        Some(station)
    } else {
        None
    };

    super::floor_settings::validate_floor_settings(floor)?;
    Ok(created)
}

fn apply_station_names_to_catalog(
    settings: &mut AppNotificationSettings,
    stations: &[StationCatalogEntry],
) -> Result<(), AppSettingsError> {
    if stations.is_empty() {
        return Ok(());
    }
    if settings.station_catalog.is_empty() {
        settings.station_catalog = catalog_from_local(settings);
    }
    for entry in stations {
        let id = entry.id.trim();
        if !is_known_station_id(id)
            && !settings
                .station_catalog
                .iter()
                .any(|existing| existing.id.trim() == id)
        {
            return Err(AppSettingsError::UnknownStation(id.to_string()));
        }
        let name = entry.name.trim();
        if name.is_empty() {
            return Err(FloorSettingsError::EmptyStationName.into());
        }
        validate_display_name(name)?;
        if let Some(slot) = settings
            .station_catalog
            .iter_mut()
            .find(|s| s.id.trim() == id)
        {
            slot.name = name.to_string();
        } else {
            settings.station_catalog.push(StationCatalogEntry {
                id: id.to_string(),
                name: name.to_string(),
                role: entry.role,
            });
        }
        if id == settings.station_id.trim() {
            settings.station_name = name.to_string();
        }
    }
    // Reject duplicates in local catalog the same way floor does.
    let mut seen: Vec<String> = Vec::new();
    for entry in &settings.station_catalog {
        let key = entry.name.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        if seen.iter().any(|s| s == &key) {
            return Err(
                FloorSettingsError::DuplicateStationName(entry.name.trim().to_string()).into(),
            );
        }
        seen.push(key);
    }
    Ok(())
}

pub fn verify_password(settings: &AppNotificationSettings, attempt: &str) -> bool {
    settings.settings_password == attempt
}

pub fn verify_settings_password(attempt: &str) -> Result<bool, AppSettingsError> {
    load_app_settings().map(|settings| verify_password(&settings, attempt))
}

pub fn change_password(
    settings: &mut AppNotificationSettings,
    current_password: &str,
    new_password: &str,
) -> Result<(), AppSettingsError> {
    if !verify_password(settings, current_password) {
        return Err(AppSettingsError::WrongPassword);
    }
    let new_password = new_password.trim();
    if new_password.is_empty() {
        return Err(AppSettingsError::EmptyPassword);
    }
    settings.settings_password = new_password.to_string();
    Ok(())
}

pub fn change_settings_password(
    request: &ChangeSettingsPasswordRequest,
) -> Result<(), AppSettingsError> {
    change_settings_password_at(configured_app_settings_path()?, request)
}

pub fn change_settings_password_at(
    path: impl AsRef<Path>,
    request: &ChangeSettingsPasswordRequest,
) -> Result<(), AppSettingsError> {
    let new_password = request.new_password.trim();
    if new_password != request.confirm_password.trim() {
        return Err(AppSettingsError::PasswordMismatch);
    }
    let _guard = settings_io_guard()?;
    let path = path.as_ref();
    let mut settings = load_or_create_unlocked(path)?;
    let shared = settings.shared_shift_log_path.trim().to_string();
    if !shared.is_empty() {
        let station_id = settings.station_id.trim().to_string();
        let mut wrong_password = false;
        let floor = super::floor_settings::update_floor_settings_with_lock(&shared, |current| {
            let mut floor = current.ok_or_else(|| {
                FloorSettingsError::Read(
                    "Floor settings file is missing; cannot change password while a shared folder is configured."
                        .to_string(),
                )
            })?;
            if floor.settings_password != request.current_password {
                wrong_password = true;
                return Err(FloorSettingsError::Write(
                    "current floor password is incorrect".to_string(),
                ));
            }
            floor.settings_password = new_password.to_string();
            floor.updated_by_station_id = station_id;
            floor.updated_at = floor_settings_now();
            Ok(floor)
        })
        .map_err(|error| {
            if wrong_password {
                AppSettingsError::WrongPassword
            } else {
                AppSettingsError::from(error)
            }
        })?;
        floor.apply_to_local(&mut settings);
    } else {
        change_password(&mut settings, &request.current_password, new_password)?;
    }
    write_settings_unlocked(path, &settings)
}

pub fn load_runtime_resolved_config() -> Result<ResolvedConfig, AppSettingsError> {
    load_app_settings().map(|settings| settings.to_resolved_config())
}

impl AppNotificationSettings {
    pub fn to_resolved_config(&self) -> ResolvedConfig {
        let path = app_settings_path();
        let station_id = self.station_id.trim();
        let station_name = self.station_name.trim();
        ResolvedConfig {
            enabled: self.enabled,
            teams_destination_name: match self.teams_destination_name.trim() {
                "" => default_destination_name(),
                value => value.to_string(),
            },
            teams_webhook_url: self.teams_webhook_url.trim().to_string(),
            station_id: station_id.to_string(),
            station_name: if station_name.is_empty() {
                station_name_for_id(station_id).to_string()
            } else {
                station_name.to_string()
            },
            idle_timeout_minutes: self.idle_timeout_minutes,
            events: self.events.clone(),
            summary_schedule_times: Vec::new(),
            shared_shift_log_path: self.shared_shift_log_path.trim().to_string(),
            settings_path: path.clone(),
            station_path: path,
        }
    }
}

/// Load floor when path is set; never seeds. Soft-fails overlay so automation still works.
fn sync_floor_unlocked(
    local: &mut AppNotificationSettings,
) -> Result<Option<FloorSettings>, AppSettingsError> {
    let shared = local.shared_shift_log_path.trim();
    if shared.is_empty() {
        return Ok(None);
    }
    match try_load_floor_settings(shared) {
        Ok(Some(floor)) => {
            floor.apply_to_local(local);
            Ok(Some(floor))
        }
        Ok(None) => {
            // Previously configured but file missing (OneDrive lag, etc.): keep cache.
            Ok(None)
        }
        Err(_error) => {
            // Soft-fail overlay: keep local cache so automation/settings still work.
            Ok(None)
        }
    }
}

fn floor_settings_now() -> String {
    chrono::Local::now().to_rfc3339()
}

fn normalize_included_station_ids_local(
    ids: &[String],
    settings: &AppNotificationSettings,
) -> Result<Vec<String>, AppSettingsError> {
    let defaults = catalog_from_local(settings)
        .into_iter()
        .filter(|entry| entry.role == StationRole::Floor)
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    normalize_included_station_ids(ids, &defaults)
}

fn normalize_included_station_ids_floor(
    ids: &[String],
    floor: &FloorSettings,
) -> Result<Vec<String>, AppSettingsError> {
    let defaults = floor
        .floor_stations()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    normalize_included_station_ids(ids, &defaults)
}

fn normalize_included_station_ids(
    ids: &[String],
    allowed: &[String],
) -> Result<Vec<String>, AppSettingsError> {
    if ids.is_empty() {
        return Ok(allowed.to_vec());
    }
    let mut out = Vec::new();
    for id in ids {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        if !allowed.iter().any(|allowed_id| allowed_id == id) {
            return Err(AppSettingsError::UnknownStation(id.to_string()));
        }
        if !out.iter().any(|existing| existing == id) {
            out.push(id.to_string());
        }
    }
    if out.is_empty() {
        return Err(AppSettingsError::InvalidShift(
            "select at least one station for the end-of-shift summary".to_string(),
        ));
    }
    Ok(out)
}

fn is_floor_station_in_local(settings: &AppNotificationSettings, station_id: &str) -> bool {
    catalog_from_local(settings)
        .iter()
        .any(|entry| entry.id == station_id && entry.role == StationRole::Floor)
}

fn normalize_shifts(shifts: &[ShiftWindow]) -> Result<Vec<ShiftWindow>, AppSettingsError> {
    if shifts.len() > 2 {
        return Err(AppSettingsError::InvalidShift(
            "at most two shift windows are supported".to_string(),
        ));
    }
    let mut normalized = Vec::new();
    for shift in shifts {
        let label = shift.label.trim().to_string();
        let start_time = shift.start_time.trim().to_string();
        let end_time = shift.end_time.trim().to_string();
        if label.is_empty() && start_time.is_empty() && end_time.is_empty() {
            continue;
        }
        if label.is_empty() {
            return Err(AppSettingsError::InvalidShift(
                "each shift needs a label".to_string(),
            ));
        }
        validate_hhmm(&start_time)?;
        validate_hhmm(&end_time)?;
        if start_time == end_time {
            return Err(AppSettingsError::InvalidShift(format!(
                "shift '{label}' start and end must differ"
            )));
        }
        normalized.push(ShiftWindow {
            label,
            start_time,
            end_time,
        });
    }
    Ok(normalized)
}

fn validate_hhmm(value: &str) -> Result<(), AppSettingsError> {
    let bytes = value.as_bytes();
    if bytes.len() != 5 || bytes[2] != b':' {
        return Err(AppSettingsError::InvalidShift(format!(
            "time '{value}' must be HH:MM"
        )));
    }
    let hour: u32 = value[..2]
        .parse()
        .map_err(|_| AppSettingsError::InvalidShift(format!("time '{value}' must be HH:MM")))?;
    let minute: u32 = value[3..]
        .parse()
        .map_err(|_| AppSettingsError::InvalidShift(format!("time '{value}' must be HH:MM")))?;
    if hour > 23 || minute > 59 {
        return Err(AppSettingsError::InvalidShift(format!(
            "time '{value}' is out of range"
        )));
    }
    Ok(())
}

/// Create the shared OneDrive/network folder layout when a path is configured.
/// Empty path is a successful no-op.
pub fn ensure_configured_shared_layout(
    settings: &AppNotificationSettings,
) -> Result<(), shift_log::ShiftLogError> {
    let path = settings.shared_shift_log_path.trim();
    if path.is_empty() {
        return Ok(());
    }
    shift_log::ensure_shared_root_layout(path).map(|_| ())
}

fn load_or_create_unlocked(path: &Path) -> Result<AppNotificationSettings, AppSettingsError> {
    match fs::read_to_string(path) {
        Ok(raw) => parse_settings(path, &raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(settings) = recover_interrupted_write(path)? {
                return Ok(settings);
            }
            let settings = AppNotificationSettings::default();
            write_settings_unlocked(path, &settings)?;
            Ok(settings)
        }
        Err(error) => Err(AppSettingsError::Read(format!(
            "{}: {error}",
            path.display()
        ))),
    }
}

fn configured_app_settings_path() -> Result<PathBuf, AppSettingsError> {
    CONFIG_DIR
        .read()
        .ok()
        .and_then(|configured| configured.clone())
        .map(|directory| directory.join(SETTINGS_FILE_NAME))
        .ok_or(AppSettingsError::ConfigDirectoryUnavailable)
}

/// Recover the two crash windows in the Windows-compatible backup dance:
/// a fully synced temp file is the intended new value, while a backup is the
/// last committed value. Invalid artifacts are preserved for diagnosis rather
/// than silently replacing credentials with factory defaults.
fn recover_interrupted_write(
    path: &Path,
) -> Result<Option<AppNotificationSettings>, AppSettingsError> {
    let mut temp_candidates = sibling_work_candidates(path, "tmp")?;
    let mut backup_candidates = sibling_work_candidates(path, "bak")?;
    let artifact_count = temp_candidates.len() + backup_candidates.len();
    if artifact_count == 0 {
        return Ok(None);
    }

    sort_newest_first(&mut temp_candidates);
    sort_newest_first(&mut backup_candidates);
    let all_candidates = temp_candidates
        .iter()
        .chain(backup_candidates.iter())
        .cloned()
        .collect::<Vec<_>>();

    for candidate in temp_candidates.into_iter().chain(backup_candidates) {
        let Ok(raw) = fs::read_to_string(&candidate) else {
            continue;
        };
        let Ok(settings) = parse_settings(&candidate, &raw) else {
            continue;
        };

        if let Err(error) = fs::rename(&candidate, path) {
            // Another app instance may have completed recovery first.
            if path.is_file() {
                return read_existing_unlocked(path).map(Some);
            }
            return Err(AppSettingsError::Recovery(format!(
                "{} could not be restored to {}: {error}",
                candidate.display(),
                path.display()
            )));
        }
        for artifact in &all_candidates {
            if artifact != &candidate {
                let _ = fs::remove_file(artifact);
            }
        }
        return Ok(Some(settings));
    }

    Err(AppSettingsError::Recovery(format!(
        "found {artifact_count} temp/backup artifact(s), but none contained valid schema-v1 settings"
    )))
}

fn sibling_work_candidates(path: &Path, suffix: &str) -> Result<Vec<PathBuf>, AppSettingsError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(AppSettingsError::Recovery(format!(
                "{} could not be inspected: {error}",
                parent.display()
            )))
        }
    };
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(SETTINGS_FILE_NAME);
    let prefix = format!(".{file_name}.");
    let suffix = format!(".{suffix}");

    Ok(entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|candidate| {
            candidate
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(&suffix))
        })
        .collect())
}

fn sort_newest_first(paths: &mut [PathBuf]) {
    paths.sort_by_key(|path| {
        std::cmp::Reverse(
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH),
        )
    });
}

fn read_existing_unlocked(path: &Path) -> Result<AppNotificationSettings, AppSettingsError> {
    let raw = fs::read_to_string(path)
        .map_err(|error| AppSettingsError::Read(format!("{}: {error}", path.display())))?;
    parse_settings(path, &raw)
}

fn parse_settings(path: &Path, raw: &str) -> Result<AppNotificationSettings, AppSettingsError> {
    let settings: AppNotificationSettings = serde_json::from_str(raw)
        .map_err(|error| AppSettingsError::Parse(format!("{}: {error}", path.display())))?;
    validate_settings(&settings)?;
    Ok(settings)
}

fn validate_settings(settings: &AppNotificationSettings) -> Result<(), AppSettingsError> {
    if settings.schema_version != APP_SETTINGS_SCHEMA_VERSION {
        return Err(AppSettingsError::UnsupportedSchema(settings.schema_version));
    }
    if settings.settings_password.trim().is_empty() {
        return Err(AppSettingsError::EmptyPassword);
    }
    if settings.station_id.trim().is_empty() {
        return Err(AppSettingsError::EmptyStationId);
    }
    Ok(())
}

fn write_settings_unlocked(
    path: &Path,
    settings: &AppNotificationSettings,
) -> Result<(), AppSettingsError> {
    validate_settings(settings)?;
    ensure_parent_directory(path).map_err(AppSettingsError::Write)?;
    let raw = serde_json::to_vec_pretty(settings)
        .map_err(|error| AppSettingsError::Write(error.to_string()))?;
    atomic_replace(path, &raw).map_err(AppSettingsError::Write)
}

fn settings_io_guard() -> Result<std::sync::MutexGuard<'static, ()>, AppSettingsError> {
    SETTINGS_IO_LOCK.lock().map_err(|_| AppSettingsError::Busy)
}

fn ensure_parent_directory(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) else {
        return Ok(());
    };
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))
}

fn atomic_replace(path: &Path, contents: &[u8]) -> Result<(), String> {
    let temp_path = sibling_work_path(path, "tmp");
    let backup_path = sibling_work_path(path, "bak");
    let mut temp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| format!("{}: {error}", temp_path.display()))?;

    if let Err(error) = temp_file
        .write_all(contents)
        .and_then(|_| temp_file.sync_all())
    {
        drop(temp_file);
        let _ = fs::remove_file(&temp_path);
        return Err(format!("{}: {error}", temp_path.display()));
    }
    drop(temp_file);

    if !path.exists() {
        return fs::rename(&temp_path, path).map_err(|error| {
            let _ = fs::remove_file(&temp_path);
            format!("{}: {error}", path.display())
        });
    }

    if let Err(error) = fs::rename(path, &backup_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("{}: {error}", path.display()));
    }

    match fs::rename(&temp_path, path) {
        Ok(()) => {
            let _ = fs::remove_file(&backup_path);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(&backup_path, path);
            let _ = fs::remove_file(&temp_path);
            Err(format!("{}: {error}", path.display()))
        }
    }
}

fn sibling_work_path(path: &Path, suffix: &str) -> PathBuf {
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(SETTINGS_FILE_NAME);
    path.with_file_name(format!(
        ".{file_name}.{}.{}.{}",
        std::process::id(),
        counter,
        suffix
    ))
}

fn redacted_or_empty(value: &str) -> &'static str {
    if value.trim().is_empty() {
        "<empty>"
    } else {
        "<redacted>"
    }
}

fn default_schema_version() -> u32 {
    APP_SETTINGS_SCHEMA_VERSION
}

fn default_password() -> String {
    DEFAULT_SETTINGS_PASSWORD.to_string()
}

fn default_true() -> bool {
    true
}

fn default_destination_name() -> String {
    "PDU Testing".to_string()
}

fn default_station_id() -> String {
    "test-station-1".to_string()
}

fn default_station_name() -> String {
    "Test Station 1".to_string()
}

fn default_idle_timeout() -> u32 {
    30
}

fn default_app_events() -> EventToggles {
    EventToggles {
        problem: true,
        complete: true,
        changeover: true,
        stuck: false,
        summary: true,
    }
}

fn default_summary_poster() -> String {
    DEFAULT_SUMMARY_POSTER_STATION_ID.to_string()
}

fn default_summary_included() -> Vec<String> {
    super::stations::known_station_ids()
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
fn default_stations_catalog() -> Vec<StationCatalogEntry> {
    known_stations_owned()
        .into_iter()
        .map(|(id, name)| StationCatalogEntry {
            id,
            name,
            role: StationRole::Floor,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::can_send;
    use tempfile::tempdir;

    fn base_request() -> SaveAppNotificationSettingsRequest {
        SaveAppNotificationSettingsRequest {
            enabled: true,
            teams_destination_name: "PDU Testing".to_string(),
            teams_webhook_url: String::new(),
            clear_webhook: false,
            station_id: "test-station-3".to_string(),
            station_name: String::new(),
            idle_timeout_minutes: 45,
            events: EventToggles {
                problem: true,
                complete: false,
                changeover: true,
                stuck: false,
                summary: true,
            },
            shared_shift_log_path: String::new(),
            shifts: vec![ShiftWindow {
                label: "Day".to_string(),
                start_time: "06:00".to_string(),
                end_time: "15:00".to_string(),
            }],
            summary_poster_station_id: "pdu-lab".to_string(),
            summary_included_station_ids: default_summary_included(),
            stations: Vec::new(),
            catalog_create: None,
            scope: SettingsSaveScope::Operator,
            connect_password: String::new(),
        }
    }

    fn connect_request(shared: &str, station_id: &str) -> SaveAppNotificationSettingsRequest {
        let mut req = base_request();
        req.scope = SettingsSaveScope::Connect;
        req.shared_shift_log_path = shared.to_string();
        req.station_id = station_id.to_string();
        req.connect_password = DEFAULT_SETTINGS_PASSWORD.to_string();
        req
    }

    fn advanced_request() -> SaveAppNotificationSettingsRequest {
        let mut req = base_request();
        req.scope = SettingsSaveScope::Advanced;
        req
    }

    fn operator_request() -> SaveAppNotificationSettingsRequest {
        let mut req = base_request();
        req.scope = SettingsSaveScope::Operator;
        req
    }

    fn frontend_shaped_default_stations() -> Vec<StationCatalogEntry> {
        default_stations_catalog()
    }

    #[test]
    fn missing_file_creates_persisted_schema_v1_defaults() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nested/notification_settings.json");
        let settings = load_app_settings_from(&path).unwrap();

        assert!(path.is_file(), "first load must persist defaults");
        assert_eq!(settings.schema_version, APP_SETTINGS_SCHEMA_VERSION);
        assert_eq!(settings.settings_password, DEFAULT_SETTINGS_PASSWORD);
        assert_eq!(settings.station_id, "test-station-1");
        assert_eq!(settings.station_name, "Test Station 1");
        assert!(settings.enabled);
        assert!(settings.teams_webhook_url.is_empty());
        assert!(settings.events.problem);
        assert!(settings.events.complete);
        assert!(!settings.events.stuck);
        assert!(settings.events.summary);
        assert_eq!(settings.summary_poster_station_id, "pdu-lab");

        assert_eq!(load_app_settings_from(&path).unwrap(), settings);
    }

    #[test]
    fn save_round_trip_redacts_password_and_signed_webhook() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(SETTINGS_FILE_NAME);
        let mut settings = AppNotificationSettings::default();
        settings.settings_password = "9876".to_string();
        settings.teams_webhook_url = "https://example.invalid/workflow?sig=TOP_SECRET".to_string();
        settings.station_id = "test-station-3".to_string();
        settings.station_name = "Test Station 3".to_string();
        settings.shared_shift_log_path = String::new();

        save_app_settings_to(&path, &settings).unwrap();
        let loaded = load_app_settings_from(&path).unwrap();
        assert_eq!(loaded, settings);

        let debug = format!("{loaded:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("TOP_SECRET"));
        assert!(!debug.contains("9876"));
        assert!(!debug.contains("example.invalid"));

        let view = AppNotificationSettingsView::from(&loaded);
        assert!(view.webhook_configured);
        assert!(view.teams_webhook_url.is_empty());
    }

    #[test]
    fn empty_webhook_on_raw_save_preserves_existing_credential() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(SETTINGS_FILE_NAME);
        let mut settings = AppNotificationSettings::default();
        settings.teams_webhook_url = "https://example.invalid/hook?sig=KEEP_ME".to_string();
        save_app_settings_to(&path, &settings).unwrap();

        settings.teams_webhook_url.clear();
        settings.station_id = "test-station-3".to_string();
        save_app_settings_to(&path, &settings).unwrap();

        let loaded = load_app_settings_from(&path).unwrap();
        assert_eq!(loaded.station_id, "test-station-3");
        assert_eq!(
            loaded.teams_webhook_url,
            "https://example.invalid/hook?sig=KEEP_ME"
        );
    }

    #[test]
    fn safe_request_preserves_or_explicitly_clears_webhook_and_password() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(SETTINGS_FILE_NAME);
        let shared = directory.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        let shared_path = shared.to_string_lossy().to_string();
        let mut initial = AppNotificationSettings::default();
        initial.settings_password = "2468".to_string();
        initial.teams_webhook_url = "https://example.invalid/hook?sig=KEEP_ME".to_string();
        save_app_settings_to(&path, &initial).unwrap();

        // Connect seeds floor from local (webhook + password).
        let mut connect = connect_request(&shared_path, "test-station-3");
        connect.connect_password = "2468".to_string();
        // Floor was just seeded with password 2468 — but connect when missing seeds
        // without password check. Password only required when floor exists.
        // First connect: file absent → seed.
        connect.connect_password.clear();
        let updated = save_app_settings_request_to(&path, &connect).unwrap();
        assert_eq!(updated.settings_password, "2468");
        assert_eq!(updated.station_name, "Test Station 3");
        assert_eq!(updated.teams_webhook_url, initial.teams_webhook_url);
        assert_eq!(updated.shared_shift_log_path, shared_path);
        assert!(shared
            .join(super::super::floor_settings::FLOOR_SETTINGS_FILE_NAME)
            .is_file());

        let mut clear = advanced_request();
        clear.station_id = "test-station-3".to_string();
        clear.clear_webhook = true;
        let cleared = save_app_settings_request_to(&path, &clear).unwrap();
        assert!(cleared.teams_webhook_url.is_empty());
        assert_eq!(cleared.settings_password, "2468");
    }

    #[test]
    fn adoption_with_full_frontend_shaped_catalog_does_not_overwrite_renames() {
        let directory = tempdir().unwrap();
        let shared = directory.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        let shared_path = shared.to_string_lossy().to_string();

        let path_a = directory.path().join("a").join(SETTINGS_FILE_NAME);
        let path_b = directory.path().join("b").join(SETTINGS_FILE_NAME);

        // PC A: Advanced renames locally, then Connect seeds floor.
        let mut advanced_a = advanced_request();
        advanced_a.station_id = "test-station-1".to_string();
        advanced_a.teams_webhook_url = "https://floor.example/hook".to_string();
        advanced_a.stations = vec![StationCatalogEntry {
            id: "test-station-3".to_string(),
            name: "Bay Three".to_string(),
            role: StationRole::Floor,
        }];
        save_app_settings_request_to(&path_a, &advanced_a).unwrap();
        save_app_settings_request_to(&path_a, &connect_request(&shared_path, "test-station-1"))
            .unwrap();

        // PC B: Connect with full default frontend-shaped catalog — must adopt only.
        let mut req_b = connect_request(&shared_path, "pdu-lab");
        req_b.teams_webhook_url = String::new();
        req_b.stations = frontend_shaped_default_stations();
        req_b.shifts = vec![ShiftWindow {
            label: "Stale".to_string(),
            start_time: "00:00".to_string(),
            end_time: "01:00".to_string(),
        }];
        req_b.summary_poster_station_id = "test-station-1".to_string();
        let loaded_b = save_app_settings_request_to(&path_b, &req_b).unwrap();

        assert_eq!(loaded_b.station_id, "pdu-lab");
        assert_eq!(loaded_b.teams_webhook_url, "https://floor.example/hook");
        assert_eq!(loaded_b.station_name, "PDU Lab");
        assert!(
            loaded_b
                .station_catalog
                .iter()
                .any(|s| s.id == "test-station-3" && s.name == "Bay Three"),
            "catalog cache must show peer rename: {:?}",
            loaded_b.station_catalog
        );

        let (merged, floor) = load_app_settings_with_floor_from(&path_b).unwrap();
        let floor = floor.expect("floor present");
        assert_eq!(floor.station_name_for_id("test-station-3"), "Bay Three");
        assert_eq!(merged.teams_webhook_url, "https://floor.example/hook");
        // Floor file must not have been rewritten with B's defaults.
        assert_eq!(floor.station_name_for_id("test-station-3"), "Bay Three");
    }

    #[test]
    fn operator_scoped_save_does_not_clobber_station_names() {
        let directory = tempdir().unwrap();
        let shared = directory.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        let shared_path = shared.to_string_lossy().to_string();
        let path = directory.path().join(SETTINGS_FILE_NAME);

        let mut advanced = advanced_request();
        advanced.station_id = "test-station-1".to_string();
        advanced.stations = vec![StationCatalogEntry {
            id: "test-station-1".to_string(),
            name: "Bay One".to_string(),
            role: StationRole::Floor,
        }];
        save_app_settings_request_to(&path, &advanced).unwrap();
        save_app_settings_request_to(&path, &connect_request(&shared_path, "test-station-1"))
            .unwrap();

        // Operator save with default stations in request must not rewrite names.
        let mut op = operator_request();
        op.station_id = "test-station-1".to_string();
        op.stations = frontend_shaped_default_stations();
        op.summary_poster_station_id = "test-station-3".to_string();
        op.shifts = vec![ShiftWindow {
            label: "Night".to_string(),
            start_time: "15:00".to_string(),
            end_time: "23:00".to_string(),
        }];
        let saved = save_app_settings_request_to(&path, &op).unwrap();
        assert_eq!(saved.summary_poster_station_id, "test-station-3");
        assert_eq!(saved.shifts[0].label, "Night");
        assert_eq!(saved.station_name, "Bay One");

        let floor = try_load_floor_settings(&shared_path)
            .unwrap()
            .expect("floor");
        assert_eq!(floor.station_name_for_id("test-station-1"), "Bay One");
        assert_eq!(floor.summary_poster_station_id, "test-station-3");
    }

    #[test]
    fn teams_scoped_save_preserves_identity_catalog_and_local_station() {
        let directory = tempdir().unwrap();
        let shared = directory.path().join("shared");
        let shared_path = shared.to_string_lossy().to_string();
        let path = directory.path().join(SETTINGS_FILE_NAME);

        save_app_settings_request_to(&path, &connect_request(&shared_path, "test-station-1"))
            .unwrap();
        let before = load_app_settings_from(&path).unwrap();

        let mut request = advanced_request();
        request.scope = SettingsSaveScope::Teams;
        request.station_id = "pdu-lab".to_string();
        request.stations = vec![StationCatalogEntry {
            id: "test-station-1".to_string(),
            name: "STALE NAME".to_string(),
            role: StationRole::Floor,
        }];
        request.teams_destination_name = "New Teams Destination".to_string();

        let saved = save_app_settings_request_to(&path, &request).unwrap();
        let floor = try_load_floor_settings(&shared_path).unwrap().unwrap();

        assert_eq!(saved.station_id, before.station_id);
        assert_eq!(
            floor.station_name_for_id("test-station-1"),
            "Test Station 1"
        );
        assert_eq!(floor.teams_destination_name, "New Teams Destination");
    }

    #[test]
    fn identity_scoped_save_preserves_teams_policy() {
        let directory = tempdir().unwrap();
        let shared = directory.path().join("shared");
        let shared_path = shared.to_string_lossy().to_string();
        let path = directory.path().join(SETTINGS_FILE_NAME);

        save_app_settings_request_to(&path, &connect_request(&shared_path, "test-station-1"))
            .unwrap();
        let before_floor = try_load_floor_settings(&shared_path).unwrap().unwrap();

        let mut request = advanced_request();
        request.scope = SettingsSaveScope::Identity;
        request.station_id = "pdu-lab".to_string();
        request.stations = vec![StationCatalogEntry {
            id: "test-station-1".to_string(),
            name: "Bay One".to_string(),
            role: StationRole::Floor,
        }];
        request.enabled = !before_floor.enabled;
        request.teams_destination_name = "STALE DESTINATION".to_string();

        let saved = save_app_settings_request_to(&path, &request).unwrap();
        let floor = try_load_floor_settings(&shared_path).unwrap().unwrap();

        assert_eq!(saved.station_id, "pdu-lab");
        assert_eq!(floor.station_name_for_id("test-station-1"), "Bay One");
        assert_eq!(floor.enabled, before_floor.enabled);
        assert_eq!(
            floor.teams_destination_name,
            before_floor.teams_destination_name
        );
    }

    #[test]
    fn missing_floor_after_connect_does_not_reseed_on_load() {
        let directory = tempdir().unwrap();
        let shared = directory.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        let shared_path = shared.to_string_lossy().to_string();
        let path = directory.path().join(SETTINGS_FILE_NAME);

        let mut advanced = advanced_request();
        advanced.station_id = "test-station-1".to_string();
        advanced.teams_webhook_url = "https://keep.example/hook".to_string();
        advanced.stations = vec![StationCatalogEntry {
            id: "test-station-1".to_string(),
            name: "Cached Bay".to_string(),
            role: StationRole::Floor,
        }];
        save_app_settings_request_to(&path, &advanced).unwrap();
        save_app_settings_request_to(&path, &connect_request(&shared_path, "test-station-1"))
            .unwrap();

        let floor_path = shared.join(super::super::floor_settings::FLOOR_SETTINGS_FILE_NAME);
        assert!(floor_path.is_file());
        fs::remove_file(&floor_path).unwrap();

        let loaded = load_app_settings_from(&path).unwrap();
        assert!(!floor_path.exists(), "load must not reseed floor file");
        assert_eq!(loaded.teams_webhook_url, "https://keep.example/hook");
        assert_eq!(loaded.station_name, "Cached Bay");
        assert!(
            loaded
                .station_catalog
                .iter()
                .any(|s| s.id == "test-station-1" && s.name == "Cached Bay"),
            "local catalog cache retained: {:?}",
            loaded.station_catalog
        );
        let view = AppNotificationSettingsView::from_merged(&loaded, None);
        assert_eq!(view.floor_sync.source, "local-cache");
    }

    #[test]
    fn connect_requires_password_when_floor_exists() {
        let directory = tempdir().unwrap();
        let shared = directory.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        let shared_path = shared.to_string_lossy().to_string();
        let path_a = directory.path().join("a").join(SETTINGS_FILE_NAME);
        let path_b = directory.path().join("b").join(SETTINGS_FILE_NAME);

        // Seed with non-default password via local + connect.
        let mut initial = AppNotificationSettings::default();
        initial.settings_password = "4242".to_string();
        save_app_settings_to(&path_a, &initial).unwrap();
        save_app_settings_request_to(&path_a, &connect_request(&shared_path, "test-station-1"))
            .unwrap();

        // PC B wrong password
        let mut bad = connect_request(&shared_path, "pdu-lab");
        bad.connect_password = "0601".to_string();
        assert_eq!(
            save_app_settings_request_to(&path_b, &bad),
            Err(AppSettingsError::WrongFloorPassword)
        );

        // Correct password adopts.
        let mut good = connect_request(&shared_path, "pdu-lab");
        good.connect_password = "4242".to_string();
        let loaded = save_app_settings_request_to(&path_b, &good).unwrap();
        assert_eq!(loaded.settings_password, "4242");
        assert_eq!(loaded.station_id, "pdu-lab");
    }

    #[test]
    fn seed_on_connect_when_missing_still_works() {
        let directory = tempdir().unwrap();
        let shared = directory.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        let shared_path = shared.to_string_lossy().to_string();
        let path = directory.path().join(SETTINGS_FILE_NAME);

        let mut advanced = advanced_request();
        advanced.station_id = "test-station-3".to_string();
        advanced.teams_webhook_url = "https://seed.example/hook".to_string();
        advanced.stations = vec![StationCatalogEntry {
            id: "test-station-3".to_string(),
            name: "Seed Three".to_string(),
            role: StationRole::Floor,
        }];
        save_app_settings_request_to(&path, &advanced).unwrap();

        let floor_path = shared.join(super::super::floor_settings::FLOOR_SETTINGS_FILE_NAME);
        assert!(!floor_path.exists());
        let seeded =
            save_app_settings_request_to(&path, &connect_request(&shared_path, "test-station-3"))
                .unwrap();
        assert!(floor_path.is_file());
        assert_eq!(seeded.teams_webhook_url, "https://seed.example/hook");
        assert_eq!(seeded.station_name, "Seed Three");

        let floor = try_load_floor_settings(&shared_path).unwrap().unwrap();
        assert_eq!(floor.station_name_for_id("test-station-3"), "Seed Three");
        assert_eq!(floor.teams_webhook_url, "https://seed.example/hook");
    }

    #[test]
    fn connect_seed_includes_unsaved_advanced_form_fields() {
        // Single Browse+Save: no prior Advanced local save; form fields on Connect request seed.
        let directory = tempdir().unwrap();
        let shared = directory.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        let shared_path = shared.to_string_lossy().to_string();
        let path = directory.path().join(SETTINGS_FILE_NAME);

        let mut req = connect_request(&shared_path, "test-station-1");
        req.teams_webhook_url = "https://one-shot.example/hook".to_string();
        req.teams_destination_name = "PDU Lab Chat".to_string();
        req.stations = vec![
            StationCatalogEntry {
                id: "test-station-1".to_string(),
                name: "Bay One".to_string(),
                role: StationRole::Floor,
            },
            StationCatalogEntry {
                id: "test-station-3".to_string(),
                name: "Bay Three".to_string(),
                role: StationRole::Floor,
            },
            StationCatalogEntry {
                id: "test-station-4".to_string(),
                name: "Test Station 4".to_string(),
                role: StationRole::Floor,
            },
            StationCatalogEntry {
                id: "pdu-lab".to_string(),
                name: "PDU Lab".to_string(),
                role: StationRole::Floor,
            },
        ];
        let seeded = save_app_settings_request_to(&path, &req).unwrap();
        assert_eq!(seeded.teams_webhook_url, "https://one-shot.example/hook");
        assert_eq!(seeded.station_name, "Bay One");

        let floor = try_load_floor_settings(&shared_path).unwrap().unwrap();
        assert_eq!(floor.teams_webhook_url, "https://one-shot.example/hook");
        assert_eq!(floor.teams_destination_name, "PDU Lab Chat");
        assert_eq!(floor.station_name_for_id("test-station-1"), "Bay One");
        assert_eq!(floor.station_name_for_id("test-station-3"), "Bay Three");
    }

    #[test]
    fn advanced_create_admin_generates_id_selects_locally_and_writes_schema_v2() {
        let directory = tempdir().unwrap();
        let app_path = directory.path().join(SETTINGS_FILE_NAME);
        let shared = directory.path().join("shared");
        let connect = connect_request(shared.to_str().unwrap(), "pdu-lab");
        save_app_settings_request_to(&app_path, &connect).unwrap();

        let mut request = advanced_request();
        request.station_id = "pdu-lab".to_string();
        request.catalog_create = Some(CatalogCreateRequest {
            name: "Syed Admin".to_string(),
            role: StationRole::Admin,
            select_for_this_pc: true,
        });

        let saved = save_app_settings_request_to(&app_path, &request).unwrap();
        let floor = try_load_floor_settings(shared.to_str().unwrap())
            .unwrap()
            .unwrap();
        let admin = floor
            .stations
            .iter()
            .find(|entry| entry.name == "Syed Admin")
            .unwrap();

        assert_eq!(
            floor.schema_version,
            super::super::floor_settings::FLOOR_SETTINGS_SCHEMA_VERSION
        );
        assert_eq!(admin.role, StationRole::Admin);
        assert_eq!(saved.station_id, admin.id);
        assert!(!shared.join("stations").join(&admin.id).exists());
    }

    #[test]
    fn floor_identity_creation_adds_directory_and_is_available_for_summary() {
        let directory = tempdir().unwrap();
        let app_path = directory.path().join(SETTINGS_FILE_NAME);
        let shared = directory.path().join("shared");
        save_app_settings_request_to(
            &app_path,
            &connect_request(shared.to_str().unwrap(), "pdu-lab"),
        )
        .unwrap();

        let mut request = advanced_request();
        request.station_id = "pdu-lab".to_string();
        request.catalog_create = Some(CatalogCreateRequest {
            name: "Burn-In Bay".to_string(),
            role: StationRole::Floor,
            select_for_this_pc: false,
        });
        save_app_settings_request_to(&app_path, &request).unwrap();

        let floor = try_load_floor_settings(shared.to_str().unwrap())
            .unwrap()
            .unwrap();
        let created = floor
            .stations
            .iter()
            .find(|entry| entry.name == "Burn-In Bay")
            .unwrap();
        assert!(floor.is_floor_identity(&created.id));
        assert!(shared.join("stations").join(&created.id).is_dir());
    }

    #[test]
    fn identity_creation_requires_a_connected_shared_folder() {
        let directory = tempdir().unwrap();
        let app_path = directory.path().join(SETTINGS_FILE_NAME);
        let mut request = advanced_request();
        request.catalog_create = Some(CatalogCreateRequest {
            name: "Desk Admin".to_string(),
            role: StationRole::Admin,
            select_for_this_pc: true,
        });

        assert_eq!(
            save_app_settings_request_to(&app_path, &request),
            Err(AppSettingsError::SharedFolderRequiredForIdentity)
        );
    }

    #[test]
    fn partial_rename_and_password_change_preserve_peer_dynamic_identity() {
        let directory = tempdir().unwrap();
        let app_path = directory.path().join(SETTINGS_FILE_NAME);
        let shared = directory.path().join("shared");
        save_app_settings_request_to(
            &app_path,
            &connect_request(shared.to_str().unwrap(), "pdu-lab"),
        )
        .unwrap();

        let mut create = advanced_request();
        create.station_id = "pdu-lab".to_string();
        create.catalog_create = Some(CatalogCreateRequest {
            name: "Peer Admin".to_string(),
            role: StationRole::Admin,
            select_for_this_pc: false,
        });
        save_app_settings_request_to(&app_path, &create).unwrap();

        let mut rename = advanced_request();
        rename.station_id = "pdu-lab".to_string();
        rename.stations = vec![StationCatalogEntry {
            id: "test-station-1".to_string(),
            name: "Bay One".to_string(),
            role: StationRole::Floor,
        }];
        save_app_settings_request_to(&app_path, &rename).unwrap();
        change_settings_password_at(
            &app_path,
            &ChangeSettingsPasswordRequest {
                current_password: DEFAULT_SETTINGS_PASSWORD.to_string(),
                new_password: "4242".to_string(),
                confirm_password: "4242".to_string(),
            },
        )
        .unwrap();

        let floor = try_load_floor_settings(shared.to_str().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(floor.settings_password, "4242");
        assert_eq!(floor.station_name_for_id("test-station-1"), "Bay One");
        assert!(floor
            .stations
            .iter()
            .any(|entry| entry.name == "Peer Admin" && entry.role == StationRole::Admin));
    }

    #[test]
    fn password_verification_change_and_confirmation_are_enforced() {
        let mut settings = AppNotificationSettings::default();
        assert!(verify_password(&settings, "0601"));
        assert!(!verify_password(&settings, "0000"));
        assert_eq!(
            change_password(&mut settings, "wrong", "9999"),
            Err(AppSettingsError::WrongPassword)
        );
        change_password(&mut settings, "0601", "9999").unwrap();
        assert!(verify_password(&settings, "9999"));

        let directory = tempdir().unwrap();
        let path = directory.path().join(SETTINGS_FILE_NAME);
        save_app_settings_to(&path, &settings).unwrap();
        let mismatch = ChangeSettingsPasswordRequest {
            current_password: "9999".to_string(),
            new_password: "1111".to_string(),
            confirm_password: "2222".to_string(),
        };
        assert_eq!(
            change_settings_password_at(&path, &mismatch),
            Err(AppSettingsError::PasswordMismatch)
        );
        assert!(verify_password(
            &load_app_settings_from(&path).unwrap(),
            "9999"
        ));
    }

    #[test]
    fn password_change_with_shared_path_does_not_reseed() {
        let directory = tempdir().unwrap();
        let shared = directory.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        let shared_path = shared.to_string_lossy().to_string();
        let path = directory.path().join(SETTINGS_FILE_NAME);

        save_app_settings_request_to(&path, &connect_request(&shared_path, "test-station-1"))
            .unwrap();
        let floor_path = shared.join(super::super::floor_settings::FLOOR_SETTINGS_FILE_NAME);
        fs::remove_file(&floor_path).unwrap();

        let req = ChangeSettingsPasswordRequest {
            current_password: DEFAULT_SETTINGS_PASSWORD.to_string(),
            new_password: "9999".to_string(),
            confirm_password: "9999".to_string(),
        };
        assert!(matches!(
            change_settings_password_at(&path, &req),
            Err(AppSettingsError::Floor(_))
        ));
        assert!(!floor_path.exists());
    }

    #[test]
    fn resolved_config_maps_app_fields_including_shift_log() {
        let mut settings = AppNotificationSettings::default();
        settings.teams_webhook_url = " https://example.invalid/hook ".to_string();
        settings.station_id = "test-station-4".to_string();
        settings.station_name.clear();
        settings.shared_shift_log_path = " C:/shared/shift_log.json ".to_string();

        let resolved = settings.to_resolved_config();
        assert_eq!(resolved.station_name, "Test Station 4");
        assert_eq!(resolved.teams_webhook_url, "https://example.invalid/hook");
        assert_eq!(resolved.shared_shift_log_path, "C:/shared/shift_log.json");
        assert!(can_send(&resolved).is_ok());
    }

    #[test]
    fn corrupt_or_unsupported_file_is_not_silently_replaced() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(SETTINGS_FILE_NAME);
        fs::write(&path, "{not-json").unwrap();
        assert!(matches!(
            load_app_settings_from(&path),
            Err(AppSettingsError::Parse(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{not-json");

        fs::write(
            &path,
            r#"{"schema_version":2,"station_id":"test-station-1"}"#,
        )
        .unwrap();
        assert_eq!(
            load_app_settings_from(&path),
            Err(AppSettingsError::UnsupportedSchema(2))
        );
    }

    #[test]
    fn interrupted_windows_backup_is_recovered_instead_of_factory_reset() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(SETTINGS_FILE_NAME);
        let backup = directory
            .path()
            .join(format!(".{SETTINGS_FILE_NAME}.crashed.bak"));
        let mut expected = AppNotificationSettings::default();
        expected.settings_password = "2468".to_string();
        expected.teams_webhook_url = "https://example.invalid/workflow?sig=RECOVER_ME".to_string();
        save_app_settings_to(&path, &expected).unwrap();
        fs::rename(&path, &backup).unwrap();

        let recovered = load_app_settings_from(&path).unwrap();

        assert_eq!(recovered, expected);
        assert!(path.is_file());
        assert!(!backup.exists());
    }

    #[test]
    fn synced_temp_is_preferred_over_backup_during_interrupted_write_recovery() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(SETTINGS_FILE_NAME);
        let backup = directory
            .path()
            .join(format!(".{SETTINGS_FILE_NAME}.crashed.bak"));
        let temp = directory
            .path()
            .join(format!(".{SETTINGS_FILE_NAME}.crashed.tmp"));
        let mut old = AppNotificationSettings::default();
        old.settings_password = "1111".to_string();
        let mut intended = old.clone();
        intended.settings_password = "2222".to_string();
        intended.station_id = "test-station-3".to_string();
        intended.station_name = "Test Station 3".to_string();
        fs::write(&backup, serde_json::to_vec_pretty(&old).unwrap()).unwrap();
        fs::write(&temp, serde_json::to_vec_pretty(&intended).unwrap()).unwrap();

        let recovered = load_app_settings_from(&path).unwrap();

        assert_eq!(recovered, intended);
        assert!(path.is_file());
        assert!(!backup.exists());
        assert!(!temp.exists());
    }

    #[test]
    fn invalid_recovery_artifacts_are_preserved_and_do_not_create_defaults() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(SETTINGS_FILE_NAME);
        let backup = directory
            .path()
            .join(format!(".{SETTINGS_FILE_NAME}.crashed.bak"));
        fs::write(&backup, "{not-json").unwrap();

        assert!(matches!(
            load_app_settings_from(&path),
            Err(AppSettingsError::Recovery(_))
        ));
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(&backup).unwrap(), "{not-json");
    }

    #[test]
    fn atomic_save_leaves_only_the_settings_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(SETTINGS_FILE_NAME);
        save_app_settings_to(&path, &AppNotificationSettings::default()).unwrap();
        save_app_settings_to(&path, &AppNotificationSettings::default()).unwrap();

        let names = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, [std::ffi::OsString::from(SETTINGS_FILE_NAME)]);
    }
}
