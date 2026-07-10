# In-app notification settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a password-gated Settings screen (cog → unlock → edit station/webhook/password/test ping) with config stored inside the app’s AppData store, and wire the existing Teams notification worker to that store instead of operator-managed JSON files.

**Architecture:** Rust owns a single `notification_settings.json` under Tauri `app_config_dir`. New Tauri commands load/save/verify password and feed `ResolvedConfig` into the existing `NotificationService`. React adds a small view state machine on top of `OperatorPanel` / `App`: main panel ↔ password modal ↔ settings page. Optional `shared_shift_log_path` is stored now; append/read runs only when non-empty (path confirmed later by deploy).

**Tech Stack:** Tauri 2, Rust (`serde`/`serde_json`), React + TypeScript, existing `notifications` module, lucide-react icons, Vitest for UI, `cargo test` for Rust.

**Design spec:** `docs/superpowers/specs/2026-07-10-in-app-notification-settings-design.md`

**Implementation status (2026-07-10):** Tasks 1–5 and the Task 6 documentation are implemented and pass the repository validation pipeline. The live Teams/installer checklist remains for station deployment, and Task 7 remains intentionally deferred until the shared path is confirmed.

**Out of scope for this plan:** Stuck automation; full end-of-shift UI button (optional stretch in Task 8 only if path already known); cryptographic password hashing.

---

## File structure

```text
backend/src/notifications/
  app_settings.rs          # NEW: AppData load/save, password, DTO → ResolvedConfig
  config.rs                # MODIFY: load_config prefers app settings; keep helpers for tests
  worker.rs                # MODIFY: use app settings path provider if needed
  mod.rs                   # MODIFY: exports
  shift_log.rs             # NEW: optional shared log append (no-op if path empty)
backend/src/commands.rs    # MODIFY: settings + password + test ping commands
backend/src/lib.rs         # MODIFY: register commands; pass app handle / config dir
frontend/src/features/settings/
  SettingsPage.tsx         # NEW: full settings UI
  SettingsPasswordModal.tsx# NEW: unlock modal
  settingsTypes.ts         # NEW: shared TS types
frontend/src/features/test-panel/OperatorPanel.tsx  # MODIFY: cog, view switch
frontend/src/integrations/tauri/backend.ts          # MODIFY: invoke wrappers
frontend/tests/...         # MODIFY/NEW: settings UI tests
docs/NOTIFICATIONS.md      # MODIFY: document in-app settings + optional shift log path
```

| File | Responsibility |
|------|----------------|
| `app_settings.rs` | Persist/load settings; default password `0601`; map to `ResolvedConfig` |
| `shift_log.rs` | Append event to shared JSON when path set; soft-fail |
| `SettingsPage.tsx` | Form fields, save, change password, test ping |
| `OperatorPanel.tsx` | Cog button; host modal + swap main content |

---

### Task 1: App settings model + disk I/O (Rust)

**Files:**
- Create: `backend/src/notifications/app_settings.rs`
- Modify: `backend/src/notifications/mod.rs`
- Test: unit tests inside `app_settings.rs`

- [ ] **Step 1: Write failing tests for defaults and round-trip**

