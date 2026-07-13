use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use regex::Regex;
use serde::Serialize;

use crate::automation::tasks::{find_task, TaskKind};
use crate::automation::{self, TaskProcessResult};

use super::{
    append_shift_log_event, can_send, format_event_message_now, load_runtime_resolved_config,
    resolve_floor_settings_file, EventKind, LoggedEvent, NotificationEvent, ResolvedConfig,
    ShiftLogError, TeamsClient,
};

const NOTIFICATION_QUEUE_CAPACITY: usize = 16;
/// How often to re-read `floor_settings.json` while the app is open.
const FLOOR_SETTINGS_POLL_INTERVAL: Duration = Duration::from_secs(45);

#[derive(Debug)]
enum NotificationJob {
    TaskResults {
        unit_folder: PathBuf,
        results: Vec<TaskProcessResult>,
    },
    CheckComplete {
        unit_folder: PathBuf,
    },
    TestPing,
}

impl NotificationJob {
    fn event_kind(&self) -> Option<EventKind> {
        match self {
            Self::TaskResults { results, .. }
                if results
                    .iter()
                    .any(|result| matches!(result.state.as_str(), "fail" | "warning")) =>
            {
                Some(EventKind::Problem)
            }
            Self::TaskResults { results, .. }
                if results.iter().any(|result| result.state == "pass") =>
            {
                Some(EventKind::Complete)
            }
            Self::TaskResults { .. } => None,
            Self::CheckComplete { .. } => Some(EventKind::Complete),
            Self::TestPing => Some(EventKind::TestPing),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NotificationRuntimeStatus {
    pub state: String,
    pub message: String,
    pub station_name: Option<String>,
    pub destination_name: Option<String>,
    pub updated_at: Option<String>,
    pub event_kind: Option<String>,
}

impl Default for NotificationRuntimeStatus {
    fn default() -> Self {
        Self {
            state: "idle".to_string(),
            message: "No notification delivery has been attempted yet.".to_string(),
            station_name: None,
            destination_name: None,
            updated_at: None,
            event_kind: None,
        }
    }
}

/// Non-blocking entry point used by Tauri commands.
///
/// The bounded queue and dedicated worker keep webhook file/network work out of
/// the CSV processing and workbook commit path. Queue or worker failures are
/// deliberately reflected only in notification status.
pub struct NotificationService {
    sender: SyncSender<NotificationJob>,
    status: Arc<Mutex<NotificationRuntimeStatus>>,
}

impl NotificationService {
    pub fn start() -> Self {
        let (sender, receiver) = mpsc::sync_channel(NOTIFICATION_QUEUE_CAPACITY);
        let status = Arc::new(Mutex::new(NotificationRuntimeStatus::default()));
        let worker_status = Arc::clone(&status);

        if thread::Builder::new()
            .name("pdu-teams-notifications".to_string())
            .spawn(move || worker_loop(receiver, worker_status))
            .is_err()
        {
            set_status(
                &status,
                "failed",
                "The notification background worker could not start.",
                None,
            );
        }

        let poll_status = Arc::clone(&status);
        let _ = thread::Builder::new()
            .name("pdu-floor-settings-poll".to_string())
            .spawn(move || floor_settings_poll_loop(poll_status));

        Self { sender, status }
    }

    pub fn enqueue_task_results(
        &self,
        unit_folder: impl Into<PathBuf>,
        results: Vec<TaskProcessResult>,
    ) {
        if results.is_empty() {
            return;
        }
        self.try_enqueue(NotificationJob::TaskResults {
            unit_folder: unit_folder.into(),
            results,
        });
    }

    pub fn enqueue_complete_check(&self, unit_folder: impl Into<PathBuf>) {
        self.try_enqueue(NotificationJob::CheckComplete {
            unit_folder: unit_folder.into(),
        });
    }

    pub fn enqueue_test_ping(&self) {
        self.try_enqueue(NotificationJob::TestPing);
    }

    pub fn status(&self) -> NotificationRuntimeStatus {
        let current = self
            .status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_else(|_| NotificationRuntimeStatus {
                state: "failed".to_string(),
                message: "Notification status is unavailable.".to_string(),
                ..NotificationRuntimeStatus::default()
            });
        if current.state != "idle" {
            return current;
        }

        match load_runtime_resolved_config() {
            Ok(config) => match can_send(&config) {
                Ok(()) => NotificationRuntimeStatus {
                    state: "ready".to_string(),
                    message: "Teams operator alerts are configured.".to_string(),
                    station_name: Some(config.station_name),
                    destination_name: Some(config.teams_destination_name),
                    updated_at: None,
                    event_kind: None,
                },
                Err(error) => NotificationRuntimeStatus {
                    state: "skipped".to_string(),
                    message: format!("Teams operator alerts are inactive: {error}"),
                    station_name: Some(config.station_name),
                    destination_name: Some(config.teams_destination_name),
                    updated_at: None,
                    event_kind: None,
                },
            },
            Err(error) => NotificationRuntimeStatus {
                state: "skipped".to_string(),
                message: format!("Teams operator alerts are not configured: {error}"),
                updated_at: None,
                ..NotificationRuntimeStatus::default()
            },
        }
    }

    /// Make the next status read resolve the newly saved app settings instead
    /// of retaining a delivery/configuration message from the previous values.
    pub fn mark_configuration_changed(&self) {
        if let Ok(mut status) = self.status.lock() {
            *status = NotificationRuntimeStatus::default();
        }
    }

    fn try_enqueue(&self, job: NotificationJob) {
        let event_kind = job.event_kind();
        match self.sender.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => set_status_for_event(
                &self.status,
                "skipped",
                "The notification queue is full; automation continued without waiting.",
                None,
                event_kind,
            ),
            Err(TrySendError::Disconnected(_)) => set_status_for_event(
                &self.status,
                "failed",
                "The notification background worker is unavailable.",
                None,
                event_kind,
            ),
        }
    }
}

fn worker_loop(
    receiver: mpsc::Receiver<NotificationJob>,
    status: Arc<Mutex<NotificationRuntimeStatus>>,
) {
    let client = match TeamsClient::new() {
        Ok(client) => client,
        Err(error) => {
            set_status(
                &status,
                "failed",
                &format!("Notification HTTP setup failed: {error}"),
                None,
            );
            return;
        }
    };

    while let Ok(job) = receiver.recv() {
        match job {
            NotificationJob::TaskResults {
                unit_folder,
                results,
            } => handle_task_results(&client, &status, &unit_folder, results),
            NotificationJob::CheckComplete { unit_folder } => {
                handle_complete(&client, &status, &unit_folder)
            }
            NotificationJob::TestPing => handle_test_ping(&client, &status),
        }
    }
}

fn handle_task_results(
    client: &TeamsClient,
    status: &Arc<Mutex<NotificationRuntimeStatus>>,
    unit_folder: &Path,
    results: Vec<TaskProcessResult>,
) {
    let recovered_task_ids = results
        .iter()
        .filter(|result| result.state == "pass")
        .map(|result| result.task_id.clone())
        .collect::<Vec<_>>();
    if !recovered_task_ids.is_empty() {
        if let Err(error) = automation::unit_state::clear_problem_notification_receipts(
            unit_folder,
            &recovered_task_ids,
        ) {
            set_status(
                status,
                "failed",
                &format!("Notification receipt cleanup failed: {error}"),
                None,
            );
        }
    }

    let problem_results = results
        .iter()
        .filter(|result| matches!(result.state.as_str(), "fail" | "warning"))
        .collect::<Vec<_>>();

    if !problem_results.is_empty() {
        let config = match usable_config(EventKind::Problem, status) {
            Some(config) => config,
            None => return,
        };
        for result in problem_results {
            handle_problem(client, status, unit_folder, result, &config);
        }
        return;
    }

    if results.iter().any(|result| result.state == "pass") {
        handle_complete(client, status, unit_folder);
    }
}

fn handle_problem(
    client: &TeamsClient,
    status: &Arc<Mutex<NotificationRuntimeStatus>>,
    unit_folder: &Path,
    result: &TaskProcessResult,
    config: &ResolvedConfig,
) {
    match automation::unit_state::get_problem_notification_receipt(unit_folder, &result.task_id) {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(error) => {
            set_event_status(
                status,
                "failed",
                &format!("Problem notification receipt could not be read: {error}"),
                Some(config),
                EventKind::Problem,
            );
            return;
        }
    }

    let (subject, current_step) = task_context(&result.task_id);
    let detail = result
        .failure
        .as_ref()
        .map(|failure| failure.message.trim())
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| result.message.trim())
        .to_string();
    let unit_serial_number = serial_number_from_result(unit_folder, result);
    let event = NotificationEvent {
        kind: EventKind::Problem,
        unit_serial_number,
        subject,
        detail: (!detail.is_empty()).then_some(detail),
        current_step,
    };
    let message = format_event_message_now(&config.station_name, &event);

    match client.post_message(&config.teams_webhook_url, &message) {
        Ok(()) => {
            let event_key = problem_event_key(result);
            if let Err(error) = automation::unit_state::record_problem_notification_receipt(
                unit_folder,
                &result.task_id,
                &event_key,
            ) {
                set_event_status(
                    status,
                    "failed",
                    &format!(
                        "Problem card was accepted, but its receipt could not be saved: {error}"
                    ),
                    Some(config),
                    EventKind::Problem,
                );
                return;
            }
            let delivery_status = shared_log_delivery_status(
                "Problem card accepted by the Workflow.",
                config,
                EventKind::Problem,
            );
            set_event_status(
                status,
                "sent",
                &delivery_status,
                Some(config),
                EventKind::Problem,
            );
        }
        Err(error) => set_event_status(
            status,
            "failed",
            &format!("Problem card delivery failed: {error}"),
            Some(config),
            EventKind::Problem,
        ),
    }
}

fn handle_complete(
    client: &TeamsClient,
    status: &Arc<Mutex<NotificationRuntimeStatus>>,
    unit_folder: &Path,
) {
    match automation::unit_state::get_complete_notification_receipt(unit_folder) {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(error) => {
            set_event_status(
                status,
                "failed",
                &format!("Complete notification receipt could not be read: {error}"),
                None,
                EventKind::Complete,
            );
            return;
        }
    }

    let config = match usable_config(EventKind::Complete, status) {
        Some(config) => config,
        None => return,
    };
    let readiness = match automation::validate_ready_for_print(unit_folder.display().to_string()) {
        Ok(readiness) if readiness.ready => readiness,
        Ok(_) => return,
        Err(error) => {
            set_event_status(
                status,
                "failed",
                &format!("Complete readiness check failed: {}", error.message),
                Some(&config),
                EventKind::Complete,
            );
            return;
        }
    };

    let unit_serial_number = serial_number_from_unit_context(unit_folder);
    let event = NotificationEvent {
        kind: EventKind::Complete,
        unit_serial_number: unit_serial_number.clone(),
        subject: "Ready for print and operator sign-off".to_string(),
        detail: None,
        current_step: None,
    };
    let message = format_event_message_now(&config.station_name, &event);

    match client.post_message(&config.teams_webhook_url, &message) {
        Ok(()) => {
            let event_key = format!(
                "complete:{}:{}",
                unit_serial_number.as_deref().unwrap_or("unknown-unit"),
                readiness.blocking_issues.len()
            );
            if let Err(error) = automation::unit_state::record_complete_notification_receipt(
                unit_folder,
                &event_key,
            ) {
                set_event_status(
                    status,
                    "failed",
                    &format!(
                        "Complete card was accepted, but its receipt could not be saved: {error}"
                    ),
                    Some(&config),
                    EventKind::Complete,
                );
                return;
            }
            let delivery_status = shared_log_delivery_status(
                "Complete card accepted by the Workflow.",
                &config,
                EventKind::Complete,
            );
            set_event_status(
                status,
                "sent",
                &delivery_status,
                Some(&config),
                EventKind::Complete,
            );
        }
        Err(error) => set_event_status(
            status,
            "failed",
            &format!("Complete card delivery failed: {error}"),
            Some(&config),
            EventKind::Complete,
        ),
    }
}

fn handle_test_ping(client: &TeamsClient, status: &Arc<Mutex<NotificationRuntimeStatus>>) {
    let config = match usable_config(EventKind::TestPing, status) {
        Some(config) => config,
        None => return,
    };
    let message = format_event_message_now(&config.station_name, &NotificationEvent::test_ping());
    match client.post_message(&config.teams_webhook_url, &message) {
        Ok(()) => set_event_status(
            status,
            "sent",
            "Test card accepted by the Workflow.",
            Some(&config),
            EventKind::TestPing,
        ),
        Err(error) => set_event_status(
            status,
            "failed",
            &format!("Test card delivery failed: {error}"),
            Some(&config),
            EventKind::TestPing,
        ),
    }
}

/// Poll shared `floor_settings.json` so webhook/password/names stay fresh without
/// requiring Settings to be open. Soft-fails; never affects automation.
/// Fingerprint first (path pointer only), then a single merge load when changed.
fn floor_settings_poll_loop(status: Arc<Mutex<NotificationRuntimeStatus>>) {
    let mut last_fingerprint: Option<String> = None;
    loop {
        thread::sleep(FLOOR_SETTINGS_POLL_INTERVAL);
        let Some(shared) = super::configured_shared_path_pointer() else {
            last_fingerprint = None;
            continue;
        };
        let Some(path) = resolve_floor_settings_file(&shared) else {
            continue;
        };
        let fingerprint = match fs::metadata(&path).and_then(|meta| meta.modified()) {
            Ok(modified) => format!("{modified:?}:{}", path.display()),
            Err(_) => match fs::read_to_string(&path) {
                Ok(raw) => format!("body:{}", raw.len()),
                Err(_) => {
                    // File missing/unavailable: still attempt a merge load so local
                    // cache status can reflect stale/unavailable without reseeding.
                    if last_fingerprint.as_deref() != Some("missing") {
                        last_fingerprint = Some("missing".to_string());
                        let _ = super::load_app_settings_with_floor();
                    }
                    continue;
                }
            },
        };
        if last_fingerprint.as_ref() == Some(&fingerprint) {
            continue;
        }
        last_fingerprint = Some(fingerprint);
        // Single merge load so local cache picks up peer edits (no prior full load).
        if super::load_app_settings_with_floor().is_ok() {
            if let Ok(mut current) = status.lock() {
                // Reset sticky status so the next status() call re-resolves config.
                if current.state == "idle" || current.state == "ready" || current.state == "skipped"
                {
                    *current = NotificationRuntimeStatus::default();
                }
            }
        }
    }
}

fn usable_config(
    kind: EventKind,
    status: &Arc<Mutex<NotificationRuntimeStatus>>,
) -> Option<ResolvedConfig> {
    let config = match load_runtime_resolved_config() {
        Ok(config) => config,
        Err(error) => {
            set_event_status(
                status,
                "skipped",
                &format!("Notification configuration is unavailable: {error}"),
                None,
                kind,
            );
            return None;
        }
    };
    if let Err(error) = can_send(&config) {
        set_event_status(
            status,
            "skipped",
            &format!("Notification was skipped: {error}"),
            Some(&config),
            kind,
        );
        return None;
    }
    let enabled = match kind {
        EventKind::Problem => config.events.problem,
        EventKind::Complete => config.events.complete,
        EventKind::Stuck => config.events.stuck,
        EventKind::Summary => config.events.summary,
        EventKind::TestPing => true,
    };
    if !enabled {
        set_event_status(
            status,
            "skipped",
            "This notification event is disabled in Notification settings.",
            Some(&config),
            kind,
        );
        return None;
    }
    Some(config)
}

fn shared_log_delivery_status(
    accepted_message: &str,
    config: &ResolvedConfig,
    kind: EventKind,
) -> String {
    match append_to_shared_shift_log(config, kind) {
        Ok(()) => accepted_message.to_string(),
        Err(error) => format!("{accepted_message} The shared shift log was not updated: {error}"),
    }
}

fn append_to_shared_shift_log(
    config: &ResolvedConfig,
    kind: EventKind,
) -> Result<(), ShiftLogError> {
    let configured = config.shared_shift_log_path.trim();
    if configured.is_empty() {
        return Ok(());
    }

    let event = LoggedEvent::from_notification_kind(
        &config.station_id,
        &config.station_name,
        kind,
        chrono::Local::now().to_rfc3339(),
    )?;
    // Configured path is the shared root folder (OneDrive). Append resolves to
    // <root>/shift_log.json and ensures stations/* exist.
    append_shift_log_event(Path::new(configured), event)
}

fn task_context(task_id: &str) -> (String, Option<String>) {
    let Some(task) = find_task(task_id) else {
        return (task_id.to_string(), None);
    };
    let subject = match task.kind {
        TaskKind::Transformer { voltage } => format!("{} Transformer Check", voltage.display()),
        TaskKind::System { voltage, load } => {
            format!("{} System · {}", voltage.display(), load.display())
        }
        TaskKind::Breaker {
            voltage,
            breaker,
            load,
        } => format!(
            "{} Breaker {} · {}",
            voltage.display(),
            breaker,
            load.display()
        ),
        TaskKind::SystemBurnIn => "System Burn-In".to_string(),
        TaskKind::BreakerBurnIn { breaker } => format!("Breaker {breaker} Burn-In"),
    };
    (subject, Some(format!("STEP{}", task.step_display)))
}

fn problem_event_key(result: &TaskProcessResult) -> String {
    format!(
        "problem:{}:{}:{}:{}",
        result.task_id,
        result.state,
        result.code,
        result
            .csv_fingerprint
            .as_deref()
            .unwrap_or("no-fingerprint")
    )
}

fn serial_number_from_result(unit_folder: &Path, result: &TaskProcessResult) -> Option<String> {
    automation::resolve_unit_serial_number(unit_folder)
        .or_else(|| {
            result
                .report_path
                .as_deref()
                .and_then(serial_number_from_sn_marker)
        })
        .or_else(|| serial_number_from_unit_context(unit_folder))
}

fn serial_number_from_unit_context(unit_folder: &Path) -> Option<String> {
    automation::resolve_unit_serial_number(unit_folder).or_else(|| {
        fs::read_dir(unit_folder)
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .find_map(|name| serial_number_from_sn_marker(&name))
    })
}

fn serial_number_from_sn_marker(text: &str) -> Option<String> {
    let sn_pattern = Regex::new(r"(?i)SN[_ -]?(\d{6,})").expect("SN regex is valid");
    sn_pattern
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|serial| serial.as_str().to_string())
}

