# In-app notification settings — design

**Date:** 2026-07-10  
**Status:** Implemented; automated validation complete, live floor validation pending  
**App:** `PDU_Data_Automation_App`  
**Depends on:** existing Phase 2 notifications module (`backend/src/notifications/`)

---

## Problem

Operators and technicians should not manage separate `station.json` / `settings.json` files. Station identity, Teams webhook, and a light settings lock must live **inside the app**, editable from a gated Settings screen. Test ping must be available from that screen so connection to the PDU Testing chat can be verified without the standalone pilot app.

Multi-station end-of-shift totals still need a **shared shift log** somewhere on the network/OneDrive. Exact path will be confirmed later; the product must store an optional path in settings and only use the log when that path is set.

---

## Goals

1. Cog (top-right) → password modal → full Settings view (Back top-left).
2. Default password `0601`; password changeable from Settings (after unlock).
3. No operator-managed notification JSON beside the exe for normal use.
4. Settings editable in-app: station identity, webhook, enabled, optional shared shift-log path, test ping.
5. Existing Problem / Complete soft-fail delivery keeps working, reading config from the in-app store.
6. Password is a **deterrent for operators**, not cryptographic security.

## Non-goals (this slice)

- Two-way Teams bot.
- Hardening password (hashing, OS credential store) beyond plain stored value.
- Requiring the shared shift log path before ship (optional until path is confirmed).
- Stuck automation (still deferred for burn-in reasons).

---

## UX

### Main operator panel

- Keep existing layout.
- Add a **settings cog** button top-right (fixed to the main chrome, not scrolling with workflow steps).
- Footer notification status line remains.

### Password modal

- Same pattern as existing panel modals (`fixed inset-0 … bg-black/55`).
- Single password field + Cancel + Unlock.
- Wrong password → inline error; do not navigate.
- Correct password → close modal, show Settings view (replace main content).

### Settings view

- **Back** (top-left) returns to operator panel (session stays unlocked until app restart, or re-lock on Back — **re-lock on Back** so a walking-away operator is safer).
- Fields:
  - Station: dropdown Test Station 1–4 (`test-station-1` … `test-station-4`)
  - Destination name (default `PDU Testing`) — display only optional; can be editable text
  - Teams webhook URL (password-style input / masked; never shown in logs)
  - Notifications enabled (checkbox)
  - Shared shift log path (text; optional; empty = floor rollup disabled)
  - Change password: current (optional if already unlocked this session), new, confirm
  - **Test ping** button + last result text
  - **Save** button
- Unsaved edits: warn on Back if dirty (simple confirm).

---

## Persistence (in-app store)

Single file owned by the app (not hand-edited by operators):

```text
%AppData%\com.pdu.data-automation\notification_settings.json
```

(Exact folder = Tauri `app.path().app_config_dir()`; package identifier from `tauri.conf.json`.)

Shape:

```json
{
  "schema_version": 1,
  "settings_password": "0601",
  "enabled": true,
  "teams_destination_name": "PDU Testing",
  "teams_webhook_url": "",
  "station_id": "test-station-1",
  "station_name": "Test Station 1",
  "idle_timeout_minutes": 30,
  "events": {
    "problem": true,
    "complete": true,
    "stuck": false,
    "summary": false
  },
  "shared_shift_log_path": ""
}
```

- First launch / missing file → create with defaults (`settings_password` = `0601`, empty webhook, station 1).
- Webhook never written to automation logs; Debug/Display redacts it.
- Optional one-time **import**: if app store missing and old external `station.json` + settings still exist, import once then continue using app store only. Not required for v1 if time-boxed.

---

## Backend

- `load_config()` for the notification worker resolves from the **app store**, not `C:\PDU500\config\notifications\station.json` (keep env override only for automated tests if useful).
- New Tauri commands:
  - `get_app_notification_settings` → UI-safe DTO (webhook may be returned only to settings page after unlock; prefer returning masked flag + whether set)
  - `save_app_notification_settings`
  - `verify_settings_password`
  - `change_settings_password`
  - `send_notification_test` (already exists) — ensure it uses app-store config
- Soft-fail rules unchanged: notification failures never fail CSV/Excel.

---

## Shared shift log (path deferred)

| When path empty | When path set (user will confirm location) |
|-----------------|--------------------------------------------|
| No multi-station rollup | Append Problem/Complete (and Stuck later) after successful Teams accept |
| Settings can still save | End-of-shift (later button) reads file and posts combined card |

File is a single shared JSON (same idea as pilot `shift_log.json`). Writers must load-merge-save carefully (short file lock or atomic replace). **Do not block** if log write fails — status only.

Until the path is confirmed, implement store field + skip log I/O when empty; optional “Post shift summary” can be a later task once path is known.

---

## Security notes

- Password gate is intentional friction only.
- Changing password requires Settings access (already unlocked) + optional re-entry of current password.
- Do not commit real webhooks.
- Mask webhook in UI list views; full value only on settings form when unlocked.

---

## Success criteria

1. Fresh install: cog → `0601` → settings; set station + webhook → Save → Test ping appears in Teams.
2. Wrong password rejected; Back re-locks.
3. Password change to a new value; old password no longer unlocks after save.
4. Process a failing task → Problem card uses in-app station name.
5. No requirement to place `station.json` beside the exe for normal floor use.
6. Shared shift log path can be blank without errors; when set later, append does not break automation.