Add to `app_settings.rs` (create file with tests first):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_file_loads_defaults_with_factory_password() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("notification_settings.json");
        let settings = load_app_settings_from(&path).unwrap();
        assert_eq!(settings.settings_password, DEFAULT_SETTINGS_PASSWORD);
        assert_eq!(settings.station_id, "test-station-1");
        assert_eq!(settings.station_name, "Test Station 1");
        assert!(settings.teams_webhook_url.is_empty());
        assert!(settings.enabled);
        assert!(settings.shared_shift_log_path.is_empty());
    }

    #[test]
    fn save_and_reload_preserves_fields_and_redacts_debug() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("notification_settings.json");
        let mut settings = AppNotificationSettings::default();
        settings.teams_webhook_url = "https://example.invalid/hook?sig=TOP_SECRET".into();
        settings.station_id = "test-station-3".into();
        settings.station_name = "Test Station 3".into();
        settings.shared_shift_log_path = r"\\server\share\shift_log.json".into();
        save_app_settings_to(&path, &settings).unwrap();

        let loaded = load_app_settings_from(&path).unwrap();
        assert_eq!(loaded.station_id, "test-station-3");
        assert_eq!(loaded.teams_webhook_url, settings.teams_webhook_url);
        assert_eq!(loaded.shared_shift_log_path, settings.shared_shift_log_path);

        let debug = format!("{loaded:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("TOP_SECRET"));
    }

    #[test]
    fn verify_and_change_password() {
        let mut settings = AppNotificationSettings::default();
        assert!(verify_password(&settings, "0601"));
        assert!(!verify_password(&settings, "0000"));
        change_password(&mut settings, "0601", "9999").unwrap();
        assert!(verify_password(&settings, "9999"));
        assert!(!verify_password(&settings, "0601"));
        assert!(change_password(&mut settings, "wrong", "1111").is_err());
    }

    #[test]
    fn into_resolved_config_maps_fields() {
        let mut settings = AppNotificationSettings::default();
        settings.teams_webhook_url = "https://example.invalid/hook".into();
        settings.enabled = true;
        let resolved = settings.to_resolved_config();
        assert_eq!(resolved.station_name, "Test Station 1");
        assert_eq!(resolved.teams_webhook_url, "https://example.invalid/hook");
        assert!(can_send(&resolved).is_ok());
    }
}
```

- [ ] **Step 2: Run tests — expect FAIL (module missing)**

```powershell
cd C:\Projects\Active\PDU_Data_Automation_App\backend
cargo test --lib notifications::app_settings -- --nocapture
```

Expected: compile error / module not found.

- [ ] **Step 3: Implement `app_settings.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use thiserror::Error;

use super::config::{can_send, EventToggles, ResolvedConfig};

pub const DEFAULT_SETTINGS_PASSWORD: &str = "0601";
pub const SETTINGS_FILE_NAME: &str = "notification_settings.json";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AppSettingsError {
    #[error("Could not read notification settings: {0}")]
    Read(String),
    #[error("Could not write notification settings: {0}")]
    Write(String),
    #[error("Invalid notification settings JSON: {0}")]
    Parse(String),
    #[error("Current password is incorrect")]
    WrongPassword,
    #[error("New password must not be empty")]
    EmptyPassword,
    #[error("New password and confirmation do not match")]
    PasswordMismatch,
    #[error("station_id is empty")]
    EmptyStationId,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppNotificationSettings {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    #[serde(default = "default_password")]
    pub settings_password: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_destination")]
    pub teams_destination_name: String,
    #[serde(default)]
    pub teams_webhook_url: String,
    #[serde(default = "default_station_id")]
    pub station_id: String,
    #[serde(default = "default_station_name")]
    pub station_name: String,
    #[serde(default = "default_idle")]
    pub idle_timeout_minutes: u32,
    #[serde(default)]
    pub events: EventToggles,
    /// Empty = multi-station shift log disabled until deploy path is configured.
    #[serde(default)]
    pub shared_shift_log_path: String,
}

impl Default for AppNotificationSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            settings_password: DEFAULT_SETTINGS_PASSWORD.to_string(),
            enabled: true,
            teams_destination_name: default_destination(),
            teams_webhook_url: String::new(),
            station_id: default_station_id(),
            station_name: default_station_name(),
            idle_timeout_minutes: default_idle(),
            events: EventToggles {
                problem: true,
                complete: true,
                stuck: false,
                summary: false,
            },
            shared_shift_log_path: String::new(),
        }
    }
}

impl fmt::Debug for AppNotificationSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppNotificationSettings")
            .field("schema_version", &self.schema_version)
            .field("settings_password", &"<redacted>")
            .field("enabled", &self.enabled)
            .field("teams_destination_name", &self.teams_destination_name)
            .field("teams_webhook_url", &webhook_debug(&self.teams_webhook_url))
            .field("station_id", &self.station_id)
            .field("station_name", &self.station_name)
            .field("idle_timeout_minutes", &self.idle_timeout_minutes)
            .field("events", &self.events)
            .field("shared_shift_log_path", &self.shared_shift_log_path)
            .finish()
    }
}

fn webhook_debug(url: &str) -> &'static str {
    if url.trim().is_empty() {
        "<empty>"
    } else {
        "<redacted>"
    }
}