fn set_status(
    status: &Arc<Mutex<NotificationRuntimeStatus>>,
    state: &str,
    message: &str,
    config: Option<&ResolvedConfig>,
) {
    set_status_for_event(status, state, message, config, None);
}

fn set_event_status(
    status: &Arc<Mutex<NotificationRuntimeStatus>>,
    state: &str,
    message: &str,
    config: Option<&ResolvedConfig>,
    event_kind: EventKind,
) {
    set_status_for_event(status, state, message, config, Some(event_kind));
}

fn set_status_for_event(
    status: &Arc<Mutex<NotificationRuntimeStatus>>,
    state: &str,
    message: &str,
    config: Option<&ResolvedConfig>,
    event_kind: Option<EventKind>,
) {
    if let Ok(mut current) = status.lock() {
        current.state = state.to_string();
        current.message = message.to_string();
        current.station_name = config.map(|value| value.station_name.clone());
        current.destination_name = config.map(|value| value.teams_destination_name.clone());
        current.updated_at = Some(chrono::Local::now().to_rfc3339());
        current.event_kind = event_kind.map(event_kind_name).map(str::to_string);
    }
}

fn event_kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::TestPing => "test_ping",
        EventKind::Problem => "problem",
        EventKind::Complete => "complete",
        EventKind::Stuck => "stuck",
        EventKind::Summary => "summary",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_status_identifies_test_ping_for_frontend_correlation() {
        let status = Arc::new(Mutex::new(NotificationRuntimeStatus::default()));
        set_event_status(
            &status,
            "sent",
            "Test card accepted.",
            None,
            EventKind::TestPing,
        );

        let current = status.lock().unwrap().clone();
        assert_eq!(current.event_kind.as_deref(), Some("test_ping"));
        assert_eq!(current.state, "sent");
    }

    #[test]
    fn full_task_context_includes_voltage_breaker_load_and_step() {
        let (subject, step) = task_context("208v-breaker-3-100% Load");
        assert_eq!(subject, "208V Breaker 3 · 100% Load");
        assert_eq!(step.as_deref(), Some("STEP24"));
    }

    #[test]
    fn serial_number_prefers_profile_valid_selected_unit_folder() {
        let result = TaskProcessResult {
            task_id: "208v-transformer".to_string(),
            state: "fail".to_string(),
            code: 1,
            message: "failed".to_string(),
            log: Vec::new(),
            report_path: Some("C:/unit/Report_SN262343000072.xlsx".to_string()),
            print_report_path: None,
            failure: None,
            source_csv_path: None,
            csv_fingerprint: None,
        };
        assert_eq!(
            serial_number_from_result(Path::new("C:/unit/999999999999"), &result).as_deref(),
            Some("999999999999")
        );
    }

    #[test]
    fn serial_number_uses_sn_marker_in_real_report_name_not_product_number() {
        let real_name = "PDUD500442AM088_Test Report_0.2CT_Rev02_SN262343000072.xlsx";
        assert_eq!(
            serial_number_from_sn_marker(real_name).as_deref(),
            Some("262343000072")
        );
        assert_eq!(serial_number_from_sn_marker("DEMO_20260617"), None);
    }

    #[test]
    fn complete_context_uses_report_serial_in_noncanonical_demo_folder() {
        let temp = tempfile::tempdir().unwrap();
        let unit_folder = temp.path().join("DEMO_20260617");
        fs::create_dir_all(&unit_folder).unwrap();
        fs::write(
            unit_folder.join("PDUD500442AM088_Test Report_0.2CT_Rev02_SN262343000074.xlsx"),
            [],
        )
        .unwrap();

        assert_eq!(
            serial_number_from_unit_context(&unit_folder).as_deref(),
            Some("262343000074")
        );
    }

    #[test]
    fn problem_key_never_contains_webhook_or_failure_detail() {
        let result = TaskProcessResult {
            task_id: "task".to_string(),
            state: "fail".to_string(),
            code: 1,
            message: "sensitive detail".to_string(),
            log: Vec::new(),
            report_path: None,
            print_report_path: None,
            failure: None,
            source_csv_path: None,
            csv_fingerprint: Some("fingerprint".to_string()),
        };
        let key = problem_event_key(&result);
        assert!(!key.contains("sensitive detail"));
        assert_eq!(key, "problem:task:fail:1:fingerprint");
    }
}
