//! Floor-wide notification settings stored in the shared notifications folder.
//!
//! When operators configure a shared OneDrive/network root, this file is the
//! source of truth for password, webhook, shifts, poster, included stations,
//! event toggles, and editable station display names. Each PC still keeps its
//! own `station_id` and path pointer in AppData.
//!
//! # Multi-PC / OneDrive concurrency
//!
//! Writes use a best-effort cross-process lock file and unique temp names so two
//! processes on the **same** machine do not interleave a replace. OneDrive (and
//! similar multi-replica sync) is still **best-effort**: each PC has its own
//! local folder replica, so last-writer-wins can still apply across PCs after
//! sync. Do not treat this file as a strongly consistent distributed store.

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

use super::app_settings::{
    AppNotificationSettings, ShiftWindow, StationCatalogEntry, DEFAULT_SETTINGS_PASSWORD,
};
use super::config::EventToggles;
use super::stations::{
    is_known_station_id, known_stations_owned, station_name_for_id,
    DEFAULT_SUMMARY_POSTER_STATION_ID, KNOWN_STATIONS,
};

pub const FLOOR_SETTINGS_SCHEMA_VERSION: u32 = 1;
pub const FLOOR_SETTINGS_FILE_NAME: &str = "floor_settings.json";
pub const MAX_STATION_DISPLAY_NAME_LEN: usize = 64;

const LOCK_WAIT: Duration = Duration::from_secs(2);
const LOCK_POLL: Duration = Duration::from_millis(20);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FloorSettingsError {
    #[error("Could not read floor settings: {0}")]
    Read(String),
    #[error("Could not write floor settings: {0}")]
    Write(String),
    #[error("Invalid floor settings JSON: {0}")]
    Parse(String),
    #[error("Unsupported floor settings schema version {0}")]
    UnsupportedSchema(u32),
    #[error("station_id is empty")]
    EmptyStationId,
    #[error("Unknown station_id '{0}'")]
    UnknownStation(String),
    #[error("Station display name is empty")]
    EmptyStationName,
    #[error("Station display name is too long (max {MAX_STATION_DISPLAY_NAME_LEN} characters)")]
    StationNameTooLong,
    #[error("Station display name contains invalid characters")]
    InvalidStationName,
    #[error("Duplicate station display name '{0}'")]
    DuplicateStationName(String),
    #[error("Settings password must not be empty")]
    EmptyPassword,
    #[error("Invalid shift window: {0}")]
    InvalidShift(String),
    #[error("Shared folder path is empty")]
    EmptySharedPath,
    #[error("Floor settings are busy; try again")]
    Busy,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct FloorStation {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FloorSettings {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub updated_by_station_id: String,
    #[serde(default = "default_password")]
    pub settings_password: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_destination_name")]
    pub teams_destination_name: String,
    #[serde(default)]
    pub teams_webhook_url: String,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_minutes: u32,
    #[serde(default = "default_events")]
    pub events: EventToggles,
    #[serde(default)]
    pub shifts: Vec<ShiftWindow>,
    #[serde(default = "default_summary_poster")]
    pub summary_poster_station_id: String,
    #[serde(default = "default_summary_included")]
    pub summary_included_station_ids: Vec<String>,
    #[serde(default = "default_stations")]
    pub stations: Vec<FloorStation>,
}

impl Default for FloorSettings {
    fn default() -> Self {
        Self {
            schema_version: FLOOR_SETTINGS_SCHEMA_VERSION,
            updated_at: String::new(),
            updated_by_station_id: String::new(),
            settings_password: default_password(),
            enabled: true,
            teams_destination_name: default_destination_name(),
            teams_webhook_url: String::new(),
            idle_timeout_minutes: default_idle_timeout(),
            events: default_events(),
            shifts: Vec::new(),
            summary_poster_station_id: default_summary_poster(),
            summary_included_station_ids: default_summary_included(),
            stations: default_stations(),
        }
    }
}

impl fmt::Debug for FloorSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FloorSettings")
            .field("schema_version", &self.schema_version)
            .field("updated_at", &self.updated_at)
            .field("updated_by_station_id", &self.updated_by_station_id)
            .field("settings_password", &"<redacted>")
            .field("enabled", &self.enabled)
            .field("teams_destination_name", &self.teams_destination_name)
            .field(
                "teams_webhook_url",
                if self.teams_webhook_url.trim().is_empty() {
                    &"<empty>"
                } else {
                    &"<redacted>"
                },
            )
            .field("idle_timeout_minutes", &self.idle_timeout_minutes)
            .field("events", &self.events)
            .field("shifts", &self.shifts)
            .field("summary_poster_station_id", &self.summary_poster_station_id)
            .field(
                "summary_included_station_ids",
                &self.summary_included_station_ids,
            )
            .field("stations", &self.stations)
            .finish()
    }
}

