//! Manual end-of-shift summary posting.
//! Any station with a configured webhook and shared folder may post early;
//! the shared log records who posted so other stations skip a second post.

use serde::{Deserialize, Serialize};

use super::app_settings::{catalog_from_local, load_app_settings, AppNotificationSettings};
use super::config::can_send;
use super::message::now_timestamp;
use super::shift_log::{
    format_floor_summary, load_shift_log, mark_summary_and_clear, resolve_shift_log_file,
};
use super::stations::{
    is_known_station_id, known_stations_owned, station_name_for_id,
    DEFAULT_SUMMARY_POSTER_STATION_ID,
};
use super::teams::TeamsClient;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SummaryError {
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostShiftSummaryRequest {
    #[serde(default)]
    pub shift_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftSummaryPreview {
    pub text: String,
    pub is_summary_poster: bool,
    pub poster_station_id: String,
    pub poster_station_name: String,
    pub event_count: usize,
    pub shared_folder_configured: bool,
    /// True when a floor summary was already posted and no new events have arrived.
    #[serde(default)]
    pub already_posted: bool,
    #[serde(default)]
    pub last_summary_at: Option<String>,
    #[serde(default)]
    pub last_summary_by: Option<String>,
    #[serde(default)]
    pub last_summary_shift: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftSummaryResult {
    pub message: String,
    pub text: String,
}

pub fn preview_shift_summary(
    shift_label: Option<&str>,
) -> Result<ShiftSummaryPreview, SummaryError> {
    let settings = load_app_settings().map_err(|error| SummaryError::Message(error.to_string()))?;
    let poster_id = poster_station_id(&settings);
    let is_poster = settings.station_id.trim() == poster_id;
    let shared = settings.shared_shift_log_path.trim();

    let (event_count, already_posted, last_summary_at, last_summary_by, last_summary_shift) =
        if shared.is_empty() {
            (0, false, None, None, None)
        } else {
            let log_path = resolve_shift_log_file(shared).ok_or_else(|| {
                SummaryError::Message("Shared shift log path is empty".to_string())
            })?;
            match load_shift_log(&log_path) {
                Ok(log) => {
                    let already = log.last_summary_at.is_some() && log.events.is_empty();
                    (
                        log.events.len(),
                        already,
                        log.last_summary_at,
                        log.last_summary_by,
                        log.last_summary_shift,
                    )
                }
                Err(_) => (0, false, None, None, None),
            }
        };

    let text = build_summary_text(&settings, shift_label.unwrap_or("").trim())?;

    Ok(ShiftSummaryPreview {
        text,
        is_summary_poster: is_poster,
        poster_station_id: poster_id.to_string(),
        poster_station_name: display_name_for_station(&settings, poster_id),
        event_count,
        shared_folder_configured: !shared.is_empty(),
        already_posted,
        last_summary_at,
        last_summary_by,
        last_summary_shift,
    })
}

pub fn post_shift_summary(
    request: &PostShiftSummaryRequest,
) -> Result<ShiftSummaryResult, SummaryError> {
    let settings = load_app_settings().map_err(|error| SummaryError::Message(error.to_string()))?;
    if !settings.events.summary {
        return Err(SummaryError::Message(
            "End-of-shift summary is disabled in notification settings.".to_string(),
        ));
    }
    let resolved = settings.to_resolved_config();
    can_send(&resolved).map_err(|error| SummaryError::Message(error.to_string()))?;

    let shared = settings.shared_shift_log_path.trim();
    if shared.is_empty() {
        return Err(SummaryError::Message(
            "Configure the shared OneDrive folder before posting end of shift.".to_string(),
        ));
    }

    let log_path = resolve_shift_log_file(shared)
        .ok_or_else(|| SummaryError::Message("Shared shift log path is empty".to_string()))?;
    if let Ok(log) = load_shift_log(&log_path) {
        if log.last_summary_at.is_some() && log.events.is_empty() {
            let by = log
                .last_summary_by
                .as_deref()
                .filter(|value| !value.is_empty())
                .unwrap_or("another station");
            let when = log.last_summary_at.as_deref().unwrap_or("recently");
            return Err(SummaryError::Message(format!(
                "End-of-shift summary was already posted by {by} at {when}. No new floor events have been logged since."
            )));
        }
    }

    let shift_label = request.shift_label.trim();
    let text = build_summary_text(&settings, shift_label)?;

    let client = TeamsClient::new().map_err(|error| SummaryError::Message(error.to_string()))?;
    client
        .post_text(&resolved.teams_webhook_url, &text)
        .map_err(|error| SummaryError::Message(format!("Summary card delivery failed: {error}")))?;

    let timestamp = now_timestamp();
    mark_summary_and_clear(
        std::path::Path::new(shared),
        &timestamp,
        settings.station_name.trim(),
        shift_label,
    )
    .map_err(|error| {
        SummaryError::Message(format!(
            "Summary was posted, but the shared log could not be cleared: {error}"
        ))
    })?;

    Ok(ShiftSummaryResult {
        message: format!(
            "End-of-shift summary posted by {}. Other stations will see it was already sent.",
            settings.station_name.trim()
        ),
        text,
    })
}

fn poster_station_id(settings: &AppNotificationSettings) -> &str {
    let poster = settings.summary_poster_station_id.trim();
    if poster.is_empty() || !is_known_station_id(poster) {
        DEFAULT_SUMMARY_POSTER_STATION_ID
    } else {
        poster
    }
}

fn catalog_pairs(settings: &AppNotificationSettings) -> Vec<(String, String)> {
    let shared = settings.shared_shift_log_path.trim();
    if !shared.is_empty() {
        if let Ok(Some(floor)) = super::try_load_floor_settings(shared) {
            return floor.catalog_pairs();
        }
    }
    // Offline / missing floor: use last-known-good local catalog cache.
    let local = catalog_from_local(settings);
    if !local.is_empty() {
        return local
            .into_iter()
            .map(|entry| (entry.id, entry.name))
            .collect();
    }
    known_stations_owned()
}

fn display_name_for_station(settings: &AppNotificationSettings, station_id: &str) -> String {
    catalog_pairs(settings)
        .into_iter()
        .find(|(id, _)| id == station_id)
        .map(|(_, name)| name)
        .unwrap_or_else(|| station_name_for_id(station_id).to_string())
}

fn included_stations(settings: &AppNotificationSettings) -> Vec<(String, String)> {
    let all = catalog_pairs(settings);
    if settings.summary_included_station_ids.is_empty() {
        return all;
    }
    let mut selected = Vec::new();
    for id in &settings.summary_included_station_ids {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        if let Some((known_id, name)) = all.iter().find(|(known_id, _)| known_id == id) {
            selected.push((known_id.clone(), name.clone()));
        }
    }
    if selected.is_empty() {
        all
    } else {
        selected
    }
}

fn build_summary_text(
    settings: &AppNotificationSettings,
    shift_label: &str,
) -> Result<String, SummaryError> {
    let shared = settings.shared_shift_log_path.trim();
    let log = if shared.is_empty() {
        super::shift_log::ShiftLog::default()
    } else {
        let log_path = resolve_shift_log_file(shared)
            .ok_or_else(|| SummaryError::Message("Shared shift log path is empty".to_string()))?;
        load_shift_log(&log_path).map_err(|error| SummaryError::Message(error.to_string()))?
    };

    let known = included_stations(settings);
    let label = if shift_label.is_empty() {
        settings
            .shifts
            .first()
            .map(|shift| shift.label.as_str())
            .unwrap_or("")
    } else {
        shift_label
    };

    Ok(format_floor_summary(
        &log,
        &now_timestamp(),
        &known,
        if label.is_empty() { None } else { Some(label) },
        None,
        Some(settings.station_name.trim()),
    ))
}