fn default_schema() -> u32 { 1 }
fn default_password() -> String { DEFAULT_SETTINGS_PASSWORD.to_string() }
fn default_true() -> bool { true }
fn default_destination() -> String { "PDU Testing".to_string() }
fn default_station_id() -> String { "test-station-1".to_string() }
fn default_station_name() -> String { "Test Station 1".to_string() }
fn default_idle() -> u32 { 30 }

/// Process-wide config directory set once from Tauri setup.
static CONFIG_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

pub fn set_app_config_dir(path: PathBuf) {
    if let Ok(mut guard) = CONFIG_DIR.write() {
        *guard = Some(path);
    }
}

pub fn app_settings_path() -> PathBuf {
    let dir = CONFIG_DIR
        .read()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| std::env::temp_dir().join("pdu-data-automation-app"));
    dir.join(SETTINGS_FILE_NAME)
}

pub fn load_app_settings() -> Result<AppNotificationSettings, AppSettingsError> {
    load_app_settings_from(app_settings_path())
}

pub fn load_app_settings_from(path: impl AsRef<Path>) -> Result<AppNotificationSettings, AppSettingsError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(AppNotificationSettings::default());
    }
    let raw = fs::read_to_string(path).map_err(|e| AppSettingsError::Read(e.to_string()))?;
    serde_json::from_str(&raw).map_err(|e| AppSettingsError::Parse(e.to_string()))
}

pub fn save_app_settings(settings: &AppNotificationSettings) -> Result<(), AppSettingsError> {
    save_app_settings_to(app_settings_path(), settings)
}

pub fn save_app_settings_to(
    path: impl AsRef<Path>,
    settings: &AppNotificationSettings,
) -> Result<(), AppSettingsError> {
    validate_for_save(settings)?;
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppSettingsError::Write(e.to_string()))?;
    }
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|e| AppSettingsError::Write(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw).map_err(|e| AppSettingsError::Write(e.to_string()))?;
    fs::rename(&tmp, path).map_err(|e| AppSettingsError::Write(e.to_string()))?;
    Ok(())
}

fn validate_for_save(settings: &AppNotificationSettings) -> Result<(), AppSettingsError> {
    if settings.station_id.trim().is_empty() {
        return Err(AppSettingsError::EmptyStationId);
    }
    if settings.settings_password.trim().is_empty() {
        return Err(AppSettingsError::EmptyPassword);
    }
    Ok(())
}

pub fn verify_password(settings: &AppNotificationSettings, attempt: &str) -> bool {
    settings.settings_password == attempt
}

pub fn change_password(
    settings: &mut AppNotificationSettings,
    current: &str,
    new_password: &str,
) -> Result<(), AppSettingsError> {
    if !verify_password(settings, current) {
        return Err(AppSettingsError::WrongPassword);
    }
    let new_password = new_password.trim();
    if new_password.is_empty() {
        return Err(AppSettingsError::EmptyPassword);
    }
    settings.settings_password = new_password.to_string();
    Ok(())
}

impl AppNotificationSettings {
    pub fn to_resolved_config(&self) -> ResolvedConfig {
        ResolvedConfig {
            enabled: self.enabled,
            teams_destination_name: if self.teams_destination_name.trim().is_empty() {
                default_destination()
            } else {
                self.teams_destination_name.trim().to_string()
            },
            teams_webhook_url: self.teams_webhook_url.trim().to_string(),
            station_id: self.station_id.trim().to_string(),
            station_name: if self.station_name.trim().is_empty() {
                self.station_id.trim().to_string()
            } else {
                self.station_name.trim().to_string()
            },
            idle_timeout_minutes: self.idle_timeout_minutes,
            events: self.events.clone(),
            summary_schedule_times: Vec::new(),
            // Compatibility placeholders — no external station/settings files required.
            settings_path: app_settings_path(),
            station_path: app_settings_path(),
        }
    }
}

/// UI-facing DTO: never put password in the happy-path status type.
#[derive(Clone, Serialize, Deserialize)]
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
}

impl From<&AppNotificationSettings> for AppNotificationSettingsView {
    fn from(value: &AppNotificationSettings) -> Self {
        Self {
            enabled: value.enabled,
            teams_destination_name: value.teams_destination_name.clone(),
            teams_webhook_url: value.teams_webhook_url.clone(),
            webhook_configured: !value.teams_webhook_url.trim().is_empty(),
            station_id: value.station_id.clone(),
            station_name: value.station_name.clone(),
            idle_timeout_minutes: value.idle_timeout_minutes,
            events: value.events.clone(),
            shared_shift_log_path: value.shared_shift_log_path.clone(),
        }
    }
}

