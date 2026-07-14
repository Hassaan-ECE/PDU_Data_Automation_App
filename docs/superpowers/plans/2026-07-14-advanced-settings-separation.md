# Advanced Settings Separation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the combined Station & Teams page with independently saved Station & Identities and Teams & Notifications pages.

**Architecture:** Add granular `identity` and `teams` save scopes while retaining `advanced` as a backward-compatible backend input. Split the React view into two sibling Advanced routes backed by the same merged settings DTO, with page-specific controls and scope resolution.

**Tech Stack:** Rust, Tauri 2 command DTOs, React, TypeScript, Vitest, Testing Library, Bun.

## Global Constraints

- Both pages remain behind the existing Advanced password.
- Identity saves must not rewrite Teams policy; Teams saves must not rewrite identity/catalog data.
- `connect`, `local`, and `operator` behavior remains unchanged.
- Shared mutations continue using the floor read-modify-write lock.
- No settings schema, dependency, application version, installer, updater, tag, push, or release change.
- The running `bun run desktop` app should receive the frontend change through Vite hot reload.

## File Map

| File | Responsibility |
|---|---|
| `backend/src/notifications/app_settings.rs` | Save-scope enum, granular local/floor mutation rules, backend unit tests |
| `frontend/src/features/settings/settingsTypes.ts` | TypeScript scope union and view-to-scope resolution |
| `frontend/src/integrations/tauri/backend.ts` | Tauri bridge scope union |
| `frontend/src/features/settings/NotificationSettingsPage.tsx` | Advanced menu, two page views, page-specific controls/dirty navigation |
| `frontend/tests/NotificationSettings.test.tsx` | Access, separation, request-scope, regression, and polling tests |
| `docs/NOTIFICATIONS.md` | Operator/admin navigation names |
| `docs/OPERATOR_NOTIFICATION_CATALOG.md` | Updated Settings access catalog |

---

### Task 1: Add independent backend identity and Teams save scopes

**Files:**
- Modify: `backend/src/notifications/app_settings.rs`

**Interfaces:**
- Consumes: `SaveAppNotificationSettingsRequest`, `SettingsSaveScope`, existing floor lock/update functions
- Produces: `SettingsSaveScope::Identity`, `SettingsSaveScope::Teams`, isolated local/floor mutations

- [ ] **Step 1: Write failing backend isolation tests**

Add tests beside the existing scoped-save tests. Create a connected floor, then prove each scope preserves the other section:

```rust
#[test]
fn teams_scoped_save_preserves_identity_catalog_and_local_station() {
    let (_temp, path, shared) = connected_settings_fixture();
    let before = load_app_settings_from(&path).unwrap();
    let mut request = request_from(&before);
    request.scope = SettingsSaveScope::Teams;
    request.station_id = "pdu-lab".to_string();
    request.stations[0].name = "STALE NAME".to_string();
    request.teams_destination_name = "New Teams Destination".to_string();

    let saved = save_app_settings_request_to(&path, &request).unwrap();
    let floor = load_floor_settings(&shared).unwrap().unwrap();

    assert_eq!(saved.station_id, before.station_id);
    assert_ne!(floor.stations[0].name, "STALE NAME");
    assert_eq!(floor.teams_destination_name, "New Teams Destination");
}

#[test]
fn identity_scoped_save_preserves_teams_policy() {
    let (_temp, path, shared) = connected_settings_fixture();
    let before_floor = load_floor_settings(&shared).unwrap().unwrap();
    let settings = load_app_settings_from(&path).unwrap();
    let mut request = request_from(&settings);
    request.scope = SettingsSaveScope::Identity;
    request.station_id = "pdu-lab".to_string();
    request.stations[0].name = "Bay One".to_string();
    request.enabled = !before_floor.enabled;
    request.teams_destination_name = "STALE DESTINATION".to_string();

    let saved = save_app_settings_request_to(&path, &request).unwrap();
    let floor = load_floor_settings(&shared).unwrap().unwrap();

    assert_eq!(saved.station_id, "pdu-lab");
    assert_eq!(floor.stations[0].name, "Bay One");
    assert_eq!(floor.enabled, before_floor.enabled);
    assert_eq!(floor.teams_destination_name, before_floor.teams_destination_name);
}
```

Use the file's existing request/temp/shared-floor helpers rather than introducing a second fixture framework.

- [ ] **Step 2: Run the new tests and verify RED**

Run:

```powershell
cargo test --manifest-path backend/Cargo.toml teams_scoped_save_preserves_identity_catalog_and_local_station
cargo test --manifest-path backend/Cargo.toml identity_scoped_save_preserves_teams_policy
```