impl FloorSettings {
    /// Build floor settings from a full local settings snapshot (first-seed path).
    pub fn from_local_settings(local: &AppNotificationSettings) -> Self {
        let mut stations = default_stations();
        // Prefer full local catalog cache when seeding (renames survive connect).
        if !local.station_catalog.is_empty() {
            for entry in &local.station_catalog {
                let id = entry.id.trim();
                let name = entry.name.trim();
                if id.is_empty() || name.is_empty() {
                    continue;
                }
                if let Some(slot) = stations.iter_mut().find(|s| s.id == id) {
                    slot.name = name.to_string();
                }
            }
        }
        // Prefer local station_name for this PC's id when seeding.
        let local_id = local.station_id.trim();
        let local_name = local.station_name.trim();
        if !local_id.is_empty() && !local_name.is_empty() {
            if let Some(entry) = stations.iter_mut().find(|s| s.id == local_id) {
                entry.name = local_name.to_string();
            }
        }

        let poster = local.summary_poster_station_id.trim();
        let poster = if poster.is_empty() || !is_known_station_id(poster) {
            DEFAULT_SUMMARY_POSTER_STATION_ID.to_string()
        } else {
            poster.to_string()
        };

        let included = if local.summary_included_station_ids.is_empty() {
            default_summary_included()
        } else {
            local
                .summary_included_station_ids
                .iter()
                .map(|id| id.trim().to_string())
                .filter(|id| is_known_station_id(id))
                .collect()
        };
        let included = if included.is_empty() {
            default_summary_included()
        } else {
            included
        };

        Self {
            schema_version: FLOOR_SETTINGS_SCHEMA_VERSION,
            updated_at: now_rfc3339(),
            updated_by_station_id: local.station_id.trim().to_string(),
            settings_password: match local.settings_password.trim() {
                "" => DEFAULT_SETTINGS_PASSWORD.to_string(),
                value => value.to_string(),
            },
            enabled: local.enabled,
            teams_destination_name: match local.teams_destination_name.trim() {
                "" => default_destination_name(),
                value => value.to_string(),
            },
            teams_webhook_url: local.teams_webhook_url.trim().to_string(),
            idle_timeout_minutes: local.idle_timeout_minutes,
            events: local.events.clone(),
            shifts: local.shifts.clone(),
            summary_poster_station_id: poster,
            summary_included_station_ids: included,
            stations,
        }
    }

    pub fn station_name_for_id(&self, station_id: &str) -> String {
        let station_id = station_id.trim();
        self.stations
            .iter()
            .find(|s| s.id == station_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| station_name_for_id(station_id).to_string())
    }

    pub fn catalog_pairs(&self) -> Vec<(String, String)> {
        self.stations
            .iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    }

