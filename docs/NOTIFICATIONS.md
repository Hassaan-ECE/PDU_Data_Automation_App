# Teams operator notifications

The standalone `PDU_Notifier` pilot established the Teams Workflow and Adaptive Card format. The production app uses that format for real automation events and now keeps its notification settings inside the app. Operators do not normally create or edit `station.json` or `settings.json` files.

## Current event scope

- **Problem:** sent after a committed task result enters `fail` or a blocking `warning`.
- **Complete:** sent only after the Rust print-readiness gate confirms that the print report exists, the Transformer SN is saved, and every task is passed or explicitly accepted.
- Successful receipts are stored in the unit's `unit_state.json` to reduce duplicate cards. A Problem receipt is cleared after that task passes; Complete is sent once per unit state.
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
| Station | Selects Test Station 1–4 and supplies the station identity shown on cards. Confirm the correct station on each PC. |
| Destination name | Defaults to `PDU Testing` and labels the configured destination in the UI/status. It does not route the message; the Workflow URL determines the actual Teams chat. |
| Teams webhook URL | Masked in the UI. Paste a non-empty URL to set or replace it. Leaving the edit blank preserves an already stored URL; disable notifications if delivery should stop. |
| Notifications enabled | Master switch for automatic cards and test delivery. Save the enabled state before testing. |
| Shared OneDrive folder | Optional. Use **Browse** to pick the same shared folder on every station (for example a hidden folder you create under the `svc-pdu` OneDrive). On Save the app creates `shift_log.json` plus `stations/test-station-1` … `test-station-4` inside that folder. Leave empty to disable multi-station rollups. |
| Change password | Enter the current password, a new non-empty password, and matching confirmation. The old password stops unlocking Settings after the change is saved. |
| Save | Writes the form to the app-owned settings store. The notification worker uses the saved values for subsequent events. |
| Test ping | Uses the saved, enabled configuration and posts a blue connection card through the same Workflow. Its asynchronous result is correlated to the test event, so a concurrent Problem or Complete result is not mistaken for the ping. |

## Settings persistence

The app stores one JSON file under Tauri's per-user `app_config_dir`:

```text
<Tauri app_config_dir>\notification_settings.json
```

For the current application identifier, the Windows location is normally equivalent to:

```text
%APPDATA%\com.te.lab.pdu-data-automation\notification_settings.json
```

Use the path returned by Tauri as authoritative; Windows/Tauri may resolve the base directory differently by environment. The app does not fall back to `%TEMP%` if that directory is unavailable. A missing store loads factory defaults: password `0601`, Test Station 1, destination `PDU Testing`, notifications enabled, Problem and Complete enabled, an empty webhook, and an empty shared shift-log path. **Save** creates or updates the file. On Windows, an interrupted replace recovers a validated synced temp file or prior backup rather than silently reverting a saved password/webhook to factory defaults.

The file is app-owned and should not be hand-edited as the normal operator workflow. Old external notification JSON and `PDU_NOTIFICATIONS_STATION_PATH` are not required for normal floor use.

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

1. Under the `svc-pdu` OneDrive (or another path every station can read/write), create a **hidden** folder, for example `.pdu-notifications`.
2. On each station: Settings → **Browse** → select that same folder → **Save**.
3. The app creates this layout inside the chosen folder:

```text
<shared-folder>/
  shift_log.json
  stations/
    test-station-1/
    test-station-2/
    test-station-3/
    test-station-4/
```

`shift_log.json` is the floor-wide event ledger. The per-station folders are reserved for future station-local shared state. After Teams accepts a Problem or Complete event, the app appends to `shift_log.json` with short coordination/atomic replacement so multiple stations can contribute. OneDrive sync lag, permissions, or network failures can still occur. A log failure is status-only: it must not change the accepted Teams result or automation outcome.

Legacy settings that stored a direct `*.json` file path still work; the app treats a `.json` path as the log file and creates `stations/*` beside it.

The ledger does not yet post an end-of-shift card. That remains a follow-up after the shared folder and operating procedure are confirmed.

## Deferred events

- **Stuck:** deferred until the app owns a meaningful-progress clock. The expected STEP71 system burn-in runs for roughly two hours, so a flat 30-minute inactivity timer would generate false alerts.
- **Summary:** automatic/manual combined shift posting is deferred until the shared path and shift-counter workflow are confirmed.
- Two-way Teams commands remain out of scope.

The stored `idle_timeout_minutes`, Stuck toggle, Summary toggle, and shared-log data do not mean Stuck or Summary posting is active in this release.

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
9. Temporarily use an invalid webhook and confirm CSV processing and workbook writes still finish normally while the UI reports delivery failure.
10. Leave the shared OneDrive folder blank and confirm there are no log errors. After you create a hidden shared folder, Browse to it on each station, Save, and confirm `shift_log.json` plus `stations/test-station-*` appear under that folder.
11. Confirm the app-owned settings survive an application update.

No live webhook, phone-notification, shared-path, or installer validation is claimed by this document.
