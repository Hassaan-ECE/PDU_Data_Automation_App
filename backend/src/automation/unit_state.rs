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
    #[serde(default)]
    pub notification_receipts: NotificationReceipts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationReceipts {
    #[serde(default)]
    pub complete: Option<NotificationReceipt>,
    #[serde(default)]
    pub problems: BTreeMap<String, NotificationReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationReceipt {
    pub event_key: String,
    pub accepted_at: String,
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
            notification_receipts: NotificationReceipts::default(),
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

/// Returns the durable receipt for the unit-level Complete notification, if one exists.
pub fn get_complete_notification_receipt(
    unit_folder: &Path,
) -> io::Result<Option<NotificationReceipt>> {
    let _lock = UnitStateLock::acquire(unit_folder)?;
    let state = load_or_default_unlocked(unit_folder)?;

    Ok(state.notification_receipts.complete)
}

/// Records that the Complete event was accepted by the notification endpoint.
///
/// Returns `true` when the state file changed. Re-recording the same event key preserves the
/// original acceptance timestamp and does not rewrite the state file.
pub fn record_complete_notification_receipt(
    unit_folder: &Path,
    event_key: &str,
) -> io::Result<bool> {
    let _lock = UnitStateLock::acquire(unit_folder)?;
    let mut state = load_or_default_unlocked(unit_folder)?;

    if state
        .notification_receipts
        .complete
        .as_ref()
        .is_some_and(|receipt| receipt.event_key == event_key)
    {
        return Ok(false);
    }

    state.notification_receipts.complete = Some(NotificationReceipt {
        event_key: event_key.to_string(),
        accepted_at: now_string(),
    });
    save_unit_state_unlocked(unit_folder, &state)?;

    Ok(true)
}

/// Returns the durable Problem receipt for one task, if one exists.
pub fn get_problem_notification_receipt(
    unit_folder: &Path,
    task_id: &str,
) -> io::Result<Option<NotificationReceipt>> {
    let _lock = UnitStateLock::acquire(unit_folder)?;
    let state = load_or_default_unlocked(unit_folder)?;

    Ok(state.notification_receipts.problems.get(task_id).cloned())
}

/// Records that a task-level Problem event was accepted by the notification endpoint.
///
/// Returns `true` when the state file changed. Re-recording the same event key for the task
/// preserves the original acceptance timestamp and does not rewrite the state file.
pub fn record_problem_notification_receipt(
    unit_folder: &Path,
    task_id: &str,
    event_key: &str,
) -> io::Result<bool> {
    let _lock = UnitStateLock::acquire(unit_folder)?;
    let mut state = load_or_default_unlocked(unit_folder)?;

    if state
        .notification_receipts
        .problems
        .get(task_id)
        .is_some_and(|receipt| receipt.event_key == event_key)
    {
        return Ok(false);
    }

    state.notification_receipts.problems.insert(
        task_id.to_string(),
        NotificationReceipt {
            event_key: event_key.to_string(),
            accepted_at: now_string(),
        },
    );
    save_unit_state_unlocked(unit_folder, &state)?;

    Ok(true)
}

/// Clears Problem receipts for tasks the worker has observed as recovered or passed.
///
/// Returns `true` when at least one receipt was removed. Supplying tasks without receipts is a
/// no-op and does not rewrite the state file.
pub fn clear_problem_notification_receipts(
    unit_folder: &Path,
    recovered_or_passed_task_ids: &[String],
) -> io::Result<bool> {
    let _lock = UnitStateLock::acquire(unit_folder)?;
    let mut state = load_or_default_unlocked(unit_folder)?;
    let mut changed = false;

    for task_id in recovered_or_passed_task_ids {
        changed |= state
            .notification_receipts
            .problems
            .remove(task_id)
            .is_some();
    }

    if changed {
        save_unit_state_unlocked(unit_folder, &state)?;
    }

    Ok(changed)
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
        Ok(()) => {
            let _ = fs::remove_file(&backup_path);
            Ok(())
        }
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
    record_processor_results(unit_folder, &[(task_id.to_string(), result.clone())])
}

pub fn record_processor_results(
    unit_folder: &Path,
    results: &[(String, ProcessorResult)],
) -> io::Result<UnitState> {
    let _lock = UnitStateLock::acquire(unit_folder)?;
    let mut state = load_or_default_unlocked(unit_folder)?;
    let at = now_string();

    for (task_id, result) in results {
        record_processor_result_unlocked(&mut state, task_id, result, &at);
    }

    save_unit_state_unlocked(unit_folder, &state)?;

    Ok(state)
}

fn record_processor_result_unlocked(
    state: &mut UnitState,
    task_id: &str,
    result: &ProcessorResult,
    at: &str,
) {
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
    entry.processed_at = Some(at.to_string());
    entry.result = Some(result.message.clone());
    entry.audit_log.push(UnitAuditEntry {
        at: at.to_string(),
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
}

pub fn now_string() -> String {
    Local::now().to_rfc3339()
}

fn should_refresh_from_scan(state: &str) -> bool {
    matches!(state, "off" | "detected" | "waiting")
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
    fn old_unit_state_without_notification_receipts_uses_default() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("unit");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        fs::write(
            state_path(&unit_folder),
            r#"{
  "schema_version": 1,
  "tasks": {}
}"#,
        )
        .expect("write old state");

        let state = load_or_default(&unit_folder).expect("load old state");

        assert_eq!(state.notification_receipts, NotificationReceipts::default());
    }

    #[test]
    fn problem_notification_receipt_records_queries_and_persists() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("unit");
        fs::create_dir_all(&unit_folder).expect("unit folder");

        assert_eq!(
            get_problem_notification_receipt(&unit_folder, "system-208")
                .expect("query missing receipt"),
            None
        );
        assert!(record_problem_notification_receipt(
            &unit_folder,
            "system-208",
            "problem:system-208:fingerprint-1",
        )
        .expect("record receipt"));

        let receipt = get_problem_notification_receipt(&unit_folder, "system-208")
            .expect("query recorded receipt")
            .expect("recorded receipt");
        let reloaded = load_or_default(&unit_folder).expect("reload state");

        assert_eq!(receipt.event_key, "problem:system-208:fingerprint-1");
        assert!(!receipt.accepted_at.is_empty());
        assert_eq!(
            reloaded.notification_receipts.problems.get("system-208"),
            Some(&receipt)
        );
    }

    #[test]
    fn repeated_problem_receipt_record_is_a_no_op() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("unit");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        let event_key = "problem:breaker-1:fingerprint-1";

        assert!(
            record_problem_notification_receipt(&unit_folder, "breaker-1", event_key)
                .expect("first record")
        );
        let first_receipt = get_problem_notification_receipt(&unit_folder, "breaker-1")
            .expect("query first receipt")
            .expect("first receipt");
        let first_content =
            fs::read_to_string(state_path(&unit_folder)).expect("first state content");

        assert!(
            !record_problem_notification_receipt(&unit_folder, "breaker-1", event_key)
                .expect("repeated record")
        );

        assert_eq!(
            get_problem_notification_receipt(&unit_folder, "breaker-1")
                .expect("query repeated receipt")
                .expect("repeated receipt"),
            first_receipt
        );
        assert_eq!(
            fs::read_to_string(state_path(&unit_folder)).expect("repeated state content"),
            first_content
        );
    }

    #[test]
    fn problem_receipt_clears_after_task_recovery_without_rewriting_no_op() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("unit");
        fs::create_dir_all(&unit_folder).expect("unit folder");
        record_problem_notification_receipt(&unit_folder, "recovered", "problem:recovered:1")
            .expect("record recovered receipt");
        record_problem_notification_receipt(&unit_folder, "still-failing", "problem:failing:1")
            .expect("record failing receipt");

        assert!(
            clear_problem_notification_receipts(&unit_folder, &["recovered".to_string()])
                .expect("clear recovered receipt")
        );
        assert_eq!(
            get_problem_notification_receipt(&unit_folder, "recovered")
                .expect("query cleared receipt"),
            None
        );
        assert!(
            get_problem_notification_receipt(&unit_folder, "still-failing")
                .expect("query retained receipt")
                .is_some()
        );

        let cleared_content =
            fs::read_to_string(state_path(&unit_folder)).expect("cleared state content");
        assert!(
            !clear_problem_notification_receipts(&unit_folder, &["recovered".to_string()])
                .expect("repeat clear")
        );
        assert_eq!(
            fs::read_to_string(state_path(&unit_folder)).expect("no-op state content"),
            cleared_content
        );
    }

    #[test]
    fn complete_notification_receipt_survives_reload() {
        let temp = TempDir::new().expect("temp dir");
        let unit_folder = temp.path().join("unit");
        fs::create_dir_all(&unit_folder).expect("unit folder");

        assert!(
            record_complete_notification_receipt(&unit_folder, "complete:unit-123:ready-1")
                .expect("record complete receipt")
        );

        let queried = get_complete_notification_receipt(&unit_folder)
            .expect("query complete receipt")
            .expect("complete receipt");
        let reloaded = load_or_default(&unit_folder).expect("reload state");

        assert_eq!(queried.event_key, "complete:unit-123:ready-1");
        assert!(!queried.accepted_at.is_empty());
        assert_eq!(reloaded.notification_receipts.complete, Some(queried));
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
    fn save_unit_state_removes_temp_and_backup_after_success() {
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

        assert_ne!(
            fs::read_to_string(&path).expect("second state content"),
            first_content
        );
        assert!(
            !temp_path.exists(),
            "temporary state file should be renamed away"
        );
        assert!(
            !backup_path.exists(),
            "backup state file should be removed after a successful save"
        );

        let reloaded = load_or_default(&unit_folder).expect("reloaded state");

        assert!(reloaded.tasks.contains_key("first"));
        assert!(reloaded.tasks.contains_key("second"));
    }

    #[test]
    fn persisted_waiting_is_refreshed_from_readable_scan() {
        let mut state = UnitState::default();
        state.tasks.insert(
            "208v-transformer".to_string(),
            task_state("208v-transformer", "waiting"),
        );

        let changed = ensure_task_entries(
            &mut state,
            &[TaskStateSeed {
                task_id: "208v-transformer".to_string(),
                state: "detected".to_string(),
                source_csv_path: Some("STEP14.csv".to_string()),
                csv_fingerprint: None,
            }],
        );

        assert!(changed);
        assert_eq!(state.tasks["208v-transformer"].state, "detected");
        assert_eq!(state.tasks["208v-transformer"].code, None);
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