    /// Apply floor fields onto a local settings model (keeps local station_id / path).
    /// Also caches the full station catalog for offline summary / failed floor reads.
    pub fn apply_to_local(&self, local: &mut AppNotificationSettings) {
        local.settings_password = self.settings_password.clone();
        local.enabled = self.enabled;
        local.teams_destination_name = self.teams_destination_name.clone();
        local.teams_webhook_url = self.teams_webhook_url.clone();
        local.idle_timeout_minutes = self.idle_timeout_minutes;
        local.events = self.events.clone();
        local.shifts = self.shifts.clone();
        local.summary_poster_station_id = self.summary_poster_station_id.clone();
        local.summary_included_station_ids = self.summary_included_station_ids.clone();
        local.station_catalog = self
            .stations
            .iter()
            .map(|s| StationCatalogEntry {
                id: s.id.clone(),
                name: s.name.clone(),
            })
            .collect();
        local.station_name = self.station_name_for_id(&local.station_id);
    }
}

/// Resolve `floor_settings.json` under a configured shared root or next to a legacy JSON path.
pub fn resolve_floor_settings_file(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if configured.is_empty() {
        return None;
    }
    let path = PathBuf::from(configured);
    if looks_like_json_file(&path) {
        path.parent()
            .map(|parent| parent.join(FLOOR_SETTINGS_FILE_NAME))
    } else {
        Some(path.join(FLOOR_SETTINGS_FILE_NAME))
    }
}

/// Load floor settings if the file exists. `Ok(None)` means the file is absent
/// (caller should seed only on explicit Connect). Other errors are hard failures
/// for that read attempt.
pub fn try_load_floor_settings(
    configured_shared_path: &str,
) -> Result<Option<FloorSettings>, FloorSettingsError> {
    let Some(path) = resolve_floor_settings_file(configured_shared_path) else {
        return Err(FloorSettingsError::EmptySharedPath);
    };
    if !path.is_file() {
        return Ok(None);
    }
    load_floor_settings_from(&path).map(Some)
}

pub fn load_floor_settings_from(
    path: impl AsRef<Path>,
) -> Result<FloorSettings, FloorSettingsError> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path)
        .map_err(|error| FloorSettingsError::Read(format!("{}: {error}", path.display())))?;
    parse_floor_settings(path, &raw)
}

pub fn save_floor_settings(
    configured_shared_path: &str,
    settings: &FloorSettings,
) -> Result<PathBuf, FloorSettingsError> {
    let Some(path) = resolve_floor_settings_file(configured_shared_path) else {
        return Err(FloorSettingsError::EmptySharedPath);
    };
    save_floor_settings_to(&path, settings)?;
    Ok(path)
}

pub fn save_floor_settings_to(
    path: impl AsRef<Path>,
    settings: &FloorSettings,
) -> Result<(), FloorSettingsError> {
    let path = path.as_ref();
    ensure_parent_directory(path).map_err(FloorSettingsError::Write)?;
    let _lock = SharedFloorLock::acquire(path)?;
    write_floor_contents_while_locked(path, settings)
}

/// Acquire the floor lock, load the latest file (if any), apply `mutator`, validate, and write
/// before releasing. Use this for all read–patch–write updates so two processes cannot both
/// patch the same revision without coordinating (same machine / true network share).
///
/// OneDrive multi-PC coordination remains best-effort because each PC has its own replica.
pub fn update_floor_settings_with_lock(
    configured_shared_path: &str,
    mutator: impl FnOnce(Option<FloorSettings>) -> Result<FloorSettings, FloorSettingsError>,
) -> Result<FloorSettings, FloorSettingsError> {
    let Some(path) = resolve_floor_settings_file(configured_shared_path) else {
        return Err(FloorSettingsError::EmptySharedPath);
    };
    ensure_parent_directory(&path).map_err(FloorSettingsError::Write)?;
    let _lock = SharedFloorLock::acquire(&path)?;
    let current = if path.is_file() {
        Some(load_floor_settings_from(&path)?)
    } else {
        None
    };
    let next = mutator(current)?;
    write_floor_contents_while_locked(&path, &next)?;
    Ok(next)
}