pub fn station_name_for_id(station_id: &str) -> &'static str {
    match station_id {
        "test-station-1" => "Test Station 1",
        "test-station-2" => "Test Station 2",
        "test-station-3" => "Test Station 3",
        "test-station-4" => "Test Station 4",
        _ => "Unknown station",
    }
}
```

Export from `mod.rs`:

```rust
mod app_settings;
pub use app_settings::{
    change_password, load_app_settings, save_app_settings, set_app_config_dir,
    station_name_for_id, verify_password, AppNotificationSettings, AppNotificationSettingsView,
    AppSettingsError, DEFAULT_SETTINGS_PASSWORD,
};
```

- [ ] **Step 4: Run tests — expect PASS**

```powershell
cd C:\Projects\Active\PDU_Data_Automation_App\backend
cargo test --lib notifications::app_settings -- --nocapture
```

Expected: all `app_settings` tests pass.

- [ ] **Step 5: Commit**

```powershell
cd C:\Projects\Active\PDU_Data_Automation_App
git add backend/src/notifications/app_settings.rs backend/src/notifications/mod.rs
git commit -m "feat(notifications): add in-app AppData settings store with password gate defaults"
```

---

### Task 2: Point notification worker at app settings

**Files:**
- Modify: `backend/src/notifications/config.rs` (or add thin `load_runtime_config` in `app_settings.rs`)
- Modify: `backend/src/notifications/worker.rs` — replace `load_config()` / `can_send` sources
- Modify: `backend/src/lib.rs` — call `set_app_config_dir` in setup

- [ ] **Step 1: Add runtime loader used by worker**

In `app_settings.rs`:

```rust
pub fn load_runtime_resolved_config() -> Result<ResolvedConfig, AppSettingsError> {
    let settings = load_app_settings()?;
    Ok(settings.to_resolved_config())
}
```

In `worker.rs`, change `usable_config` and `status()` paths that call `load_config()` / `can_send` to:

```rust
use super::app_settings::load_runtime_resolved_config;
// ...
let config = match load_runtime_resolved_config() {
    Ok(config) => config,
    Err(error) => {
        set_status(status, "skipped", &format!("Notification configuration is unavailable: {error}"), None);
        return None;
    }
};
if let Err(error) = can_send(&config) {
    // same as today
}
```

Same for `NotificationService::status` idle branch.

Keep `config::load_config` / external JSON helpers for unit tests that still use fixtures; production worker must **not** require `C:\PDU500\...\station.json`.

- [ ] **Step 2: Wire config dir in `lib.rs` setup**

```rust
.setup(move |app| {
    if let Ok(resource_dir) = app.path().resource_dir() {
        config::set_runtime_resource_dir(resource_dir);
    }
    if let Ok(config_dir) = app.path().app_config_dir() {
        let _ = std::fs::create_dir_all(&config_dir);
        notifications::set_app_config_dir(config_dir);
    }
    commands::mark_window_setup_elapsed(process_start.elapsed());
    Ok(())
})
```

- [ ] **Step 3: Run notification + lib tests**

```powershell
cd C:\Projects\Active\PDU_Data_Automation_App\backend
cargo test --lib notifications -- --nocapture
```

Expected: pass (update any test that assumed external station file for worker status if needed).

- [ ] **Step 4: Commit**

```powershell
git add backend/src/notifications/worker.rs backend/src/notifications/app_settings.rs backend/src/lib.rs
git commit -m "feat(notifications): resolve Teams config from in-app settings store"
```

---

### Task 3: Tauri commands for settings + password + test ping

**Files:**
- Modify: `backend/src/commands.rs`
- Modify: `backend/src/lib.rs` invoke handler
- Modify: `frontend/src/integrations/tauri/backend.ts`

- [ ] **Step 1: Add Rust commands**

```rust
#[tauri::command]
pub fn get_app_notification_settings() -> Result<notifications::AppNotificationSettingsView, String> {
    let settings = notifications::load_app_settings().map_err(|e| e.to_string())?;
    Ok((&settings).into())
}

