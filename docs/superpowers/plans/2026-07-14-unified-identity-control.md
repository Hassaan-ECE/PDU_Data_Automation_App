# Unified Identity Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the duplicate identity controls with one Advanced-only combobox and allow an intentional Identity save to create a missing `floor_settings.json` while keeping Admin identities out of operator surfaces.

**Architecture:** The frontend uses one combobox for selection and creation, with rename fields shown only after an explicit Rename action. The backend retains the floor read-modify-write lock, but Identity scope may construct a seed when the file is absent; loads and non-Identity scopes retain no-reseed behavior.

**Tech Stack:** Rust, Tauri 2, React 19, TypeScript, Vitest, Testing Library, Bun.

## Global Constraints

- Work only in `C:\Projects\Active\PDU_Data_Automation_App` on `main`; do not create another worktree.
- Keep version `0.2.14`; do not build an installer, publish, tag, or push.
- Admin identities remain visible in password-locked Advanced identity selection only.
- Admin identities never appear in Main choices, included-station rows, or shift summaries.
- Loads, polling, Teams saves, operator saves, and password changes never recreate a missing floor file.
- Never print or commit the shared password or webhook.

---

### Task 1: Seed a missing floor only for an intentional Identity save

**Files:**
- Modify: `backend/src/notifications/app_settings.rs`
- Test: `backend/src/notifications/app_settings.rs`

**Interfaces:**
- Consumes: `save_scoped_policy_unlocked`, `FloorSettings::from_local_settings`, `apply_catalog_patch`, and `update_floor_settings_with_lock`.
- Produces: Identity scope turns `existing: None` into a seed before catalog patching; other scopes retain the unavailable-floor error.

- [ ] **Step 1: Write the failing regression test**

Add beside the existing Identity-scope tests:

```rust
#[test]
fn identity_create_seeds_missing_floor_and_selects_new_admin_locally() {
    let dir = tempfile::tempdir().unwrap();
    let local_path = dir.path().join("notification_settings.json");
    let shared = dir.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();

    let mut local = AppNotificationSettings::default();
    local.shared_shift_log_path = shared.to_string_lossy().into_owned();
    save_app_settings_to(&local_path, &local).unwrap();

    let mut request = advanced_request();
    request.scope = SettingsSaveScope::Identity;
    request.shared_shift_log_path = local.shared_shift_log_path.clone();
    request.catalog_create = Some(CatalogCreateRequest {
        name: "Admin Desk".to_string(),
        role: StationRole::Admin,
        select_for_this_pc: true,
    });

    let saved = save_app_settings_request_to(&local_path, &request).unwrap();
    let floor = try_load_floor_settings(&local.shared_shift_log_path)
        .unwrap()
        .expect("Identity save should seed the missing floor file");
    let admin = floor.stations.iter().find(|station| station.name == "Admin Desk").unwrap();
    assert_eq!(admin.role, StationRole::Admin);
    assert_eq!(saved.station_id, admin.id);
    assert_eq!(saved.station_name, "Admin Desk");
}
```

Keep `missing_floor_after_connect_does_not_reseed_on_load` unchanged.

- [ ] **Step 2: Run the focused test for RED**

```powershell
cargo test --manifest-path backend/Cargo.toml --locked notifications::app_settings::tests::identity_create_seeds_missing_floor_and_selects_new_admin_locally -- --exact
```

Expected: failure containing `Floor settings are unavailable`.

- [ ] **Step 3: Implement the Identity-only seed under the floor lock**

In `save_scoped_policy_unlocked`, initialize the floor as follows before applying the catalog patch:

```rust
let mut floor = match existing {
    Some(floor) => floor,
    None if scope == SettingsSaveScope::Identity => FloorSettings::from_local_settings(settings),
    None => {
        return Err(super::floor_settings::FloorSettingsError::Read(
            "Floor settings are unavailable; cannot save floor policy until the shared file is readable."
                .to_string(),
        ));
    }
};
```

Keep `apply_scope_to_floor`, `apply_catalog_patch`, timestamps, validation, and atomic write inside the existing `update_floor_settings_with_lock` closure.

- [ ] **Step 4: Run notification tests for GREEN**

```powershell
cargo test --manifest-path backend/Cargo.toml --locked notifications:: -- --test-threads=1
```

Expected: all notification tests pass, including no-load-reseed coverage.

- [ ] **Step 5: Commit**

```powershell
git add backend/src/notifications/app_settings.rs
git commit -m "fix: seed missing floor on identity save"
```

---

### Task 2: Replace duplicate identity controls with one control

