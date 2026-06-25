use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};

use super::processors::ProcessorResult;

pub const UNIT_STATE_FILE: &str = "unit_state.json";

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
    let path = state_path(unit_folder);

    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;
    let state = serde_json::from_str::<UnitState>(&content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    Ok(Some(state))
}

pub fn load_or_default(unit_folder: &Path) -> io::Result<UnitState> {
    Ok(load_unit_state(unit_folder)?.unwrap_or_default())
}

pub fn save_unit_state(unit_folder: &Path, state: &UnitState) -> io::Result<()> {
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
    let mut state = load_or_default(unit_folder)?;
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

    save_unit_state(unit_folder, &state)?;

    Ok(state)
}

pub fn now_string() -> String {
    Local::now().to_rfc3339()
}

fn should_refresh_from_scan(state: &str) -> bool {
    matches!(state, "off" | "detected")
}
