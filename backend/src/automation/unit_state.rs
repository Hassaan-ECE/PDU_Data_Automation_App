use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Local;
use serde::{Deserialize, Serialize};

use super::processors::ProcessorResult;

pub const UNIT_STATE_FILE: &str = "unit_state.json";
const UNIT_STATE_LOCK_FILE: &str = "unit_state.lock";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitState {
    pub schema_version: u8,
    #[serde(default)]
    pub tasks: BTreeMap<String, UnitTaskState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitTaskState {
    pub task_id: String,
    pub state: String,
    #[serde(default)]
    pub code: Option<u8>,
    #[serde(default)]
    pub source_csv_path: Option<String>,
    #[serde(default)]
    pub csv_fingerprint: Option<String>,
    #[serde(default)]
    pub processed_at: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub accepted: TaskAcceptance,
    #[serde(default)]
    pub audit_log: Vec<UnitAuditEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskAcceptance {
    #[serde(default)]
    pub accepted: bool,
    #[serde(default)]
    pub accepted_at: Option<String>,
    #[serde(default)]
    pub accepted_by: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitAuditEntry {
    pub at: String,
    pub event: String,
    pub message: String,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub code: Option<u8>,
    #[serde(default)]
    pub source_csv_path: Option<String>,
    #[serde(default)]
    pub csv_fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskStateSeed {
    pub task_id: String,
    pub state: String,
    pub source_csv_path: Option<String>,
    pub csv_fingerprint: Option<String>,
}

impl Default for UnitState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            tasks: BTreeMap::new(),
        }
    }
}

impl UnitTaskState {
    pub fn is_print_ready(&self) -> bool {
        self.state == "pass" || self.accepted.accepted
    }

    pub fn already_processed_fingerprint(&self) -> Option<&str> {
        if self.state == "pass" {
            return self.csv_fingerprint.as_deref();
        }

        None
    }
}

pub fn state_path(unit_folder: &Path) -> PathBuf {
    unit_folder.join(UNIT_STATE_FILE)
}

pub fn load_unit_state(unit_folder: &Path) -> io::Result<Option<UnitState>> {
    let _lock = UnitStateLock::acquire(unit_folder)?;

    load_unit_state_unlocked(unit_folder)
}

pub fn load_or_default(unit_folder: &Path) -> io::Result<UnitState> {
    Ok(load_unit_state(unit_folder)?.unwrap_or_default())
}

#[cfg(test)]
pub fn save_unit_state(unit_folder: &Path, state: &UnitState) -> io::Result<()> {
    let _lock = UnitStateLock::acquire(unit_folder)?;

    save_unit_state_unlocked(unit_folder, state)
}

pub fn load_or_ensure_task_entries(
    unit_folder: &Path,
    seeds: &[TaskStateSeed],
) -> io::Result<UnitState> {
    let _lock = UnitStateLock::acquire(unit_folder)?;
    let mut state = load_or_default_unlocked(unit_folder)?;

    if ensure_task_entries(&mut state, seeds) {
        save_unit_state_unlocked(unit_folder, &state)?;
    }

    Ok(state)
}

fn load_unit_state_unlocked(unit_folder: &Path) -> io::Result<Option<UnitState>> {
    let path = state_path(unit_folder);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("failed to read {}: {error}", path.display()),
            ))
        }
    };

    let state = serde_json::from_str::<UnitState>(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse {}: {error}", path.display()),
        )
    })?;

    Ok(Some(state))
}

fn load_or_default_unlocked(unit_folder: &Path) -> io::Result<UnitState> {
    Ok(load_unit_state_unlocked(unit_folder)?.unwrap_or_default())
}

fn save_unit_state_unlocked(unit_folder: &Path, state: &UnitState) -> io::Result<()> {
    let path = state_path(unit_folder);
    let temp_path = unit_folder.join(format!("{UNIT_STATE_FILE}.tmp"));
    let backup_path = unit_folder.join(format!("{UNIT_STATE_FILE}.bak"));
    let content = serde_json::to_string_pretty(state).map_err(io::Error::other)?;

    fs::write(&temp_path, content)?;

    if path.exists() {
        fs::copy(&path, &backup_path)?;
        fs::remove_file(&path)?;
    }

    match fs::rename(&temp_path, &path) {
        Ok(()) => Ok(()),
        Err(error) => {
            if backup_path.is_file() {
                let _ = fs::copy(&backup_path, &path);
            }
            let _ = fs::remove_file(&temp_path);
            Err(error)
        }
    }
}

