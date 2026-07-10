//! Best-effort shared Problem/Complete event log for later floor summaries.
//!
//! Appends hold a small cross-process lock file only while loading, merging and
//! atomically replacing the JSON. A busy or unavailable shared path is reported
//! to the caller and must never block the automation workflow.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

use super::message::EventKind;

pub const SHIFT_LOG_SCHEMA_VERSION: u32 = 1;
/// Floor-wide event file created under the shared root folder.
pub const SHIFT_LOG_FILE_NAME: &str = "shift_log.json";
/// Per-station subfolders for future station-local shared state.
pub const STATIONS_DIR_NAME: &str = "stations";
pub use super::stations::known_station_ids;

/// Station folder ids created under the shared OneDrive root.
pub fn shared_station_ids() -> Vec<&'static str> {
    known_station_ids()
}
const LOCK_WAIT: Duration = Duration::from_secs(2);
const LOCK_POLL: Duration = Duration::from_millis(20);
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ShiftLogError {
    #[error("Could not read shared shift log: {0}")]
    Read(String),
    #[error("Invalid shared shift log JSON: {0}")]
    Parse(String),
    #[error("Could not write shared shift log: {0}")]
    Write(String),
    #[error("Shared shift log is busy; this event was not recorded")]
    Busy,
    #[error("Unsupported shift-log schema version {0}")]
    UnsupportedSchema(u32),
    #[error("Only Problem and Complete events belong in the shared shift log")]
    UnsupportedEventKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShiftLogEventKind {
    Problem,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoggedEvent {
    pub station_id: String,
    pub station_name: String,
    pub kind: ShiftLogEventKind,
    pub timestamp: String,
}

impl LoggedEvent {
    pub fn problem(
        station_id: impl Into<String>,
        station_name: impl Into<String>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self::new(
            station_id,
            station_name,
            ShiftLogEventKind::Problem,
            timestamp,
        )
    }

    pub fn complete(
        station_id: impl Into<String>,
        station_name: impl Into<String>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self::new(
            station_id,
            station_name,
            ShiftLogEventKind::Complete,
            timestamp,
        )
    }

    pub fn from_notification_kind(
        station_id: impl Into<String>,
        station_name: impl Into<String>,
        kind: EventKind,
        timestamp: impl Into<String>,
    ) -> Result<Self, ShiftLogError> {
        let kind = match kind {
            EventKind::Problem => ShiftLogEventKind::Problem,
            EventKind::Complete => ShiftLogEventKind::Complete,
            EventKind::TestPing | EventKind::Stuck | EventKind::Summary => {
                return Err(ShiftLogError::UnsupportedEventKind)
            }
        };
        Ok(Self::new(station_id, station_name, kind, timestamp))
    }

    fn new(
        station_id: impl Into<String>,
        station_name: impl Into<String>,
        kind: ShiftLogEventKind,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            station_id: station_id.into(),
            station_name: station_name.into(),
            kind,
            timestamp: timestamp.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShiftLog {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub events: Vec<LoggedEvent>,
    #[serde(default)]
    pub last_summary_at: Option<String>,
    /// Station name that posted the last floor summary (any station may post early).
    #[serde(default)]
    pub last_summary_by: Option<String>,
    /// Shift label used for the last floor summary, when known.
    #[serde(default)]
    pub last_summary_shift: Option<String>,
}

impl Default for ShiftLog {
    fn default() -> Self {
        Self {
            schema_version: SHIFT_LOG_SCHEMA_VERSION,
            events: Vec::new(),
            last_summary_at: None,
            last_summary_by: None,
            last_summary_shift: None,
        }
    }
}

/// Resolve a configured shared root (preferred) or legacy JSON file path to the
/// floor-wide `shift_log.json` path. Empty config means shared logging is off.
pub fn resolve_shift_log_file(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if configured.is_empty() {
        return None;
    }
    let path = PathBuf::from(configured);
    if looks_like_json_file(&path) {
        Some(path)
    } else {
        Some(path.join(SHIFT_LOG_FILE_NAME))
    }
}

/// Shared root directory for a configured path (folder or legacy JSON file).
pub fn shared_root_directory(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if configured.is_empty() {
        return None;
    }
    let path = PathBuf::from(configured);
    if looks_like_json_file(&path) {
        path.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .or_else(|| Some(PathBuf::from(".")))
    } else {
        Some(path)
    }
}

/// Create the coordinated OneDrive/network layout under the chosen folder:
///
/// ```text
/// <shared-root>/
///   shift_log.json
///   stations/
///     test-station-1/
///     test-station-2/
///     test-station-3/
///     test-station-4/
/// ```
///
/// The JSON file is not pre-created (first append publishes it). Empty config is
/// a no-op. Soft-fail callers should treat errors as advisory only.
pub fn ensure_shared_root_layout(configured: &str) -> Result<PathBuf, ShiftLogError> {
    let Some(root) = shared_root_directory(configured) else {
        return Err(ShiftLogError::Write(
            "Shared folder path is empty".to_string(),
        ));
    };
    fs::create_dir_all(&root)
        .map_err(|error| ShiftLogError::Write(format!("{}: {error}", root.display())))?;
    let stations_root = root.join(STATIONS_DIR_NAME);
    fs::create_dir_all(&stations_root)
        .map_err(|error| ShiftLogError::Write(format!("{}: {error}", stations_root.display())))?;
    for station_id in shared_station_ids() {
        let station_dir = stations_root.join(station_id);
        fs::create_dir_all(&station_dir)
            .map_err(|error| ShiftLogError::Write(format!("{}: {error}", station_dir.display())))?;
    }
    Ok(root)
}

/// Combined end-of-shift totals across known stations for Teams posting.
pub fn format_floor_summary(
    log: &ShiftLog,
    timestamp: &str,
    known_stations: &[(String, String)],
    shift_label: Option<&str>,
    _operator_name: Option<&str>,
    posted_by_station: Option<&str>,
) -> String {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct StationTotals {
        name: String,
        completes: u32,
        problems: u32,
        stuck: u32,
    }

    let mut by_station: BTreeMap<String, StationTotals> = BTreeMap::new();
    for (id, name) in known_stations {
        by_station.insert(
            id.clone(),
            StationTotals {
                name: name.clone(),
                ..StationTotals::default()
            },
        );
    }

    for event in &log.events {
        let entry = by_station
            .entry(event.station_id.clone())
            .or_insert_with(|| StationTotals {
                name: event.station_name.clone(),
                ..StationTotals::default()
            });
        if entry.name.is_empty() {
            entry.name = event.station_name.clone();
        }
        match event.kind {
            ShiftLogEventKind::Problem => entry.problems += 1,
            ShiftLogEventKind::Complete => entry.completes += 1,
        }
    }

    let mut total_c = 0u32;
    let mut total_p = 0u32;
    let mut total_s = 0u32;
    let mut stations_with_work = 0u32;
    for totals in by_station.values() {
        total_c += totals.completes;
        total_p += totals.problems;
        total_s += totals.stuck;
        if totals.completes + totals.problems + totals.stuck > 0 {
            stations_with_work += 1;
        }
    }
    let station_count = known_stations.len().max(by_station.len());
    let total_events = total_c + total_p + total_s;

    let headline = match shift_label.map(str::trim).filter(|value| !value.is_empty()) {
        Some(label) => format!("📊 End of shift — {label}"),
        None => "📊 End of shift — all stations".to_string(),
    };

    let mut lines = vec![
        headline,
        format!(
            "TOTALS (combined)\n✅ Completed: {total_c}\n🔴 Problems: {total_p}\n🟠 Stuck: {total_s}\nΣ Events: {total_events}"
        ),
        format!("Stations\n{stations_with_work} of {station_count} had activity this shift"),
    ];

    // `posted_by_station` is kept on the API for callers/logging but is not shown on the card.
    let _ = posted_by_station;

    if log.events.is_empty() {
        lines.push(String::from(
            "No problem / complete events logged yet for this shift period.",
        ));
    } else {
        let mut station_lines = String::from("By station\n");
        for (id, totals) in &by_station {
            let name = if totals.name.is_empty() {
                id.as_str()
            } else {
                totals.name.as_str()
            };
            if totals.completes + totals.problems + totals.stuck == 0 {
                station_lines.push_str(&format!("• {name}: —\n"));
            } else {
                station_lines.push_str(&format!(
                    "• {name}: ✅{} 🔴{} 🟠{}\n",
                    totals.completes, totals.problems, totals.stuck
                ));
            }
        }
        lines.push(station_lines.trim_end().to_string());
    }

    if let Some(prev) = &log.last_summary_at {
        lines.push(format!("Previous end-of-shift post\n{prev}"));
    }

    lines.push(timestamp.to_string());
    lines.join("\n\n")
}

/// After a successful Teams post: record summary time/station and clear event ledger.
/// All floor PCs that share this log can see the post was already sent.
pub fn mark_summary_and_clear(
    path: &Path,
    timestamp: &str,
    posted_by: &str,
    shift_label: &str,
) -> Result<(), ShiftLogError> {
    if path.as_os_str().is_empty() {
        return Err(ShiftLogError::Write(
            "Shared shift log path is empty".to_string(),
        ));
    }
    let configured = path.to_string_lossy();
    let log_path = resolve_shift_log_file(&configured)
        .ok_or_else(|| ShiftLogError::Write("Shared shift log path is empty".to_string()))?;
    let _lock = SharedLogLock::acquire(&log_path)?;
    let mut log = load_shift_log(&log_path)?;
    log.last_summary_at = Some(timestamp.to_string());
    log.last_summary_by = {
        let by = posted_by.trim();
        if by.is_empty() {
            None
        } else {
            Some(by.to_string())
        }
    };
    log.last_summary_shift = {
        let label = shift_label.trim();
        if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        }
    };
    log.events.clear();
    write_shift_log(&log_path, &log)
}

/// Append one accepted Problem/Complete event. `path` may be either the shared
/// root folder (preferred) or a legacy direct path to `shift_log.json`.
/// An empty path is a configured no-op until deployment supplies a shared folder.
pub fn append_event(path: &Path, event: LoggedEvent) -> Result<(), ShiftLogError> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    let configured = path.to_string_lossy();
    let log_path = resolve_shift_log_file(&configured)
        .ok_or_else(|| ShiftLogError::Write("Shared shift log path is empty".to_string()))?;
    let _ = ensure_shared_root_layout(&configured);
    ensure_parent_directory(&log_path).map_err(ShiftLogError::Write)?;
    let _lock = SharedLogLock::acquire(&log_path)?;
    let mut log = load_shift_log(&log_path)?;
    log.events.push(event);
    write_shift_log(&log_path, &log)
}

fn looks_like_json_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

/// Load a complete, atomically published snapshot. Missing files are an empty
/// schema-v1 log; corrupt or unsupported files are never silently overwritten.
pub fn load_shift_log(path: &Path) -> Result<ShiftLog, ShiftLogError> {
    if path.as_os_str().is_empty() {
        return Ok(ShiftLog::default());
    }
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ShiftLog::default())
        }
        Err(error) => return Err(ShiftLogError::Read(format!("{}: {error}", path.display()))),
    };
    let log: ShiftLog = serde_json::from_str(&raw)
        .map_err(|error| ShiftLogError::Parse(format!("{}: {error}", path.display())))?;
    if log.schema_version != SHIFT_LOG_SCHEMA_VERSION {
        return Err(ShiftLogError::UnsupportedSchema(log.schema_version));
    }
    Ok(log)
}