Expected: compilation fails because `Identity` and `Teams` do not exist.

- [ ] **Step 3: Add granular enum variants and routing**

Extend the serde snake-case enum while keeping `Advanced` readable for compatibility:

```rust
pub enum SettingsSaveScope {
    #[default]
    Operator,
    Identity,
    Teams,
    /// Backward-compatible combined scope for older callers.
    Advanced,
    Connect,
    Local,
}
```

Route `Identity`, `Teams`, and `Advanced` through `save_scoped_policy_unlocked` in `save_app_settings_request_unlocked`.

- [ ] **Step 4: Isolate local and floor mutation rules**

In `apply_scope_to_local`:

```rust
SettingsSaveScope::Identity => {
    apply_station_id_from_request(settings, request)?;
    apply_station_names_to_catalog(settings, &request.stations)?;
}
SettingsSaveScope::Teams => {
    apply_teams_scope_to_local(settings, request);
}
SettingsSaveScope::Advanced => {
    apply_teams_scope_to_local(settings, request);
    apply_station_id_from_request(settings, request)?;
    apply_station_names_to_catalog(settings, &request.stations)?;
}
```

Create focused helpers that assign `enabled`, destination, webhook replacement, idle timeout, and Problem/Complete/Changeover/Stuck toggles. `apply_scope_to_floor` applies that helper only for `Teams | Advanced`; `Identity` performs no policy assignment there.

In `save_scoped_policy_unlocked`, apply local station selection and `apply_catalog_patch` only for `Identity | Advanced`. Reject `catalog_create` without a shared path exactly as today.

- [ ] **Step 5: Run backend scope and notification tests**

Run:

```powershell
cargo fmt --manifest-path backend/Cargo.toml
cargo test --manifest-path backend/Cargo.toml teams_scoped_save_preserves_identity_catalog_and_local_station
cargo test --manifest-path backend/Cargo.toml identity_scoped_save_preserves_teams_policy
cargo test --manifest-path backend/Cargo.toml notifications::
```

Expected: both isolation tests and the complete notification suite pass.

- [ ] **Step 6: Commit backend scopes**

```powershell
git add backend/src/notifications/app_settings.rs
git diff --cached --check
git commit -m "fix: isolate identity and Teams settings saves"
```

---

### Task 2: Split the Advanced React page

**Files:**
- Modify: `frontend/src/features/settings/settingsTypes.ts`
- Modify: `frontend/src/integrations/tauri/backend.ts`
- Modify: `frontend/src/features/settings/NotificationSettingsPage.tsx`
- Test: `frontend/tests/NotificationSettings.test.tsx`

**Interfaces:**
- Consumes: backend `identity` and `teams` scope strings, existing merged settings DTO
- Produces: `SettingsView` values `identities` and `teams`, separate Advanced pages and request scopes

- [ ] **Step 1: Replace combined-page assertions with failing separation tests**

Update the Advanced access test and add page-content/scope tests:

```tsx
expect(await screen.findByRole("button", { name: "Station & Identities" })).toBeInTheDocument();
expect(screen.getByRole("button", { name: "Teams & Notifications" })).toBeInTheDocument();
expect(screen.queryByRole("button", { name: "Station & Teams" })).not.toBeInTheDocument();

fireEvent.click(screen.getByRole("button", { name: "Station & Identities" }));
expect(await screen.findByRole("heading", { name: "Station & Identities" })).toBeInTheDocument();
expect(screen.getByRole("combobox", { name: "This PC identity" })).toBeInTheDocument();
expect(screen.getByLabelText("Shared OneDrive folder")).toBeInTheDocument();
expect(screen.queryByLabelText("Teams webhook URL")).not.toBeInTheDocument();

fireEvent.click(screen.getByRole("button", { name: "Back to settings menu" }));
fireEvent.click(screen.getByRole("button", { name: "Teams & Notifications" }));
expect(await screen.findByRole("heading", { name: "Teams & Notifications" })).toBeInTheDocument();
expect(screen.getByLabelText("Teams webhook URL")).toBeInTheDocument();
expect(screen.queryByRole("combobox", { name: "This PC identity" })).not.toBeInTheDocument();
expect(screen.queryByLabelText("Shared OneDrive folder")).not.toBeInTheDocument();
```

Add one test that edits an identity and expects `scope: "identity"`, plus one that toggles Changeover and expects `scope: "teams"` with no `catalog_create`.

- [ ] **Step 2: Run Settings tests and verify RED**

Run:

```powershell
bun --bun vitest run --config frontend/vite.config.ts frontend/tests/NotificationSettings.test.tsx
```

Expected: fails because the two menu pages and scope strings do not exist.

- [ ] **Step 3: Extend TypeScript scopes and resolution**

In both TypeScript scope unions:

```ts
export type SettingsSaveScope =
  | "operator"
  | "identity"
  | "teams"
  | "advanced"
  | "connect"
  | "local";
```

Update `resolveSaveScope` after connect/local path checks:

```ts
if (view === "identities") return { scope: "identity" };
if (view === "teams") return { scope: "teams" };
if (view === "advanced" || view === "password" || view === "home") {
  return { scope: "advanced" };
}
```

Connect/local path changes are evaluated first so Browse/Clear on Station & Identities retains current behavior.

- [ ] **Step 4: Split view names, titles, navigation, and menu entries**

Replace the `station` view with:

```ts
export type SettingsView =
  | "home"
  | "shifts"
  | "summaryOptions"
  | "advanced"
  | "identities"
  | "teams"
  | "password";
```

Render sibling menu entries:

```tsx
<MenuButton icon={Radio} title="Station & Identities" onClick={() => setView("identities")} />
<MenuButton icon={Bell} title="Teams & Notifications" onClick={() => setView("teams")} />
```

Update the page-title switch and Back navigation so both return to Advanced.

- [ ] **Step 5: Move controls without duplicating state**

Render the existing identity combobox, manager, folder, Connect password, sync status, and Save row only under `view === "identities"`.

Render destination, webhook, Notifications enabled, Changeover toggle, sync-status context, Test Ping, and Save row only under `view === "teams"`.

Use `stationSettingsDirty` for the identities Save row and `settingsDirty` for Teams. Keep identity validation (`This PC` selection and staged Add choice) limited to the identities view. Successful save behavior remains shared.

- [ ] **Step 6: Update test helpers and run focused GREEN checks**

Rename the helper to `unlockAdvancedIdentities`, navigate using the new menu name, and update existing identity tests. Add a Teams helper where needed.

Run:

```powershell
bun --bun vitest run --config frontend/vite.config.ts frontend/tests/NotificationSettings.test.tsx frontend/tests/OperatorPanelSetup.test.tsx
bun --bun eslint frontend/src/features/settings frontend/src/integrations/tauri/backend.ts frontend/tests/NotificationSettings.test.tsx
bun run build
```

Expected: Settings/operator tests, lint, and production build pass. The running desktop app should hot-reload and show the two menu entries after reopening Advanced.

- [ ] **Step 7: Commit frontend split**

```powershell
git add frontend/src/features/settings/settingsTypes.ts frontend/src/integrations/tauri/backend.ts frontend/src/features/settings/NotificationSettingsPage.tsx frontend/tests/NotificationSettings.test.tsx
git diff --cached --check
git commit -m "feat: separate identity and Teams settings pages"
```

---

### Task 3: Update docs and verify the complete branch

**Files:**
- Modify: `docs/NOTIFICATIONS.md`
- Modify: `docs/OPERATOR_NOTIFICATION_CATALOG.md`

**Interfaces:**
- Consumes: completed backend scopes and UI names
- Produces: accurate operator/admin instructions and final verification evidence

- [ ] **Step 1: Update Settings navigation copy**

Replace combined-page references with:

```text
Advanced → Station & Identities
Advanced → Teams & Notifications
```

Document that the pages save independently and retain existing password/shared-floor behavior.

- [ ] **Step 2: Run final verification as independent gates**

Run:

```powershell
cargo fmt --manifest-path backend/Cargo.toml -- --check
cargo test --manifest-path backend/Cargo.toml -- --test-threads=1
cargo check --manifest-path backend/Cargo.toml --locked
bun run test
bun --bun eslint frontend/src frontend/tests
bun run build
bun scripts/release/check-version-consistency.mjs
git diff --check
```

Expected: all tests/lint/build/checks pass; version remains `0.2.14`.

- [ ] **Step 3: Commit documentation**

```powershell
git add docs/NOTIFICATIONS.md docs/OPERATOR_NOTIFICATION_CATALOG.md
git diff --cached --check
git commit -m "docs: separate identity and Teams settings guidance"
```

- [ ] **Step 4: Manual hot-reload handoff**

Ask the user to reopen Advanced in the running desktop app and confirm:

1. Station & Identities shows no webhook/toggles.
2. Teams & Notifications shows no identity/folder editors.
3. Back returns to the Advanced menu from each page.

Do not claim live Teams delivery, OneDrive synchronization, installer behavior, or multi-PC behavior from this visual check.
