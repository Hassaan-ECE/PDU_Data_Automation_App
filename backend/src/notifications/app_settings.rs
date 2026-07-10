//! App-owned notification settings stored under Tauri's `app_config_dir`.
//!
//! The public `*_from` helpers keep disk behavior testable without changing the
//! process-wide directory. Production calls use the directory installed once
//! during Tauri setup via [`set_app_config_dir`].

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
use super::shift_log;
use super::stations::{
    is_known_station_id, station_name_for_id, DEFAULT_SUMMARY_POSTER_STATION_ID,
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
    #[error("station_id is empty")]
    EmptyStationId,
    #[error("Unknown station_id '{0}'")]
    UnknownStation(String),
    #[error("Invalid shift window: {0}")]
    InvalidShift(String),
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
            .finish()
    }
}

impl From<&AppNotificationSettings> for AppNotificationSettingsView {
    fn from(settings: &AppNotificationSettings) -> Self {
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
        Self {
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
        }
    }
}

/// Safe update shape: schema version and password cannot be overwritten by the
/// ordinary settings form. Empty webhook means "leave unchanged" unless the
/// explicit `clear_webhook` flag is set.
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

pub fn load_app_settings() -> Result<AppNotificationSettings, AppSettingsError> {
    load_app_settings_from(configured_app_settings_path()?)
}

/// Load settings, atomically creating and persisting schema-v1 defaults when
/// the file does not yet exist.
pub fn load_app_settings_from(
    path: impl AsRef<Path>,
) -> Result<AppNotificationSettings, AppSettingsError> {
    let _guard = settings_io_guard()?;
    load_or_create_unlocked(path.as_ref())
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
    save_app_settings_request_to(configured_app_settings_path()?, request)
        .map(|settings| AppNotificationSettingsView::from(&settings))
}

pub fn save_app_settings_request_to(
    path: impl AsRef<Path>,
    request: &SaveAppNotificationSettingsRequest,
) -> Result<AppNotificationSettings, AppSettingsError> {
    let _guard = settings_io_guard()?;
    let path = path.as_ref();
    let mut settings = load_or_create_unlocked(path)?;
    apply_settings_request(&mut settings, request)?;
    // This is an exact write: `apply_settings_request` already implements the
    // empty-preserves-existing and explicit-clear semantics.
    write_settings_unlocked(path, &settings)?;
    Ok(settings)
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
    change_password(&mut settings, &request.current_password, new_password)?;
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

fn apply_settings_request(
    settings: &mut AppNotificationSettings,
    request: &SaveAppNotificationSettingsRequest,
) -> Result<(), AppSettingsError> {
    let station_id = request.station_id.trim();
    if station_id.is_empty() {
        return Err(AppSettingsError::EmptyStationId);
    }
    if !is_known_station_id(station_id) {
        return Err(AppSettingsError::UnknownStation(station_id.to_string()));
    }

    settings.enabled = request.enabled;
    settings.teams_destination_name = match request.teams_destination_name.trim() {
        "" => default_destination_name(),
        value => value.to_string(),
    };
    if request.clear_webhook {
        settings.teams_webhook_url.clear();
    } else if !request.teams_webhook_url.trim().is_empty() {
        settings.teams_webhook_url = request.teams_webhook_url.trim().to_string();
    }
    settings.station_id = station_id.to_string();
    settings.station_name = match request.station_name.trim() {
        "" => station_name_for_id(station_id).to_string(),
        value => value.to_string(),
    };
    settings.idle_timeout_minutes = request.idle_timeout_minutes;
    settings.events = request.events.clone();
    settings.shared_shift_log_path = request.shared_shift_log_path.trim().to_string();
    settings.shifts = normalize_shifts(&request.shifts)?;
    let poster = request.summary_poster_station_id.trim();
    settings.summary_poster_station_id = if poster.is_empty() {
        default_summary_poster()
    } else if is_known_station_id(poster) {
        poster.to_string()
    } else {
        return Err(AppSettingsError::UnknownStation(poster.to_string()));
    };
    settings.summary_included_station_ids =
        normalize_included_station_ids(&request.summary_included_station_ids)?;
    validate_settings(settings)?;
    Ok(())
}

fn normalize_included_station_ids(ids: &[String]) -> Result<Vec<String>, AppSettingsError> {
    if ids.is_empty() {
        return Ok(default_summary_included());
    }
    let mut out = Vec::new();
    for id in ids {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        if !is_known_station_id(id) {
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
mod tests {
    use super::*;
    use crate::notifications::can_send;
    use tempfile::tempdir;

    fn request() -> SaveAppNotificationSettingsRequest {
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
                stuck: false,
                summary: true,
            },
            shared_shift_log_path: r"\\server\share\shift_log.json".to_string(),
            shifts: vec![ShiftWindow {
                label: "Day".to_string(),
                start_time: "06:00".to_string(),
                end_time: "15:00".to_string(),
            }],
            summary_poster_station_id: "pdu-lab".to_string(),
            summary_included_station_ids: default_summary_included(),
        }
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
        settings.shared_shift_log_path = r"\\server\share\shift_log.json".to_string();

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
        let mut initial = AppNotificationSettings::default();
        initial.settings_password = "2468".to_string();
        initial.teams_webhook_url = "https://example.invalid/hook?sig=KEEP_ME".to_string();
        save_app_settings_to(&path, &initial).unwrap();

        let updated = save_app_settings_request_to(&path, &request()).unwrap();
        assert_eq!(updated.settings_password, "2468");
        assert_eq!(updated.station_name, "Test Station 3");
        assert_eq!(updated.teams_webhook_url, initial.teams_webhook_url);
        assert_eq!(
            updated.shared_shift_log_path,
            request().shared_shift_log_path
        );

        let mut clear = request();
        clear.clear_webhook = true;
        let cleared = save_app_settings_request_to(&path, &clear).unwrap();
        assert!(cleared.teams_webhook_url.is_empty());
        assert_eq!(cleared.settings_password, "2468");
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