fn write_shift_log(path: &Path, log: &ShiftLog) -> Result<(), ShiftLogError> {
    if log.schema_version != SHIFT_LOG_SCHEMA_VERSION {
        return Err(ShiftLogError::UnsupportedSchema(log.schema_version));
    }
    let raw =
        serde_json::to_vec_pretty(log).map_err(|error| ShiftLogError::Write(error.to_string()))?;
    atomic_replace(path, &raw).map_err(ShiftLogError::Write)
}

struct SharedLogLock {
    path: PathBuf,
}

impl SharedLogLock {
    fn acquire(log_path: &Path) -> Result<Self, ShiftLogError> {
        let path = lock_path_for(log_path);
        let started = Instant::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    if let Err(error) = writeln!(file, "pid={}", std::process::id()) {
                        drop(file);
                        let _ = fs::remove_file(&path);
                        return Err(ShiftLogError::Write(format!("{}: {error}", path.display())));
                    }
                    return Ok(Self { path });
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists
                        || (error.kind() == std::io::ErrorKind::PermissionDenied
                            && path.exists()) =>
                {
                    // A process crash can leave the marker behind. Normal appends
                    // hold it for milliseconds, so only a conservatively old
                    // marker is eligible for recovery.
                    //
                    // On Windows, concurrent create_new can report Access Denied
                    // while another writer holds the lock file — treat that as
                    // contention when the marker is present.
                    if lock_is_stale(&path) {
                        match fs::remove_file(&path) {
                            Ok(()) => continue,
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                            Err(_) => {}
                        }
                    }
                    if started.elapsed() >= LOCK_WAIT {
                        return Err(ShiftLogError::Busy);
                    }
                    thread::sleep(LOCK_POLL);
                }
                Err(error) => {
                    return Err(ShiftLogError::Write(format!("{}: {error}", path.display())))
                }
            }
        }
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