fn write_floor_contents_while_locked(
    path: &Path,
    settings: &FloorSettings,
) -> Result<(), FloorSettingsError> {
    let mut settings = settings.clone();
    validate_floor_settings(&mut settings)?;
    if settings.updated_at.trim().is_empty() {
        settings.updated_at = now_rfc3339();
    }
    let raw = serde_json::to_vec_pretty(&settings)
        .map_err(|error| FloorSettingsError::Write(error.to_string()))?;
    atomic_replace(path, &raw).map_err(FloorSettingsError::Write)
}

/// Create floor settings when missing; return existing file when present (do not overwrite).
/// Prefer explicit Connect scope in app_settings over calling this from ordinary loads.
pub fn load_or_seed_floor_settings(
    configured_shared_path: &str,
    seed_from: &AppNotificationSettings,
) -> Result<FloorSettings, FloorSettingsError> {
    match try_load_floor_settings(configured_shared_path)? {
        Some(existing) => Ok(existing),
        None => {
            let seeded = FloorSettings::from_local_settings(seed_from);
            save_floor_settings(configured_shared_path, &seeded)?;
            Ok(seeded)
        }
    }
}

pub fn validate_floor_settings(settings: &mut FloorSettings) -> Result<(), FloorSettingsError> {
    if settings.schema_version != FLOOR_SETTINGS_SCHEMA_VERSION {
        return Err(FloorSettingsError::UnsupportedSchema(
            settings.schema_version,
        ));
    }
    if settings.settings_password.trim().is_empty() {
        return Err(FloorSettingsError::EmptyPassword);
    }
    settings.settings_password = settings.settings_password.trim().to_string();

    settings.teams_destination_name = match settings.teams_destination_name.trim() {
        "" => default_destination_name(),
        value => value.to_string(),
    };
    settings.teams_webhook_url = settings.teams_webhook_url.trim().to_string();

    // Normalize catalog: fixed known ids only. Missing ids get defaults; a
    // provided blank name for a known id is rejected (EmptyStationName).
    let mut normalized = Vec::with_capacity(KNOWN_STATIONS.len());
    for (id, default_name) in KNOWN_STATIONS {
        if let Some(entry) = settings.stations.iter().find(|s| s.id.trim() == *id) {
            let name = entry.name.trim().to_string();
            if name.is_empty() {
                return Err(FloorSettingsError::EmptyStationName);
            }
            validate_display_name(&name)?;
            normalized.push(FloorStation {
                id: (*id).to_string(),
                name,
            });
        } else {
            normalized.push(FloorStation {
                id: (*id).to_string(),
                name: (*default_name).to_string(),
            });
        }
    }
    if normalized.is_empty() {
        return Err(FloorSettingsError::EmptyStationId);
    }
    reject_duplicate_display_names(&normalized)?;
    settings.stations = normalized;

    let poster = settings.summary_poster_station_id.trim();
    if poster.is_empty() {
        settings.summary_poster_station_id = default_summary_poster();
    } else if !is_known_station_id(poster) {
        return Err(FloorSettingsError::UnknownStation(poster.to_string()));
    } else {
        settings.summary_poster_station_id = poster.to_string();
    }

    let mut included = Vec::new();
    for id in &settings.summary_included_station_ids {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        if !is_known_station_id(id) {
            return Err(FloorSettingsError::UnknownStation(id.to_string()));
        }
        if !included.iter().any(|existing: &String| existing == id) {
            included.push(id.to_string());
        }
    }
    if included.is_empty() {
        included = default_summary_included();
    }
    settings.summary_included_station_ids = included;

    if settings.shifts.len() > 2 {
        return Err(FloorSettingsError::InvalidShift(
            "at most two shift windows are supported".to_string(),
        ));
    }
    let mut normalized_shifts = Vec::new();
    for shift in &settings.shifts {
        let label = shift.label.trim().to_string();
        let start_time = shift.start_time.trim().to_string();
        let end_time = shift.end_time.trim().to_string();
        if label.is_empty() && start_time.is_empty() && end_time.is_empty() {
            continue;
        }
        if label.is_empty() {
            return Err(FloorSettingsError::InvalidShift(
                "each shift needs a label".to_string(),
            ));
        }
        validate_hhmm(&start_time)?;
        validate_hhmm(&end_time)?;
        if start_time == end_time {
            return Err(FloorSettingsError::InvalidShift(format!(
                "shift '{label}' start and end must differ"
            )));
        }
        normalized_shifts.push(ShiftWindow {
            label,
            start_time,
            end_time,
        });
    }
    settings.shifts = normalized_shifts;

    Ok(())
}