pub fn ensure_task_entries(state: &mut UnitState, seeds: &[TaskStateSeed]) -> bool {
    let mut changed = false;

    for seed in seeds {
        if let Some(entry) = state.tasks.get_mut(&seed.task_id) {
            if should_refresh_from_scan(&entry.state) {
                let next_state = seed.state.clone();
                if entry.state != next_state {
                    entry.state = next_state;
                    entry.code = None;
                    changed = true;
                }
            }

            if entry.source_csv_path.is_none() && seed.source_csv_path.is_some() {
                entry.source_csv_path = seed.source_csv_path.clone();
                changed = true;
            }

            if entry.csv_fingerprint.is_none() && seed.csv_fingerprint.is_some() {
                entry.csv_fingerprint = seed.csv_fingerprint.clone();
                changed = true;
            }

            continue;
        }

        let mut entry = UnitTaskState {
            task_id: seed.task_id.clone(),
            state: seed.state.clone(),
            code: None,
            source_csv_path: seed.source_csv_path.clone(),
            csv_fingerprint: seed.csv_fingerprint.clone(),
            processed_at: None,
            result: None,
            accepted: TaskAcceptance::default(),
            audit_log: Vec::new(),
        };

        entry.audit_log.push(UnitAuditEntry {
            at: now_string(),
            event: "initialized_from_scan".to_string(),
            message: format!("Initialized task state as '{}'", seed.state),
            state: Some(seed.state.clone()),
            code: None,
            source_csv_path: seed.source_csv_path.clone(),
            csv_fingerprint: seed.csv_fingerprint.clone(),
        });

        state.tasks.insert(seed.task_id.clone(), entry);
        changed = true;
    }

    changed
}

pub fn record_processor_result(
    unit_folder: &Path,
    task_id: &str,
    result: &ProcessorResult,
) -> io::Result<UnitState> {
    let _lock = UnitStateLock::acquire(unit_folder)?;
    let mut state = load_or_default_unlocked(unit_folder)?;
    let at = now_string();
    let entry = state
        .tasks
        .entry(task_id.to_string())
        .or_insert_with(|| UnitTaskState {
            task_id: task_id.to_string(),
            state: "off".to_string(),
            code: None,
            source_csv_path: None,
            csv_fingerprint: None,
            processed_at: None,
            result: None,
            accepted: TaskAcceptance::default(),
            audit_log: Vec::new(),
        });

    entry.state = result.state.clone();
    entry.code = Some(result.code);
    entry.source_csv_path = result.source_csv_path.clone();
    entry.csv_fingerprint = result.csv_fingerprint.clone();
    entry.processed_at = Some(at.clone());
    entry.result = Some(result.message.clone());
    entry.audit_log.push(UnitAuditEntry {
        at,
        event: if result
            .message
            .contains("already processed from the same CSV")
        {
            "idempotent_skip".to_string()
        } else {
            "processed".to_string()
        },
        message: result.message.clone(),
        state: Some(result.state.clone()),
        code: Some(result.code),
        source_csv_path: result.source_csv_path.clone(),
        csv_fingerprint: result.csv_fingerprint.clone(),
    });

    save_unit_state_unlocked(unit_folder, &state)?;

    Ok(state)
}

pub fn now_string() -> String {
    Local::now().to_rfc3339()
}

fn should_refresh_from_scan(state: &str) -> bool {
    matches!(state, "off" | "detected")
}

struct UnitStateLock {
    path: PathBuf,
}

