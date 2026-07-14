# Teams operator notifications

The standalone `PDU_Notifier` pilot established the Teams Workflow and Adaptive Card format. The production app uses that format for real automation events and now keeps its notification settings inside the app. Operators do not normally create or edit `station.json` or `settings.json` files.

**Operator / floor catalog (what cards exist and when):** [OPERATOR_NOTIFICATION_CATALOG.md](./OPERATOR_NOTIFICATION_CATALOG.md)

## Current event scope

- **Problem:** sent after a committed task result enters `fail` or a blocking `warning`.
- **Changeover:** sent once after `208v-breaker-8-20% Load` (STEP41) commits a pass, prompting the STEP42 shutdown/tap change before STEP43.
- **Complete:** sent only after the Rust print-readiness gate confirms that the print report exists, the Transformer SN is saved, and every task is passed or explicitly accepted.
- Successful receipts are stored in the unit's `unit_state.json` to reduce duplicate cards. A Problem receipt is cleared after that task passes; Changeover and Complete are each sent once per unit state.
- Delivery runs on the notification worker, outside the CSV/Excel command path. Missing settings, a full queue, a timeout, a shared-log error, or a Teams failure must never change the automation result.

This first production slice does not convert top-level setup, scan, layout/configuration, Transformer-SN-write, or print-dialog command errors into Teams cards. Those errors remain visible in the app. Delivery is best effort and has no automatic HTTP retry: a timeout can happen after Teams accepted a card, so an immediate retry could create a duplicate. A failed delivery leaves no receipt and can be attempted again when a later qualifying task or Transformer SN event rechecks the unit.

## Configure notifications in the app

1. Select the **cog** in the top-right of the operator panel.
2. Enter the Settings password. The factory default is `0601`.
3. Configure the station, destination label, Teams Workflow URL, and enabled state.
4. Select **Save**.
5. Select **Test ping** and review the result shown in Settings and the notification runtime status.
6. Use **Back** to return to the operator panel. Back re-locks Settings, so the password is required next time.

An incorrect password shows an inline error and does not open Settings. If the form has unsaved edits, Back asks for confirmation before discarding them.
The cog is disabled while report setup or task automation is active so station and destination identity cannot change halfway through a unit run.

### Settings behavior

| Setting | Behavior |
|---------|----------|
| This PC identity | **Advanced → Station & Identities.** Searchable combobox containing Floor Stations and Admin Identities. The selection is local to this PC; selecting an Admin Identity is allowed. |
| Manage identities | **Advanced → Station & Identities.** Select an existing identity to rename it without changing its stable id, or type a unique name and add it as a **Floor Station** or **Admin Identity**. New identities require a configured shared floor. Delete and role conversion are intentionally unavailable. |
| Destination name | **Advanced → Teams & Notifications.** Defaults to `PDU Testing` and labels the configured destination in the UI/status. Shared when floor sync is on. It does not route the message; the Workflow URL determines the actual Teams chat. |
| Teams webhook URL | **Advanced → Teams & Notifications.** Masked in the UI. Paste a non-empty URL to set or replace it. Leaving the edit blank preserves an already stored URL; disable notifications if delivery should stop. Shared when floor sync is on. |
| Notifications enabled | **Advanced → Teams & Notifications.** Master switch for automatic cards and test delivery. Save the enabled state before testing. Shared when floor sync is on. |
| Changeover card | **Advanced → Teams & Notifications.** Toggle, default on. Controls the one-time STEP42 action card after the committed STEP41 pass. It does not change automation or STEP43 readiness. |
| Shared OneDrive folder | **Advanced → Station & Identities.** Optional. Use **Browse** on each PC to pick that machine’s local sync of the same org OneDrive folder (confirmed name: `.PDU_Notifications` under the org OneDrive root). Never hard-code the absolute path — usernames and OneDrive roots differ per PC. On first Connect the app seeds `floor_settings.json` if missing, or adopts an existing floor (requires the floor Settings password). Creates `shift_log.json` and `stations/*` as needed. Leave empty / clear for local-only mode. |
| Main poster / included stations / shifts | Operator-open controls (no password). Shared when floor sync is on so every PC agrees who posts and which windows apply. Saved with **operator** scope only. |
| Change password | Enter the current password, a new non-empty password, and matching confirmation. Shared when floor sync is on so every PC uses the same Advanced unlock. Uses the dedicated change-password API (not a full settings save). |
| Save (scoped) | Saves only the fields for the current form section so a stale open Settings page cannot overwrite peer edits. **operator** = shifts / summary / Main / included Floor Stations; **identity** = identity rename/create and this-PC identity; **teams** = webhook, destination, notification toggles, and idle timeout; **advanced** remains accepted for older callers; **connect** = set shared path and adopt or seed floor (does not write form policy over an existing floor; needs `connect_password` when the floor already exists); **local** = this-PC identity and clearing the shared path without writing floor policy. |
| Test ping | **Advanced → Teams & Notifications.** Uses the saved, enabled configuration and posts a blue connection card through the same Workflow. Its asynchronous result is correlated to the test event, so a concurrent Problem or Complete result is not mistaken for the ping. |