/// Validate a station display name that is being set (non-empty, length, controls).
pub fn validate_display_name(name: &str) -> Result<(), FloorSettingsError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(FloorSettingsError::EmptyStationName);
    }
    if name.chars().count() > MAX_STATION_DISPLAY_NAME_LEN {
        return Err(FloorSettingsError::StationNameTooLong);
    }
    if name
        .chars()
        .any(|ch| ch.is_control() || ch == '\n' || ch == '\r')
    {
        return Err(FloorSettingsError::InvalidStationName);
    }
    Ok(())
}

fn reject_duplicate_display_names(stations: &[FloorStation]) -> Result<(), FloorSettingsError> {
    for (index, station) in stations.iter().enumerate() {
        let needle = station.name.trim().to_lowercase();
        for other in stations.iter().skip(index + 1) {
            if other.name.trim().to_lowercase() == needle {
                return Err(FloorSettingsError::DuplicateStationName(
                    station.name.trim().to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_hhmm(value: &str) -> Result<(), FloorSettingsError> {
    let bytes = value.as_bytes();
    if bytes.len() != 5 || bytes[2] != b':' {
        return Err(FloorSettingsError::InvalidShift(format!(
            "time '{value}' must be HH:MM"
        )));
    }
    let hour: u32 = value[..2]
        .parse()
        .map_err(|_| FloorSettingsError::InvalidShift(format!("time '{value}' must be HH:MM")))?;
    let minute: u32 = value[3..]
        .parse()
        .map_err(|_| FloorSettingsError::InvalidShift(format!("time '{value}' must be HH:MM")))?;
    if hour > 23 || minute > 59 {
        return Err(FloorSettingsError::InvalidShift(format!(
            "time '{value}' is out of range"
        )));
    }
    Ok(())
}

fn parse_floor_settings(path: &Path, raw: &str) -> Result<FloorSettings, FloorSettingsError> {
    let mut settings: FloorSettings = serde_json::from_str(raw)
        .map_err(|error| FloorSettingsError::Parse(format!("{}: {error}", path.display())))?;
    validate_floor_settings(&mut settings)?;
    Ok(settings)
}

fn looks_like_json_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

fn ensure_parent_directory(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) else {
        return Ok(());
    };
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))
}

/// Best-effort cross-process lock for floor_settings.json writes (same machine).
struct SharedFloorLock {
    path: PathBuf,
}

impl SharedFloorLock {
    fn acquire(floor_path: &Path) -> Result<Self, FloorSettingsError> {
        let path = lock_path_for(floor_path);
        let started = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    if let Err(error) = writeln!(file, "pid={}", std::process::id()) {
                        drop(file);
                        let _ = fs::remove_file(&path);
                        return Err(FloorSettingsError::Write(format!(
                            "{}: {error}",
                            path.display()
                        )));
                    }
                    return Ok(Self { path });
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists
                        || (error.kind() == std::io::ErrorKind::PermissionDenied
                            && path.exists()) =>
                {
                    if lock_is_stale(&path) {
                        match fs::remove_file(&path) {
                            Ok(()) => continue,
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                            Err(_) => {}
                        }
                    }
                    if started.elapsed() >= LOCK_WAIT {
                        return Err(FloorSettingsError::Busy);
                    }
                    thread::sleep(LOCK_POLL);
                }
                Err(error) => {
                    return Err(FloorSettingsError::Write(format!(
                        "{}: {error}",
                        path.display()
                    )))
                }
            }
        }
    }
}

impl Drop for SharedFloorLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_is_stale(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata
        .modified()
        .or_else(|_| metadata.created())
        .ok()
        .and_then(|timestamp| timestamp.elapsed().ok())
        .is_some_and(|age| age >= STALE_LOCK_AGE)
}

fn lock_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(FLOOR_SETTINGS_FILE_NAME);
    path.with_file_name(format!("{file_name}.lock"))
}