impl Drop for SharedLogLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("shift_log.json");
    path.with_file_name(format!("{file_name}.lock"))
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

    // This is a best-effort shared activity log, not the source of truth for
    // automation. Flush userspace buffers but avoid a slow network/disk fsync
    // while holding the cross-station lock; rename still publishes whole JSON.
    if let Err(error) = temp_file
        .write_all(contents)
        .and_then(|_| temp_file.flush())
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
        .unwrap_or("shift_log.json");
    path.with_file_name(format!(
        ".{file_name}.{}.{}.{}",
        std::process::id(),
        counter,
        suffix
    ))
}

fn default_schema_version() -> u32 {
    SHIFT_LOG_SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use tempfile::tempdir;

    #[test]
    fn appending_two_stations_loads_merges_and_persists() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("shared/shift_log.json");
        append_event(
            &path,
            LoggedEvent::problem("test-station-1", "Test Station 1", "t1"),
        )
        .unwrap();
        append_event(
            &path,
            LoggedEvent::complete("test-station-2", "Test Station 2", "t2"),
        )
        .unwrap();

        let log = load_shift_log(&path).unwrap();
        assert_eq!(log.schema_version, SHIFT_LOG_SCHEMA_VERSION);
        assert_eq!(log.events.len(), 2);
        assert_eq!(log.events[0].kind, ShiftLogEventKind::Problem);
        assert_eq!(log.events[1].kind, ShiftLogEventKind::Complete);
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""kind": "problem""#));
        assert!(raw.contains(r#""kind": "complete""#));
        assert!(!lock_path_for(&path).exists());
    }

    #[test]
    fn concurrent_writers_do_not_lose_events() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("shared-root");
        ensure_shared_root_layout(root.to_str().unwrap()).unwrap();
        let path = Arc::new(root);
        let writer_count = 8;
        let barrier = Arc::new(Barrier::new(writer_count));
        let handles = (0..writer_count)
            .map(|index| {
                let path = Arc::clone(&path);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    // Retry briefly on Busy so Windows lock contention does not
                    // flake the coordination test under high parallel load.
                    let event = LoggedEvent::complete(
                        format!("test-station-{}", index % 4 + 1),
                        format!("Test Station {}", index % 4 + 1),
                        format!("t{index}"),
                    );
                    for attempt in 0..20 {
                        match append_event(path.as_ref(), event.clone()) {
                            Ok(()) => return Ok(()),
                            Err(ShiftLogError::Busy) if attempt + 1 < 20 => {
                                thread::sleep(Duration::from_millis(25));
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    Err(ShiftLogError::Busy)
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap().unwrap();
        }
        let log = load_shift_log(&path.join(SHIFT_LOG_FILE_NAME)).unwrap();
        assert_eq!(log.events.len(), writer_count);
    }

    #[test]
    fn held_lock_fails_quickly_and_does_not_modify_log() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("shift_log.json");
        fs::write(lock_path_for(&path), "held").unwrap();
        let started = Instant::now();

        let result = append_event(
            &path,
            LoggedEvent::problem("test-station-1", "Test Station 1", "t1"),
        );
        assert_eq!(result, Err(ShiftLogError::Busy));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(!path.exists());
    }

    #[test]
    fn stale_lock_from_crashed_writer_is_recovered() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("shift_log.json");
        let lock_path = lock_path_for(&path);
        let lock_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .unwrap();
        lock_file
            .set_times(
                fs::FileTimes::new()
                    .set_modified(std::time::SystemTime::now() - Duration::from_secs(31)),
            )
            .unwrap();
        drop(lock_file);

        append_event(
            &path,
            LoggedEvent::complete("test-station-4", "Test Station 4", "t1"),
        )
        .unwrap();

        assert_eq!(load_shift_log(&path).unwrap().events.len(), 1);
        assert!(!lock_path.exists());
    }

    #[test]
    fn corrupt_log_is_preserved_instead_of_overwritten() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("shift_log.json");
        fs::write(&path, "{not-json").unwrap();
        assert!(matches!(
            append_event(
                &path,
                LoggedEvent::problem("test-station-1", "Test Station 1", "t1")
            ),
            Err(ShiftLogError::Parse(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{not-json");
        assert!(!lock_path_for(&path).exists());
    }

    #[test]
    fn floor_summary_includes_lab_and_totals() {
        let mut log = ShiftLog::default();
        log.events.push(LoggedEvent::complete(
            "test-station-1",
            "Test Station 1",
            "t1",
        ));
        log.events
            .push(LoggedEvent::problem("pdu-lab", "PDU Lab", "t2"));
        let known = super::super::stations::known_stations_owned();
        let body = format_floor_summary(&log, "now", &known, Some("Day"), None, Some("PDU Lab"));
        assert!(body.contains("End of shift — Day"));
        assert!(body.contains("Completed: 1"));
        assert!(body.contains("Problems: 1"));
        assert!(body.contains("PDU Lab"));
        assert!(!body.contains("Operator\n"));
        assert!(!body.contains("Posted by"));
    }

    #[test]
    fn blank_path_is_noop_and_nonproduction_kinds_are_rejected() {
        append_event(
            Path::new(""),
            LoggedEvent::problem("test-station-1", "Test Station 1", "t1"),
        )
        .unwrap();
        assert_eq!(
            LoggedEvent::from_notification_kind(
                "test-station-1",
                "Test Station 1",
                EventKind::TestPing,
                "now"
            ),
            Err(ShiftLogError::UnsupportedEventKind)
        );
    }

    #[test]
    fn shared_root_layout_creates_station_folders_and_append_uses_root_file() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("OneDrive/hidden-notifications");
        let created = ensure_shared_root_layout(root.to_str().unwrap()).unwrap();
        assert_eq!(created, root);
        for station_id in shared_station_ids() {
            assert!(root.join(STATIONS_DIR_NAME).join(station_id).is_dir());
        }
        assert!(root.join(STATIONS_DIR_NAME).join("pdu-lab").is_dir());
        assert!(!root.join(SHIFT_LOG_FILE_NAME).exists());

        append_event(
            &root,
            LoggedEvent::problem("test-station-1", "Test Station 1", "t1"),
        )
        .unwrap();
        let log = load_shift_log(&root.join(SHIFT_LOG_FILE_NAME)).unwrap();
        assert_eq!(log.events.len(), 1);
        assert_eq!(
            resolve_shift_log_file(root.to_str().unwrap()).unwrap(),
            root.join(SHIFT_LOG_FILE_NAME)
        );
    }

    #[test]
    fn legacy_json_file_path_still_resolves_and_appends() {
        let directory = tempdir().unwrap();
        let file = directory.path().join("custom_shift_log.json");
        append_event(
            &file,
            LoggedEvent::complete("test-station-2", "Test Station 2", "t1"),
        )
        .unwrap();
        assert_eq!(load_shift_log(&file).unwrap().events.len(), 1);
        assert!(directory
            .path()
            .join(STATIONS_DIR_NAME)
            .join("test-station-1")
            .is_dir());
    }

    #[test]
    fn atomic_replacements_leave_no_work_or_lock_files() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("shift_log.json");
        append_event(
            &path,
            LoggedEvent::problem("test-station-1", "Test Station 1", "t1"),
        )
        .unwrap();
        append_event(
            &path,
            LoggedEvent::complete("test-station-1", "Test Station 1", "t2"),
        )
        .unwrap();

        let names = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        // Layout also creates stations/ beside a legacy JSON file path.
        assert!(names.contains(&std::ffi::OsString::from("shift_log.json")));
        assert!(names.contains(&std::ffi::OsString::from(STATIONS_DIR_NAME)));
        assert_eq!(names.len(), 2);
    }
}