#[tauri::command]
pub fn verify_settings_password(password: String) -> Result<bool, String> {
    let settings = notifications::load_app_settings().map_err(|e| e.to_string())?;
    Ok(notifications::verify_password(&settings, &password))
}

#[derive(serde::Deserialize)]
pub struct SaveNotificationSettingsRequest {
    pub enabled: bool,
    pub teams_destination_name: String,
    pub teams_webhook_url: String,
    pub station_id: String,
    pub station_name: String,
    pub idle_timeout_minutes: u32,
    pub events: notifications::EventToggles,
    pub shared_shift_log_path: String,
}

#[tauri::command]
pub fn save_app_notification_settings(
    request: SaveNotificationSettingsRequest,
) -> Result<notifications::AppNotificationSettingsView, String> {
    let mut settings = notifications::load_app_settings().map_err(|e| e.to_string())?;
    settings.enabled = request.enabled;
    settings.teams_destination_name = request.teams_destination_name;
    // If UI sends empty webhook, keep existing (so masked "leave unchanged" works).
    if !request.teams_webhook_url.trim().is_empty() {
        settings.teams_webhook_url = request.teams_webhook_url.trim().to_string();
    }
    settings.station_id = request.station_id.trim().to_string();
    settings.station_name = if request.station_name.trim().is_empty() {
        notifications::station_name_for_id(&settings.station_id).to_string()
    } else {
        request.station_name.trim().to_string()
    };
    settings.idle_timeout_minutes = request.idle_timeout_minutes;
    settings.events = request.events;
    settings.shared_shift_log_path = request.shared_shift_log_path.trim().to_string();
    notifications::save_app_settings(&settings).map_err(|e| e.to_string())?;
    Ok((&settings).into())
}

#[derive(serde::Deserialize)]
pub struct ChangeSettingsPasswordRequest {
    pub current_password: String,
    pub new_password: String,
    pub confirm_password: String,
}

#[tauri::command]
pub fn change_settings_password(request: ChangeSettingsPasswordRequest) -> Result<(), String> {
    if request.new_password != request.confirm_password {
        return Err("New password and confirmation do not match".into());
    }
    let mut settings = notifications::load_app_settings().map_err(|e| e.to_string())?;
    notifications::change_password(
        &mut settings,
        &request.current_password,
        &request.new_password,
    )
    .map_err(|e| e.to_string())?;
    notifications::save_app_settings(&settings).map_err(|e| e.to_string())?;
    Ok(())
}
```

Register in `lib.rs` next to `send_notification_test`.

- [ ] **Step 2: Frontend API wrappers**

In `backend.ts`:

```typescript
export type EventToggles = {
  problem: boolean;
  complete: boolean;
  stuck: boolean;
  summary: boolean;
};

export type AppNotificationSettingsView = {
  enabled: boolean;
  teams_destination_name: string;
  teams_webhook_url: string;
  webhook_configured: boolean;
  station_id: string;
  station_name: string;
  idle_timeout_minutes: number;
  events: EventToggles;
  shared_shift_log_path: string;
};

export async function getAppNotificationSettings(): Promise<AppNotificationSettingsView | null> {
  if (!isTauriRuntime()) return null;
  return invoke<AppNotificationSettingsView>("get_app_notification_settings");
}

export async function verifySettingsPassword(password: string): Promise<boolean> {
  if (!isTauriRuntime()) return password === "0601";
  return invoke<boolean>("verify_settings_password", { password });
}

export async function saveAppNotificationSettings(
  request: Omit<AppNotificationSettingsView, "webhook_configured">,
): Promise<AppNotificationSettingsView | null> {
  if (!isTauriRuntime()) return null;
  return invoke<AppNotificationSettingsView>("save_app_notification_settings", { request });
}

export async function changeSettingsPassword(
  currentPassword: string,
  newPassword: string,
  confirmPassword: string,
): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("change_settings_password", {
    request: {
      current_password: currentPassword,
      new_password: newPassword,
      confirm_password: confirmPassword,
    },
  });
}