## Settings persistence

### Local (this PC)

Each install stores a local pointer file under Tauri's per-user `app_config_dir`:

```text
<Tauri app_config_dir>\notification_settings.json
```

For the current application identifier, the Windows location is normally equivalent to:

```text
%APPDATA%\com.te.lab.pdu-data-automation\notification_settings.json
```

**Local-only fields:** this PC's station id, and the path to the shared notifications folder.

### Floor-wide (shared folder)

When Advanced → **Station & Identities → Browse** points at a shared OneDrive/network folder, floor-wide settings live in:

```text
<shared-folder>\floor_settings.json
```

alongside the existing `shift_log.json` and `stations/*` layout.

**Shared fields:** Settings password, Teams webhook, destination name, notification enable/event toggles, shift windows, Main poster, Floor Stations included on the summary card, and the shared identity catalog. The historical ids remain valid; new ids are generated by the backend and are never derived from editable display names.

- First PC to **Connect** when `floor_settings.json` is missing **seeds** it from that PC’s local settings (explicit connect only — ordinary loads never reseed a missing file).
- Later PCs that Browse the same folder and **Save** with **connect** scope **adopt** the existing floor after the floor password matches (they keep only their own station id). Connect does **not** write the rest of the form over peer policy.
- **Delivery** always merges floor settings on the backend worker poll (~45s). The open Settings UI also re-fetches about every **45 seconds**, but only while the form is **clean** (unsaved edits are never clobbered by a peer reload).
- Multi-PC OneDrive concurrency is best-effort: atomic replace + short locks reduce same-machine races; across replicas last writer still wins after sync lag.
- Confirmed operator shared folder name: `.PDU_Notifications` under org OneDrive. Each PC must Browse its own synced copy — do not hard-code a user-specific absolute path.
- Admin workflow: install the same app on any machine, Advanced → Station & Identities → Browse → Connect (floor password). Manage identities there, and use Advanced → Teams & Notifications for Teams policy. Peers pick saved changes up on the next clean UI poll / delivery poll.

### Dynamic identity compatibility boundary

`floor_settings.json` stays at schema 1 while the historical catalog is only read or renamed. Creating the first new Floor Station or Admin Identity upgrades it to schema 2. **Upgrade every PC that uses this shared floor before creating the first new identity.** A v0.2.14 client rejects schema 2 rather than rewriting it, but it must not remain responsible for settings edits after the upgrade.

- `floor` identities can be Main, appear in summary controls, and receive a `stations/<stable-id>/` directory.
- `admin` identities can identify a desk/admin copy of the app and appear on its cards/status, but cannot be Main or included in shift summaries.
- Identity creation and rename run inside the floor read-modify-write lock so a stale partial form cannot omit/delete another client's identity.
- OneDrive coordination remains best effort across separately synchronized replicas; wait for sync before making simultaneous catalog changes on different PCs.

Without a shared folder path, the app stays fully local (single-PC behavior).

Use the path returned by Tauri as authoritative for AppData; Windows/Tauri may resolve the base directory differently by environment. The app does not fall back to `%TEMP%` if that directory is unavailable. A missing local store loads factory defaults: password `0601`, Test Station 1, destination `PDU Testing`, notifications enabled, Problem and Complete enabled, an empty webhook, and an empty shared shift-log path. **Save** creates or updates the local file and, when configured, the floor file. On Windows, an interrupted local replace recovers a validated synced temp file or prior backup rather than silently reverting a saved password/webhook to factory defaults.

These files are app-owned and should not be hand-edited as the normal operator workflow. Old external notification JSON and `PDU_NOTIFICATIONS_STATION_PATH` are not required for normal floor use.

## Password and secret security

The Settings password is deliberate operator friction, not an authentication or cryptographic security boundary.