fn atomic_replace(path: &Path, contents: &[u8]) -> Result<(), String> {
    let temp_path = sibling_work_path(path, "tmp");
    let mut temp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| format!("{}: {error}", temp_path.display()))?;
    temp_file
        .write_all(contents)
        .map_err(|error| format!("{}: {error}", temp_path.display()))?;
    temp_file
        .sync_all()
        .map_err(|error| format!("{}: {error}", temp_path.display()))?;
    drop(temp_file);

    // Prefer replace; on Windows rename over existing may fail — remove then rename.
    if path.exists() {
        let backup = sibling_work_path(path, "bak");
        let _ = fs::remove_file(&backup);
        if let Err(error) = fs::rename(path, &backup) {
            let _ = fs::remove_file(&temp_path);
            return Err(format!("{}: {error}", path.display()));
        }
        if let Err(error) = fs::rename(&temp_path, path) {
            let _ = fs::rename(&backup, path);
            let _ = fs::remove_file(&temp_path);
            return Err(format!("{}: {error}", path.display()));
        }
        let _ = fs::remove_file(&backup);
    } else if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("{}: {error}", path.display()));
    }
    Ok(())
}

fn sibling_work_path(path: &Path, suffix: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(FLOOR_SETTINGS_FILE_NAME);
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    parent.join(format!(
        ".{file_name}.{}.{counter}.{millis}.{suffix}",
        std::process::id()
    ))
}

fn now_rfc3339() -> String {
    Local::now().to_rfc3339()
}

