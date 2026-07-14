# Shared floor settings — follow-ups and review log

**App:** PDU Data Automation  
**Shared folder (operators):** OneDrive `.PDU_Notifications` — Browse per PC; **never hard-code** the absolute path.

---

## Status (2026-07-13, after second-review fixes)

| Area | Status |
|---|---|
| Original adoption overwrite / whole-form save / auto-reseed | **Fixed** |
| Scoped saves, catalog cache, Settings clean poll, validation | **Fixed** |
| Dedicated Connect floor-password field (floor ≠ local Advanced) | **Fixed** |
| First-seed includes Advanced form fields on Connect | **Fixed** |
| Lock covers full read–patch–write for policy updates + seed | **Fixed** (`update_floor_settings_with_lock`) |
| Two-PC OneDrive smoke | **Not done** |
| Commit / release past 0.2.13 | After smoke |

**Code side of second review is addressed; still need two-PC smoke before release.**

---

## Confirmed floor folder

```text
…\OneDrive - Delta Electronics, Inc\.PDU_Notifications
```

Each PC Browses its own synced copy. Before rollout: OneDrive fully synced; one designated seeder; backup `shift_log.json` / `floor_settings.json` if present; 45s poll starts after OneDrive has delivered the file.

---

## Implemented (original review findings — done)

These were the first review’s critical items; code now has them:

- Adopt-only **Connect** for an existing floor (no full-form overwrite).
- Save scopes: **Operator**, **Identity**, **Teams**, **Connect**, **Local**; **Advanced** remains compatible for older callers.
- Fresh floor read before scoped policy patches.
- Seed only on explicit Connect when floor file is missing (no ordinary-load reseed).
- Full local `station_catalog` cache.
- Clean-form Settings UI poll every ~45s; dirty forms not clobbered.
- Floor password check on Connect (backend).
- Stronger name/shift validation; RFC 3339 `updated_at`.
- Unique temp filenames + floor write lock (around final write).
- Tests: adoption, scoped saves, missing floor, password, renames, UI poll.

---

## Second-review gaps (addressed in code)

### 1. Dedicated existing-floor password — done

- Station & Identities shows **Existing floor password** when the shared path is new/changed (pending Connect).
- `resolveSaveScope` prefers that field over Advanced unlock password.
- WrongFloorPassword keeps the field visible and shows a clear error for retry.

### 2. First-seed includes Advanced form — done

Connect when floor is missing applies Advanced + Operator request fields into the seed under lock.

### 3. Read–patch–write lock — done

`update_floor_settings_with_lock` locks → load → mutator → write. Used by Operator/Advanced policy saves and Connect seed. Adopt of an already-present floor still avoids rewriting policy.

### 4. Hygiene — done (or re-check on validate)

- Mock save no longer destructures unused `scope` / `connect_password`.
- `default_stations_catalog` is `#[cfg(test)]`.

### 5. Tooling note

`bun run validate` may segfault under Bun 1.3.14; run ESLint / vitest / cargo pieces directly.

---

## Recommended next sequence (remaining)

1. Full validation (notifications tests + settings Vitest + ESLint).  
2. Commit implementation.  
3. Two local AppData roots vs one shared directory.  
4. Two real PCs vs local OneDrive `.PDU_Notifications`.  
5. Smoke green → bump past 0.2.13 → installer/updater release.

---

## Implementation map

| Area | Path |
|---|---|
| Floor I/O + lock | `backend/src/notifications/floor_settings.rs` |
| Scopes / connect / cache | `backend/src/notifications/app_settings.rs` |
| Commands | `backend/src/commands.rs` |
| Backend poll | `backend/src/notifications/worker.rs` |
| Settings UI | `frontend/src/features/settings/NotificationSettingsPage.tsx` |
| Types / `resolveSaveScope` | `frontend/src/features/settings/settingsTypes.ts` |
| Bridge mock | `frontend/src/integrations/tauri/backend.ts` |
| Operator docs | `docs/NOTIFICATIONS.md` |

---

## Product decisions (still valid)

- Delivery poll ~45s; UI poll only when Settings open and clean.  
- Shared password lives in floor file.  
- Local-only until shared folder set.  
- Rename display names only (fixed 4 station ids).  
- Operators: Main/shifts open; rename Advanced.  
- Same app as admin via shared folder.  
- Transport: one `floor_settings.json`.

---

## Historical: original honest status (pre-scoped-saves)

*Kept for context; most rows are fixed now.*

| Claim | Then |
|---|---|
| Browse+Save adopts floor | Unsafe whole-form overwrite |
| 45s Settings refresh | Backend only; open UI no re-fetch |
| Missing floor safe | Auto-reseed could wipe renames |
| Offline catalog | Only this PC’s name cached |
