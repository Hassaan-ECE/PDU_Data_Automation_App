# Shared floor notification settings + editable station names

**Date:** 2026-07-13  
**Status:** Code fixes for adoption/scoped saves/no-reseed/catalog cache/UI poll landed (2026-07-13). Multi-PC OneDrive smoke still required before release.  
**App:** `PDU_Data_Automation_App`  
**Follow-ups / rollout:** [2026-07-13-shared-floor-settings-followups.md](./2026-07-13-shared-floor-settings-followups.md)

## Problem

Notification settings were per-PC AppData only. Changing Main poster, shifts, webhook, password, or station labels on one machine did not update others. Station display names were hard-coded, so renumbering required a release.

## Solution

When a shared notifications folder is configured:

- **Floor-wide** settings live in `<shared-folder>/floor_settings.json`.
- **Local-only** settings remain in AppData: this PC’s `station_id` and the shared-folder path pointer.
- Peers poll the floor file about every **45s** and reload when Settings opens.
- First PC seeds the floor file; later PCs adopt it.
- Same app binary acts as admin when pointed at the same shared folder.

## Access control

| Open (operators) | Advanced (password) |
|---|---|
| Shifts, Main poster, included stations, end-of-shift | This PC station, webhook, shared folder, **station display names**, password change |

Password is floor-shared when the shared folder is set.

## Stable station slots (v1)

Fixed ids: `test-station-1`, `test-station-3`, `test-station-4`, `pdu-lab`.  
Display names editable (max 64 chars). No add/remove in v1.

## Key files

- `backend/src/notifications/floor_settings.rs` — floor file I/O
- `backend/src/notifications/app_settings.rs` — merge load/save, seed/adopt
- `backend/src/notifications/worker.rs` — 45s poll
- `frontend/src/features/settings/*` — catalog-driven UI + Advanced rename fields
- `docs/NOTIFICATIONS.md` — operator-facing persistence notes

## Validation

- Automated: `cargo test --lib notifications::` and frontend settings tests.
- Live floor: two PCs (or two AppData roots) on one shared folder — rename, Main, password, webhook sync within one poll; local-only path still independent.
