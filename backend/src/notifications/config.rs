use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const NOTIFICATIONS_STATION_PATH_ENV: &str = "PDU_NOTIFICATIONS_STATION_PATH";
pub const STABLE_STATION_PATH: &str = r"C:\PDU500\config\notifications\station.json";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Could not read notification config at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Invalid JSON in notification config at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("station.json: station_id is empty")]
    EmptyStationId,
    #[error("Station '{station_id}' has an empty station_name in {settings_path}")]
    EmptyStationName {
        station_id: String,
        settings_path: PathBuf,
    },
    #[error("Unknown station_id '{station_id}' in {settings_path}")]
    UnknownStation {
        station_id: String,
        settings_path: PathBuf,
    },
    #[error("Notifications are disabled in Notification settings")]
    Disabled,
    #[error("Teams webhook URL is empty; open Notification settings to configure it")]
    MissingWebhook,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StationFile {
    pub station_id: String,
    #[serde(default)]
    pub settings_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct EventToggles {
    #[serde(default = "default_true")]
    pub problem: bool,
    #[serde(default = "default_true")]
    pub complete: bool,
    #[serde(default = "default_true")]
    pub changeover: bool,
    #[serde(default = "default_true")]
    pub stuck: bool,
    #[serde(default = "default_true")]
    pub summary: bool,
}

impl Default for EventToggles {
    fn default() -> Self {
        Self {
            problem: true,
            complete: true,
            changeover: true,
            stuck: true,
            summary: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StationSettings {
    pub station_name: String,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_minutes: u32,
    #[serde(default)]
    pub events: EventToggles,
    #[serde(default)]
    pub summary_schedule_times: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SettingsFile {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_destination_name")]
    pub teams_destination_name: String,
    #[serde(default)]
    pub teams_webhook_url: String,
    #[serde(default)]
    pub stations: HashMap<String, StationSettings>,
}

#[derive(Clone)]
pub struct ResolvedConfig {
    pub enabled: bool,
    pub teams_destination_name: String,
    pub teams_webhook_url: String,
    pub station_id: String,
    pub station_name: String,
    pub idle_timeout_minutes: u32,
    pub events: EventToggles,
    pub summary_schedule_times: Vec<String>,
    pub shared_shift_log_path: String,
    pub settings_path: PathBuf,
    pub station_path: PathBuf,
}

impl fmt::Debug for ResolvedConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let webhook = if self.teams_webhook_url.is_empty() {
            "<empty>"
        } else {
            "<redacted>"
        };
        formatter
            .debug_struct("ResolvedConfig")
            .field("enabled", &self.enabled)
            .field("teams_destination_name", &self.teams_destination_name)
            .field("teams_webhook_url", &webhook)
            .field("station_id", &self.station_id)
            .field("station_name", &self.station_name)
            .field("idle_timeout_minutes", &self.idle_timeout_minutes)
            .field("events", &self.events)
            .field("summary_schedule_times", &self.summary_schedule_times)
            .field("shared_shift_log_path", &self.shared_shift_log_path)
            .field("settings_path", &self.settings_path)
            .field("station_path", &self.station_path)
            .finish()
    }
}

fn default_true() -> bool {
    true
}

fn default_idle_timeout() -> u32 {
    30
}

fn default_destination_name() -> String {
    "PDU Testing".to_string()
}

/// Ordered candidate paths, including the environment override when present.
pub fn station_path_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = nonempty_env_path(NOTIFICATIONS_STATION_PATH_ENV) {
        candidates.push(path);
    }
    push_unique(&mut candidates, PathBuf::from(STABLE_STATION_PATH));
    if let Ok(executable) = env::current_exe() {
        if let Some(directory) = executable.parent() {
            push_unique(&mut candidates, directory.join("station.json"));
        }
    }
    if let Ok(directory) = env::current_dir() {
        push_unique(&mut candidates, directory.join("station.json"));
    }
    candidates
}

/// Resolve the station file. An explicit environment override is authoritative,
/// even when missing, so configuration errors identify the requested path.
pub fn default_station_path() -> PathBuf {
    if let Some(path) = nonempty_env_path(NOTIFICATIONS_STATION_PATH_ENV) {
        return path;
    }
    let candidates = station_path_candidates();
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .unwrap_or_else(|| PathBuf::from(STABLE_STATION_PATH))
}

pub fn load_config() -> Result<ResolvedConfig, ConfigError> {
    load_config_from(default_station_path())
}

pub fn load_config_from(path: impl AsRef<Path>) -> Result<ResolvedConfig, ConfigError> {
    let station_path = path.as_ref();
    let station: StationFile = read_json_file(station_path)?;
    let station_id = station.station_id.trim();
    if station_id.is_empty() {
        return Err(ConfigError::EmptyStationId);
    }

    let settings_path = resolve_settings_path(station_path, &station.settings_path);
    let settings: SettingsFile = read_json_file(&settings_path)?;
    let station_settings =
        settings
            .stations
            .get(station_id)
            .cloned()
            .ok_or_else(|| ConfigError::UnknownStation {
                station_id: station_id.to_string(),
                settings_path: settings_path.clone(),
            })?;
    let station_name = station_settings.station_name.trim();
    if station_name.is_empty() {
        return Err(ConfigError::EmptyStationName {
            station_id: station_id.to_string(),
            settings_path,
        });
    }

    Ok(ResolvedConfig {
        enabled: settings.enabled,
        teams_destination_name: match settings.teams_destination_name.trim() {
            "" => default_destination_name(),
            name => name.to_string(),
        },
        teams_webhook_url: settings.teams_webhook_url.trim().to_string(),
        station_id: station_id.to_string(),
        station_name: station_name.to_string(),
        idle_timeout_minutes: station_settings.idle_timeout_minutes,
        events: station_settings.events,
        summary_schedule_times: station_settings.summary_schedule_times,
        shared_shift_log_path: String::new(),
        settings_path,
        station_path: station_path.to_path_buf(),
    })
}

pub fn can_send(config: &ResolvedConfig) -> Result<(), ConfigError> {
    if !config.enabled {
        return Err(ConfigError::Disabled);
    }
    if config.teams_webhook_url.trim().is_empty() {
        return Err(ConfigError::MissingWebhook);
    }
    Ok(())
}

fn resolve_settings_path(station_path: &Path, configured_path: &str) -> PathBuf {
    let configured_path = configured_path.trim();
    let station_directory = station_path.parent().unwrap_or_else(|| Path::new("."));
    if configured_path.is_empty() {
        return station_directory.join("settings.json");
    }
    let configured_path = PathBuf::from(configured_path);
    if configured_path.is_absolute() {
        configured_path
    } else {
        station_directory.join(configured_path)
    }
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ConfigError> {
    let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&raw).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|candidate| candidate == &path) {
        paths.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(path: &Path, value: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, value).unwrap();
    }

    #[test]
    fn relative_settings_path_is_relative_to_station_file_and_schedule_is_retained() {
        let directory = tempdir().unwrap();
        let station = directory.path().join("local/station.json");
        let settings = directory.path().join("local/shared/settings.json");
        write(
            &station,
            r#"{"station_id":"test-station-2","settings_path":"shared/settings.json"}"#,
        );
        write(
            &settings,
            r#"{
                "teams_webhook_url":" https://example.invalid/hook?sig=secret ",
                "stations":{"test-station-2":{
                    "station_name":"Test Station 2",
                    "summary_schedule_times":["15:00","23:00"]
                }}
            }"#,
        );

        let config = load_config_from(&station).unwrap();
        assert_eq!(config.settings_path, settings);
        assert_eq!(config.summary_schedule_times, ["15:00", "23:00"]);
        assert_eq!(config.idle_timeout_minutes, 30);
        assert_eq!(config.events, EventToggles::default());
        assert_eq!(config.teams_destination_name, "PDU Testing");
        assert_eq!(
            config.teams_webhook_url,
            "https://example.invalid/hook?sig=secret"
        );
    }

    #[test]
    fn blank_settings_path_uses_file_next_to_station() {
        let directory = tempdir().unwrap();
        let station = directory.path().join("station.json");
        let settings = directory.path().join("settings.json");
        write(&station, r#"{"station_id":"station-a"}"#);
        write(
            &settings,
            r#"{"enabled":false,"stations":{"station-a":{"station_name":"A"}}}"#,
        );
        let config = load_config_from(&station).unwrap();
        assert_eq!(config.settings_path, settings);
        assert!(matches!(can_send(&config), Err(ConfigError::Disabled)));
    }

    #[test]
    fn unknown_and_empty_station_ids_are_clear_errors() {
        let directory = tempdir().unwrap();
        let station = directory.path().join("station.json");
        write(
            &directory.path().join("settings.json"),
            r#"{"stations":{}}"#,
        );
        write(&station, r#"{"station_id":"missing"}"#);
        assert!(matches!(
            load_config_from(&station),
            Err(ConfigError::UnknownStation { .. })
        ));
        write(&station, r#"{"station_id":"  "}"#);
        assert!(matches!(
            load_config_from(&station),
            Err(ConfigError::EmptyStationId)
        ));
    }

    #[test]
    fn blank_station_name_is_rejected() {
        let directory = tempdir().unwrap();
        let station = directory.path().join("station.json");
        write(&station, r#"{"station_id":"station-a"}"#);
        write(
            &directory.path().join("settings.json"),
            r#"{"stations":{"station-a":{"station_name":"  "}}}"#,
        );
        assert!(matches!(
            load_config_from(&station),
            Err(ConfigError::EmptyStationName { .. })
        ));
    }

    #[test]
    fn resolved_debug_redacts_signed_webhook() {
        let directory = tempdir().unwrap();
        let station = directory.path().join("station.json");
        write(&station, r#"{"station_id":"a"}"#);
        write(
            &directory.path().join("settings.json"),
            r#"{"teams_webhook_url":"https://example.invalid/hook?sig=TOP_SECRET","stations":{"a":{"station_name":"A"}}}"#,
        );
        let debug = format!("{:?}", load_config_from(&station).unwrap());
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("TOP_SECRET"));
        assert!(!debug.contains("example.invalid"));
    }

    #[test]
    fn missing_changeover_toggle_defaults_to_enabled() {
        let toggles: EventToggles =
            serde_json::from_str(r#"{"problem":false,"complete":false}"#).unwrap();

        assert!(toggles.changeover);
    }
}