- The password is stored as plaintext in the per-user JSON file; it is not hashed and is not kept in Windows Credential Manager.
- A person with access to the Windows account or settings file can read or change it. The default `0601` should therefore be changed when stronger operator deterrence is desired, but changing it does not protect against an administrator or filesystem access.
- Masking the password and webhook in the UI reduces casual observation only.
- The Workflow URL contains a signed credential and is also stored locally in the settings JSON. Never commit it, paste it into a ticket/chat, or include it in logs or screenshots.
- Debug/status output must redact both the password and full webhook URL.
- Back re-locks Settings; an app restart also starts locked.

Use an HTTPS Teams Workflow URL. Access to the Windows account and the app-config directory should remain limited to the intended station account and administrators.

## Power Automate workflow

The existing Workflow must use the card action:

```text
When a Teams webhook request is received
└─ Post card in a chat or channel
   └─ Adaptive Card: first(triggerBody()?['attachments'])?['content']
```

Remove the plain **Post message in a chat or channel** action. The app retains a root `text` value only as a compatibility fallback; the Workflow renders `attachments[0].content`.

## Runtime status and soft failures

Notification delivery status is not shown on the operator-panel footer (to keep the main test UI clean). Open Notification settings to send a Test ping and see its result. A successful HTTP response means the Workflow accepted the card; it does not by itself prove that every phone displayed a push notification.

Notification failures are soft-fail only. CSV detection, verification, report writes, task state, and print readiness continue independently when notification delivery or the optional shared log fails.

## Optional shared OneDrive folder

The shared folder may remain blank. When blank, no shared-log I/O occurs and multi-station rollup is disabled.

Recommended setup:

1. Under the org OneDrive (or another path every station can read/write), create the shared folder **`.PDU_Notifications`** (hidden is fine).
2. On each station: Advanced → **Browse** → select that PC’s local sync of the same folder → **Save** (Connect scope; existing floors require the floor password).
3. The app creates this layout inside the chosen folder:

```text
<shared-folder>/
  floor_settings.json
  shift_log.json
  stations/
    test-station-1/
    test-station-3/
    test-station-4/
    pdu-lab/
    identity-.../          # created for additional Floor Stations only
```

`shift_log.json` is the floor-wide Problem/Complete event ledger. Changeover is deliberately excluded. The per-station folders are reserved for future station-local shared state. After Teams accepts a Problem or Complete event, the app appends to `shift_log.json` with short coordination/atomic replacement so multiple stations can contribute. OneDrive sync lag, permissions, or network failures can still occur. A log failure is status-only: it must not change the accepted Teams result or automation outcome.

Legacy settings that stored a direct `*.json` file path still work; the app treats a `.json` path as the log file and creates `stations/*` beside it.

## Deferred events

- **Stuck:** deferred until the app owns a meaningful-progress clock. The expected STEP71 system burn-in runs for roughly two hours, so a flat 30-minute inactivity timer would generate false alerts.
- **Summary scheduling:** the operator can preview/post the summary manually; automatic timed posting remains deferred.
- Two-way Teams commands remain out of scope.

The stored `idle_timeout_minutes` and Stuck toggle do not mean Stuck posting is active in this release.

## Validation required before release

Automated tests and local builds do not establish live floor delivery or installer behavior. Before calling the feature station-ready:

1. Run the notification/settings tests and full repository validation.
2. Install a development build on one station.
3. Open cog → Settings with `0601`, choose the correct station, paste the existing Workflow URL, enable notifications, and Save.
4. Send Test ping and confirm the card reaches only the intended `PDU Testing` chat.
5. Change the Settings password, go Back, and confirm the old password fails while the new password succeeds.
6. Induce one safe task failure and confirm exactly one red Problem card with the correct station, unit, and task.
7. Resolve/rerun the task and confirm a later genuine failure can alert again.
8. Process a safe fixture unit to the real print-ready state and confirm exactly one green Complete card.
9. On a fresh fixture unit, pass `208v-breaker-8-20% Load` and confirm exactly one yellow Changeover card names the STEP42 action and STEP43 next step; rerun STEP41 and confirm it does not duplicate.
10. Temporarily use an invalid webhook and confirm CSV processing and workbook writes still finish normally while the UI reports delivery failure.
11. Leave the shared OneDrive folder blank and confirm there are no log errors. After you create a hidden shared folder, Browse to it on each station, Save, and confirm `shift_log.json` plus `stations/test-station-*` appear under that folder.
12. Upgrade all test clients, add one Admin Identity and one Floor Station, and confirm Admin is absent from Main/summary while the Floor Station appears there and receives a stable-id directory.
13. Confirm the app-owned settings survive an application update.

No live webhook, phone-notification, shared-path, or installer validation is claimed by this document.