**Files:**
- Modify: `frontend/src/features/settings/NotificationSettingsPage.tsx`
- Test: `frontend/tests/NotificationSettings.test.tsx`

**Interfaces:**
- Consumes: `IdentityCombobox`, `CatalogCreateRequest`, Identity scope, and the backend response containing generated ids.
- Produces: one This PC identity combobox, automatic selection for new identities, and explicit rename mode.

- [ ] **Step 1: Update tests for the unified interaction**

Require one identity combobox:

```ts
await unlockAdvancedIdentities();
expect(screen.getByRole("combobox", { name: "This PC identity" })).toBeInTheDocument();
expect(screen.queryByRole("combobox", { name: "Manage identities" })).not.toBeInTheDocument();
```

For Admin creation, type in the single control, select the Add option, Save, and assert:

```ts
expect(saveSettings).toHaveBeenCalledWith(
  expect.objectContaining({
    scope: "identity",
    catalog_create: {
      name: "Admin Desk",
      role: "admin",
      select_for_this_pc: true,
    },
  }),
);
```

Add a visibility test where an Admin entry appears in the Advanced combobox options but is absent from Summary options and Main-poster radios.

- [ ] **Step 2: Run the Settings test for RED**

```powershell
bun --bun vitest run --config frontend/vite.config.ts frontend/tests/NotificationSettings.test.tsx
```

Expected: failures because Manage identities still renders and new creation stages `select_for_this_pc: false`.

- [ ] **Step 3: Consolidate state and UI**

In `NotificationSettingsPage.tsx`:

- Keep `thisPcIdentityQuery` as the only always-visible identity input.
- Set `allowCreate` on that combobox.
- Selecting an existing entry updates `station_id` and clears create/rename state.
- Adding stages `setCatalogCreate({ name, role, select_for_this_pc: true })` and keeps the typed name visible.
- Permit Save when `catalogCreate` exists even though its generated id is not available yet.
- Replace Manage identities with a metadata row for the selected id/role and a Rename button.
- Rename mode renders `Rename selected identity` plus Cancel; editing updates only the selected catalog entry and local display name.
- Show `Will be used on this PC` for staged creation; remove the old checkbox.
- Continue deriving operator rows exclusively from `stations.filter((entry) => entry.role === "floor")`.

The selected metadata row follows this structure:

```tsx
<div className="flex items-center justify-between gap-2">
  <div>{settings.station_id} · {identityRoleLabel(selectedIdentity.role)}</div>
  <button type="button" onClick={beginRename}>Rename</button>
</div>
```

- [ ] **Step 4: Run focused frontend tests for GREEN**

```powershell
bun --bun vitest run --config frontend/vite.config.ts frontend/tests/NotificationSettings.test.tsx frontend/tests/OperatorPanelSetup.test.tsx
```

Expected: all Settings and operator integration tests pass.

- [ ] **Step 5: Commit**

```powershell
git add frontend/src/features/settings/NotificationSettingsPage.tsx frontend/tests/NotificationSettings.test.tsx
git commit -m "feat: unify identity selection and management"
```

---

### Task 3: Documentation and full verification

**Files:**
- Modify: `docs/NOTIFICATIONS.md`
- Modify: `docs/OPERATOR_NOTIFICATION_CATALOG.md`

**Interfaces:**
- Consumes: final behavior from Tasks 1 and 2.
- Produces: matching operator/admin documentation and fresh verification evidence.

- [ ] **Step 1: Update documentation**

Document that Advanced uses one identity control, new identities are automatically assigned to this PC, Admin identities stay out of operator surfaces, and only an intentional Identity save may create a missing floor file.

- [ ] **Step 2: Run full verification**

```powershell
cargo fmt --manifest-path backend/Cargo.toml -- --check
cargo test --manifest-path backend/Cargo.toml --locked -- --test-threads=1
cargo check --manifest-path backend/Cargo.toml --locked
bun run test
bun node_modules/eslint/bin/eslint.js .
bun run build
bun scripts/release/check-version-consistency.mjs
git diff --check
git status --short
```

Expected: every command exits zero and version remains `0.2.14`.

- [ ] **Step 3: Commit docs and plan**

```powershell
git add docs/NOTIFICATIONS.md docs/OPERATOR_NOTIFICATION_CATALOG.md docs/superpowers/plans/2026-07-14-unified-identity-control.md
git commit -m "docs: describe unified identity workflow"
```

- [ ] **Step 4: Manual live-app check**

Restart `bun run desktop` from main. Add an Admin identity and Save. Confirm `floor_settings.json` appears, the returned Admin identity is selected for this PC, and Summary options still list Floor Stations only.