export async function sendNotificationTest(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("send_notification_test");
}
```

Note: Tauri 2 argument names must match command parameters (`password`, `request`).

- [ ] **Step 3: Manual smoke via `cargo check`**

```powershell
cd C:\Projects\Active\PDU_Data_Automation_App\backend
cargo check
```

Expected: success.

- [ ] **Step 4: Commit**

```powershell
git add backend/src/commands.rs backend/src/lib.rs frontend/src/integrations/tauri/backend.ts
git commit -m "feat(notifications): expose settings load/save/password and test-ping commands"
```

---

### Task 4: Password modal + Settings page UI

**Files:**
- Create: `frontend/src/features/settings/settingsTypes.ts`
- Create: `frontend/src/features/settings/SettingsPasswordModal.tsx`
- Create: `frontend/src/features/settings/SettingsPage.tsx`
- Create: `frontend/tests/SettingsPage.test.tsx` (optional but recommended)
- Modify: `frontend/src/features/test-panel/OperatorPanel.tsx`

- [ ] **Step 1: Password modal component**

Match existing modal styling from OperatorPanel (`fixed inset-0 z-50 flex ... bg-black/55`).

```tsx
// SettingsPasswordModal.tsx — props:
// open: boolean
// onCancel: () => void
// onUnlock: () => void
// verify: (password: string) => Promise<boolean>
```

- Single password input, Cancel, Unlock.
- On Unlock: call `verify`; if false show “Incorrect password”; if true `onUnlock()`.

- [ ] **Step 2: Settings page**

Layout:

- Header row: Back button (left, lucide `ArrowLeft`) + title “Notification settings”
- Station `<select>` options: `test-station-1` … `test-station-4` (update `station_name` via `stationNameForId` helper in TS)
- Destination name text input
- Webhook URL input `type="password"` (or text with reveal toggle)
- Enabled checkbox
- Shared shift log path text input + helper: “Leave blank until the shared path is confirmed.”
- Section **Change password**: new password, confirm, current password; button “Update password”
- **Test ping** button; show result from polling `getNotificationStatus` after click
- **Save** button; success/error line

On mount: `getAppNotificationSettings()`.

Station change handler:

```typescript
const STATIONS = [
  { id: "test-station-1", name: "Test Station 1" },
  { id: "test-station-2", name: "Test Station 2" },
  { id: "test-station-3", name: "Test Station 3" },
  { id: "test-station-4", name: "Test Station 4" },
] as const;
```

- [ ] **Step 3: Wire OperatorPanel**

State:

```typescript
type PanelView = "main" | "settings";
const [panelView, setPanelView] = useState<PanelView>("main");
const [passwordOpen, setPasswordOpen] = useState(false);
```

- Cog button top-right of main chrome (use lucide `Settings` icon). Place in a relative header wrapper above the timer or absolute `top-3 right-3` inside `<main>`.
- Click cog → `setPasswordOpen(true)`.
- Unlock → `setPasswordOpen(false); setPanelView("settings")`.
- When `panelView === "settings"`, render `<SettingsPage onBack={() => setPanelView("main")} />` instead of operator body (or full main content).
- On Back: `setPanelView("main")` (session re-locked — cog requires password again).

If Settings has dirty flag, `window.confirm` before leave.

- [ ] **Step 4: Frontend tests**

Update mocks in existing OperatorPanel tests if they render full panel (cog should not break).

Add:

```typescript
// SettingsPage.test.tsx
it("shows unlock error for wrong password", async () => { /* ... */ });
it("renders station dropdown and save", async () => { /* mock backend */ });
```

```powershell
cd C:\Projects\Active\PDU_Data_Automation_App
bun run test
```

Expected: pass.

- [ ] **Step 5: Commit**

```powershell
git add frontend/src/features/settings frontend/src/features/test-panel/OperatorPanel.tsx frontend/tests
git commit -m "feat(ui): password-gated notification settings page with test ping"
```

---

### Task 5: Optional shared shift log append (path-gated)

**Files:**
- Create: `backend/src/notifications/shift_log.rs`
- Modify: `backend/src/notifications/worker.rs` — after successful Problem/Complete POST
- Modify: `backend/src/notifications/mod.rs`

- [ ] **Step 1: Implement shift log module**

```rust
// shift_log.rs
// ShiftLog { events: Vec<LoggedEvent>, last_summary_at: Option<String> }
// LoggedEvent { station_id, station_name, kind, timestamp }
//
// pub fn append_event(path: &Path, event: LoggedEvent) -> Result<(), String>
// - if path string empty: Ok(()) immediately (caller checks)
// - load or default, push, atomic write
// - never panic; return Err for status only
```

Tests with `tempfile`: append two stations → two events persist.

- [ ] **Step 2: Call after successful Teams accept**

In `handle_problem` / `handle_complete` `Ok(())` branches, after receipt save:

```rust
let log_path = config /* need shared path on ResolvedConfig */;
```

Extend `ResolvedConfig` **or** re-load `AppNotificationSettings` for `shared_shift_log_path` only:

```rust
if let Ok(settings) = load_app_settings() {
    let path = settings.shared_shift_log_path.trim();
    if !path.is_empty() {
        let _ = shift_log::append_event(Path::new(path), LoggedEvent { ... });
    }
}
```

Do **not** fail the notification status to `failed` solely because log write failed; use a soft status message or log and keep `sent`.

- [ ] **Step 3: Tests + commit**

```powershell
cargo test --lib notifications::shift_log -- --nocapture
git add backend/src/notifications/shift_log.rs backend/src/notifications/worker.rs backend/src/notifications/mod.rs
git commit -m "feat(notifications): append optional shared shift log when path configured"
```

---

### Task 6: Docs + manual validation checklist

**Files:**
- Modify: `docs/NOTIFICATIONS.md`
- Modify: `docs/README.md` if it links notification docs

- [ ] **Step 1: Rewrite config section**

Document:

1. Cog → password (`0601` factory default) → Settings.
2. Set station + webhook → Save → Test ping.
3. Change password from Settings.
4. Store location: app config dir `notification_settings.json` (no hand-edited station.json required).
5. Shared shift log path: leave blank until confirmed; then paste full path (OneDrive/network) identical on all four PCs.
6. Soft-fail still applies.

- [ ] **Step 2: Manual desktop checklist (you run)**

```powershell
cd C:\Projects\Active\PDU_Data_Automation_App
bun run desktop
```

| Step | Expect |
|------|--------|
| Open app, footer may say inactive until webhook set | OK |
| Cog → wrong password | Error, stay on main |
| Cog → `0601` | Settings page |
| Set station 1, paste webhook, Save | Success |
| Test ping | Card in PDU Testing |
| Change password to something else, Back, unlock with old | Fail |
| Unlock with new password | OK |
| Run sample problem unit | One Problem card with correct station |
| Shared path blank | No errors |

- [ ] **Step 3: Commit docs**

```powershell
git add docs/NOTIFICATIONS.md docs/README.md docs/superpowers/specs/2026-07-10-in-app-notification-settings-design.md docs/superpowers/plans/2026-07-10-in-app-notification-settings.md
git commit -m "docs: in-app notification settings and optional shared shift log"
```

---

### Task 7 (after you confirm path): End-of-shift button (optional follow-up)

**Only after shared log path is confirmed and filled on all stations.**

- [ ] Add **Post end of shift** on Settings (or main panel after unlock).
- [ ] Read shared log → build same Adaptive Card style as pilot floor summary → POST Teams → mark `last_summary_at` / optional clear.
- [ ] Document which PC may press it (any station is fine if log is shared).

Do not block Tasks 1–6 on this.

---

## Implementation order (summary)

1. App settings store + password helpers (Rust tests)  
2. Worker reads app settings; Tauri sets config dir  
3. Commands + frontend API  
4. Cog / modal / Settings UI + test ping  
5. Optional shift-log append when path set  
6. Docs + your desktop validation  
7. Later: end-of-shift UI once path confirmed  

---

## Self-review (plan)

| Requirement | Task |
|-------------|------|
| Cog top-right | Task 4 |
| Password popup default 0601 | Task 1 + 4 |
| Settings replaces main; Back top-left | Task 4 |
| Change password in settings | Task 3 + 4 |
| In-app store (no operator JSON) | Task 1 + 2 |
| Station + webhook + enabled | Task 3 + 4 |
| Test ping | Task 3 + 4 (uses existing command) |
| Shared log path deferred-safe | Task 1 field + Task 5 path-gated |
| Soft-fail automation | Unchanged worker try_send pattern |
| Secrets not logged | Task 1 Debug redaction |

No TBD steps remain: empty `shared_shift_log_path` is a defined no-op until you paste the confirmed path in Settings on each PC.