fn default_schema_version() -> u32 {
    FLOOR_SETTINGS_SCHEMA_VERSION
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

fn default_idle_timeout() -> u32 {
    30
}

fn default_events() -> EventToggles {
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
    KNOWN_STATIONS
        .iter()
        .map(|(id, _)| (*id).to_string())
        .collect()
}

fn default_stations() -> Vec<FloorStation> {
    known_stations_owned()
        .into_iter()
        .map(|(id, name)| FloorStation { id, name })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_catalog_has_four_fixed_stations() {
        let floor = FloorSettings::default();
        assert_eq!(floor.stations.len(), 4);
        assert!(floor.stations.iter().any(|s| s.id == "pdu-lab"));
        assert!(!floor.stations.iter().any(|s| s.id == "test-station-2"));
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(FLOOR_SETTINGS_FILE_NAME);
        let mut floor = FloorSettings::default();
        floor.teams_webhook_url = "https://example.invalid/hook".to_string();
        floor.settings_password = "9999".to_string();
        floor.stations[0].name = "Bay Alpha".to_string();
        save_floor_settings_to(&path, &floor).unwrap();
        let loaded = load_floor_settings_from(&path).unwrap();
        assert_eq!(loaded.teams_webhook_url, "https://example.invalid/hook");
        assert_eq!(loaded.settings_password, "9999");
        assert_eq!(loaded.station_name_for_id("test-station-1"), "Bay Alpha");
        assert!(loaded.updated_at.contains('T') || loaded.updated_at.contains('+'));
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = tempdir().unwrap();
        let configured = dir.path().to_string_lossy().to_string();
        assert!(try_load_floor_settings(&configured).unwrap().is_none());
    }

    #[test]
    fn seed_creates_file_once_then_adopts() {
        let dir = tempdir().unwrap();
        let configured = dir.path().to_string_lossy().to_string();
        let mut local = AppNotificationSettings::default();
        local.teams_webhook_url = "https://seed.example/hook".to_string();
        local.settings_password = "4242".to_string();
        local.station_id = "test-station-3".to_string();
        local.station_name = "Custom Three".to_string();

        let first = load_or_seed_floor_settings(&configured, &local).unwrap();
        assert_eq!(first.teams_webhook_url, "https://seed.example/hook");
        assert_eq!(first.settings_password, "4242");
        assert_eq!(first.station_name_for_id("test-station-3"), "Custom Three");

        // Second seed from different local must not overwrite.
        local.teams_webhook_url = "https://other.example/hook".to_string();
        local.settings_password = "0000".to_string();
        let second = load_or_seed_floor_settings(&configured, &local).unwrap();
        assert_eq!(second.teams_webhook_url, "https://seed.example/hook");
        assert_eq!(second.settings_password, "4242");
    }

    #[test]
    fn empty_display_name_is_rejected() {
        let mut floor = FloorSettings::default();
        floor.stations[0].name = "   ".to_string();
        assert_eq!(
            validate_floor_settings(&mut floor),
            Err(FloorSettingsError::EmptyStationName)
        );
    }

    #[test]
    fn name_too_long_is_rejected() {
        let mut floor = FloorSettings::default();
        floor.stations[0].name = "x".repeat(MAX_STATION_DISPLAY_NAME_LEN + 1);
        assert_eq!(
            validate_floor_settings(&mut floor),
            Err(FloorSettingsError::StationNameTooLong)
        );
    }

    #[test]
    fn control_chars_and_duplicate_names_rejected() {
        let mut floor = FloorSettings::default();
        floor.stations[0].name = "Bad\nName".to_string();
        assert_eq!(
            validate_floor_settings(&mut floor),
            Err(FloorSettingsError::InvalidStationName)
        );

        floor.stations[0].name = "Same Bay".to_string();
        floor.stations[1].name = "same bay".to_string();
        assert!(matches!(
            validate_floor_settings(&mut floor),
            Err(FloorSettingsError::DuplicateStationName(_))
        ));
    }

    #[test]
    fn invalid_shift_times_rejected_on_load() {
        let mut floor = FloorSettings::default();
        floor.shifts = vec![ShiftWindow {
            label: "Day".to_string(),
            start_time: "25:00".to_string(),
            end_time: "15:00".to_string(),
        }];
        assert!(matches!(
            validate_floor_settings(&mut floor),
            Err(FloorSettingsError::InvalidShift(_))
        ));

        floor.shifts = vec![ShiftWindow {
            label: "Day".to_string(),
            start_time: "06:00".to_string(),
            end_time: "06:00".to_string(),
        }];
        assert!(matches!(
            validate_floor_settings(&mut floor),
            Err(FloorSettingsError::InvalidShift(_))
        ));
    }

    #[test]
    fn apply_to_local_preserves_station_id_and_path_and_caches_catalog() {
        let mut floor = FloorSettings::default();
        floor.teams_webhook_url = "https://floor.example/hook".to_string();
        floor.summary_poster_station_id = "test-station-1".to_string();
        floor.stations[3].name = "Lab West".to_string();

        let mut local = AppNotificationSettings::default();
        local.station_id = "pdu-lab".to_string();
        local.shared_shift_log_path = r"C:\Shared\PDU".to_string();
        local.teams_webhook_url = "https://old.example/hook".to_string();

        floor.apply_to_local(&mut local);
        assert_eq!(local.station_id, "pdu-lab");
        assert_eq!(local.shared_shift_log_path, r"C:\Shared\PDU");
        assert_eq!(local.teams_webhook_url, "https://floor.example/hook");
        assert_eq!(local.station_name, "Lab West");
        assert_eq!(local.summary_poster_station_id, "test-station-1");
        assert_eq!(local.station_catalog.len(), 4);
        assert_eq!(
            local
                .station_catalog
                .iter()
                .find(|s| s.id == "pdu-lab")
                .map(|s| s.name.as_str()),
            Some("Lab West")
        );
    }

    #[test]
    fn unknown_poster_rejected() {
        let mut floor = FloorSettings::default();
        floor.summary_poster_station_id = "nope".to_string();
        assert!(matches!(
            validate_floor_settings(&mut floor),
            Err(FloorSettingsError::UnknownStation(_))
        ));
    }
}