impl UnitStateLock {
    fn acquire(unit_folder: &Path) -> io::Result<Self> {
        let path = unit_state_lock_path(unit_folder);
        let started = Instant::now();
        let max_wait = Duration::from_secs(5);

        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    if let Err(error) = writeln!(file, "pid={}", std::process::id()) {
                        let _ = fs::remove_file(&path);
                        return Err(error);
                    }

                    return Ok(Self { path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if started.elapsed() >= max_wait {
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            format!(
                                "{} is locked by another app operation",
                                state_path(unit_folder).display()
                            ),
                        ));
                    }

                    thread::sleep(Duration::from_millis(100));
                }
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("failed to create {}: {error}", path.display()),
                    ))
                }
            }
        }
    }
}

impl Drop for UnitStateLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn unit_state_lock_path(unit_folder: &Path) -> PathBuf {
    unit_folder.join(UNIT_STATE_LOCK_FILE)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn corrupt_unit_state_returns_invalid_data() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("unit");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        fs::write(state_path(&unit_folder), "{not valid json").expect("write corrupt state");

        let error = load_or_default(&unit_folder).expect_err("corrupt state should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(UNIT_STATE_FILE));
    }

    #[test]
    fn concurrent_state_writes_preserve_all_task_updates() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = Arc::new(temp.path().join("unit"));
        fs::create_dir_all(unit_folder.as_ref()).expect("unit folder");
        let writer_count = 8;
        let barrier = Arc::new(Barrier::new(writer_count));
        let handles = (0..writer_count)
            .map(|index| {
                let unit_folder = Arc::clone(&unit_folder);
                let barrier = Arc::clone(&barrier);

                thread::spawn(move || {
                    barrier.wait();
                    record_processor_result(
                        unit_folder.as_ref(),
                        &format!("task-{index}"),
                        &processor_result(index),
                    )
                    .expect("record processor result");
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().expect("writer thread");
        }

        let state = load_or_default(unit_folder.as_ref()).expect("load state");

        for index in 0..writer_count {
            let task_id = format!("task-{index}");
            let expected_fingerprint = format!("fingerprint-{index}");
            let entry = state.tasks.get(&task_id).expect("task entry");

            assert_eq!(entry.state, "pass");
            assert_eq!(
                entry.csv_fingerprint.as_deref(),
                Some(expected_fingerprint.as_str())
            );
        }

        assert!(
            !unit_state_lock_path(unit_folder.as_ref()).exists(),
            "state lock should be removed after writes"
        );
    }

    #[test]
    fn save_unit_state_keeps_temp_and_backup_behavior() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("unit");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let path = state_path(&unit_folder);
        let temp_path = unit_folder.join(format!("{UNIT_STATE_FILE}.tmp"));
        let backup_path = unit_folder.join(format!("{UNIT_STATE_FILE}.bak"));
        let mut state = UnitState::default();

        state
            .tasks
            .insert("first".to_string(), task_state("first", "pass"));
        save_unit_state(&unit_folder, &state).expect("first save");
        let first_content = fs::read_to_string(&path).expect("first state content");

        state
            .tasks
            .insert("second".to_string(), task_state("second", "fail"));
        save_unit_state(&unit_folder, &state).expect("second save");

        assert_eq!(
            fs::read_to_string(&backup_path).expect("backup content"),
            first_content
        );
        assert!(
            !temp_path.exists(),
            "temporary state file should be renamed away"
        );

        let reloaded = load_or_default(&unit_folder).expect("reloaded state");

        assert!(reloaded.tasks.contains_key("first"));
        assert!(reloaded.tasks.contains_key("second"));
    }

    fn processor_result(index: usize) -> ProcessorResult {
        ProcessorResult {
            state: "pass".to_string(),
            code: 0,
            message: format!("processed task {index}"),
            log: Vec::new(),
            report_path: None,
            print_report_path: None,
            failure: None,
            source_csv_path: Some(format!("source-{index}.csv")),
            csv_fingerprint: Some(format!("fingerprint-{index}")),
        }
    }

    fn task_state(task_id: &str, state: &str) -> UnitTaskState {
        UnitTaskState {
            task_id: task_id.to_string(),
            state: state.to_string(),
            code: Some(0),
            source_csv_path: None,
            csv_fingerprint: None,
            processed_at: Some(now_string()),
            result: Some("test result".to_string()),
            accepted: TaskAcceptance::default(),
            audit_log: Vec::new(),
        }
    }
}
