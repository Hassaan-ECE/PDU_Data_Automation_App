# Operator notifications (Teams) — design

**Date:** 2026-07-08  
**Status:** Approved for implementation planning  
**Context:** Pilot PDU Data Automation is in use on four test stations. Operators want phone alerts for problems, completion, idle/stuck conditions, and shift handoff summaries without staying at the panel.

---

## Problem

Tests are mostly automated. Operators leave the station and need to know when:

- Something goes wrong (fail, blocked, locked workbook, etc.)
- A unit is finished / ready for print
- Progress appears stuck
- Shift handoff / end-of-day summary would help (they already do similar manual closeout)

Alerts must identify **which** of the four stations is involved and must not spam people outside the operator group.

---

## Goals

1. Deliver short, actionable notifications to operators’ **phones** via **Microsoft Teams**.
2. Use a **shared group for all four stations** (anyone free can respond).
3. Label every message with **Test Station N** (e.g. Test Station 1 … Test Station 4).
4. Prove delivery on real floor PCs **without** risking the live PDU Data Automation pilot.
5. Share config via **`svc-pdu`** OneDrive where practical.
6. Keep notification failures soft: never block CSV processing or Excel writes (main app later).

## Non-goals (v1)

- Two-way Teams chat (e.g. `/station 1 update` that replies) — **future only** (needs a bot or equivalent).
- SMS, email, or third-party push as primary channel.
- Using OneDrive as the real-time alert bus (sync lag).
- Separate always-on Windows service for v1.
- Automatic alerts on app close, PC sleep, or unit-folder switch.
- Hard-coded fixed shift times as the only summary mechanism.

---

## Constraints and environment

| Fact | Implication |
|------|-------------|
| Four stations, same Windows user **`svc-pdu`** | Shared config and permissions are practical |
| OneDrive used for shared material under that account | Good home for `settings.json` (and later optional status files) |
| Normal desktop internet | Direct HTTPS to Teams Workflow/webhook is fine |
| Operators already use Teams | Prefer Teams over a new phone app |
| Broader team can see some computer channels | Use a **new operators-only Teams group/team**, not the wide channel |
| Live pilot must stay stable | Ship a **separate pilot notifier app** first |

---

## Approach (selected)

**A — Teams webhook + shared OneDrive config**, with a **standalone pilot app** first, then later integration into PDU Data Automation using the same message builder and config.

Rejected for v1:

- **B** — OneDrive event folder + helper (extra delay and moving parts).
- **C** — ntfy / other push as primary (ignores existing Teams usage).

---

## Architecture

### Phase 1 — Pilot notifier app (this first ship)

```
PDU Notifier (small desktop tool)
    ├── buttons: test / problem / complete / stuck / summary / reload config
    ├── read OneDrive settings.json (+ optional local station override)
    └── HTTPS POST → Teams Workflow webhook
              ↓
    Operators-only Teams group  →  phone notifications
```

No CSV, Excel, unit-folder, or automation logic in the pilot.

### Phase 2 — Main app integration (after pilot is proven)

```
PDU Data Automation (existing)
    ├── existing task / print / scan flow detects real events
    ├── same notify module + same settings schema
    └── POST → same Teams webhook
```

Notification failure must never fail the automation path.

### Shared pieces (avoid building twice)

- Config schema (`settings.json`)
- Message formatting (station, event type, unit SN, detail, timestamp)
- HTTP POST to Teams webhook
- Soft error reporting

**Repository placement:** the pilot is a **separate project folder** under `C:\Projects\Active\` (not nested inside `PDU_Data_Automation_App`). Suggested path:

```text
C:\Projects\Active\PDU_Notifier
```

(Name can be adjusted at create time; keep it short and operator-safe.)

Phase 2 integration into `PDU_Data_Automation_App` will either:

- copy/port the proven notify module into the Tauri backend, or  
- depend on a tiny shared crate if both repos later share one  

Do **not** require a monorepo or submodule for phase 1. The pilot must build, run, and ship on its own.

---

## Teams destination

- Create a **new Teams group/team** for operators + `svc-pdu` only (user will create on the operator environment).
- Attach a **Workflow / incoming webhook** to a channel in that group.
- Store the webhook URL only in OneDrive config (or local override)—**never commit** to git.
- Phone notifications rely on each operator’s normal Teams mobile notification settings for that team/channel.

---

## Pilot app UX

Minimal window:

| Control | Behavior |
|---------|----------|
| Station label | Shows configured name, e.g. `Test Station 1` |
| **Test ping** | Sends health-check message |
| **Simulate problem** | Fake fail / blocked style message |
| **Simulate complete** | Fake unit finished / print-ready |
| **Simulate stuck** | Fake idle timeout message |
| **Post summary** | Fake shift summary body |
| **Reload config** | Re-read OneDrive / local settings |
| Status line | Last send result: success or clear error |

Sample unit SN and task text may be fixed placeholders in the pilot.

Optional stretch (not required for first cut): fields to override station name or config path for testing.

---

## Events (main app, later)

| Event | When | Notes |
|-------|------|--------|
| **Problem** | Task fails; process error; setup blocked; workbook locked; hard print-gate failure | Prefer once per task until state leaves fail (avoid spam) |
| **Complete** | Unit print-ready (all tasks pass/accepted + transformer SN present), or equivalent “all done” | Once per unit session until new unit / reset |
| **Stuck / idle** | Active context, no meaningful progress for **N minutes** (config, default 30) | Cooldown so stuck is not re-spammed every minute |
| **Summary** | Manual button; optional **configurable** schedule times | Schedules vary by week (1-shift vs 2-shift)—times are settings, not hard-coded product rules |
| **Test ping** | Manual only | Pilot + optional main-app settings |

### Noise control

- Per-event cooldowns where appropriate (especially stuck).
- Master `enabled` flag and per-event toggles in config.
- Missing webhook or `enabled: false` → no send; UI/log explains why; automation continues.

### Explicitly not automatic in v1 main integration

- App exit / crash  
- PC sleep  
- Unit folder switched (may add later if operators want it)

---

## Message format

Plain text (v1). Adaptive Cards are optional later.

**Test ping**
```text
[Test Station 1] Notification test — OK
```

**Problem**
```text
[Test Station 2] PROBLEM
Unit: 262343000072
208V Breaker 3 — 100% Load failed
<short detail>
```

**Complete**
```text
[Test Station 1] COMPLETE
Unit: 262343000072
Ready for print / operator name
```

**Stuck**
```text
[Test Station 3] STUCK
Unit: 262343000072
No progress for 30 min (waiting on STEP15 …)
```

**Summary**
```text
[Test Station 1] SHIFT SUMMARY
Since last summary / session:
- Units finished: 2
- Problems: 1
- Currently: idle / running unit …
```

Always include: station name, event kind, timestamp (in body or Teams metadata), unit SN when known.

---

## Configuration

### Shared OneDrive (svc-pdu)

Illustrative path (exact path chosen at implementation / deploy time):

```text
<OneDrive>/PDU_Data_Automation/notifications/settings.json
```

Example shape:

```json
{
  "enabled": true,
  "teams_webhook_url": "https://...",
  "stations": {
    "test-station-1": {
      "station_name": "Test Station 1",
      "idle_timeout_minutes": 30,
      "events": {
        "problem": true,
        "complete": true,
        "stuck": true,
        "summary": true
      },
      "summary_schedule_times": ["15:00", "23:00"]
    }
  }
}
```

Notes:

- `summary_schedule_times` is **optional**. Empty or omitted ⇒ manual summary only.
- Operators’ weeks change (one shift vs two); times are editable in config without a code release.
- Webhook URL is a secret; treat like other deployment secrets (OneDrive ACLs under svc-pdu, not source control).

### Per-PC identity

Each PC must know it is Test Station 1 vs 2 vs 3 vs 4. Options (pick one clear rule at implement time):

1. Tiny local file beside the exe, e.g. `station.json` → `{ "station_id": "test-station-1" }`, or  
2. Environment variable / installer-time setting.

Shared file holds the display name and feature flags; local file only selects which station entry applies.

### Config load order

1. Local station id override  
2. Shared OneDrive `settings.json`  
3. If webhook missing or disabled → sends fail soft with a clear message  

**Reload config** in the pilot re-reads without restart.

---

## Failure handling

| Situation | Behavior |
|-----------|----------|
| No network / Teams down | Surface error; do not crash |
| Invalid webhook | Clear config/webhook error |
| OneDrive path missing | Local override if present; else explain and disable send |
| Main app notify fails (phase 2) | Log only; **never** block scan/process/Excel |

---

## Future (out of scope for pilot and first main-app hook)

### Two-way Teams commands

Example desire: `/station 1 update` and a reply with current status.

Requires a Teams bot (or Power Automate message trigger) plus a place that holds live status (e.g. OneDrive status files written by each station). Document only; do not implement in v1.

### Possible later event ideas

- Print-ready but not printed after N minutes  
- True multi-station rollup summary (one message aggregating all four)  
- Daily quiet summary beyond per-station lines  

---

## Testing / validation plan

### Pilot app on operator PC (`svc-pdu`)

1. Create operators-only Teams group; add operators + `svc-pdu`.  
2. Create webhook/Workflow; put URL in OneDrive `settings.json`.  
3. Set local station id to `Test Station N` for that PC.  
4. Click each button; confirm messages appear only in the new group.  
5. Confirm phones notify for that team/channel.  
6. Turn off network / break webhook; confirm soft error UI.  
7. Repeat identity check on all four stations (correct station name in messages).

### Main app (phase 2)

1. Fixture or safe unit folder: induce fail, complete, and stuck paths.  
2. Confirm messages match real task labels/SNs.  
3. Confirm process still succeeds when webhook is intentionally broken.  
4. Confirm cooldowns and once-per-unit complete behavior.

---

## Repository / packaging (implementation guidance)

### Pilot project (phase 1)

```text
C:\Projects\Active\PDU_Notifier\     # separate project (own git repo recommended)
  src/                               # UI + config load + webhook POST
  station.example.json               # local station id template (no secrets)
  settings.example.json              # shared config shape (no real webhook)
  README.md                          # setup: Teams group, OneDrive path, station id
```

- **Own folder and ideally own git repo** under `C:\Projects\Active\`.  
- Design spec for the feature also lives in `PDU_Data_Automation_App` (`docs/superpowers/specs/…`) so the main product history retains the decision.  
- Pilot can ship as a folder on S-drive or OneDrive (portable exe + local station file).  
- Do not put webhook URLs or secrets in git.  
- Do **not** couple phase 1 builds to `bun run desktop` / the main installer.

### Main app (phase 2)

Wire the same config schema and message rules into `PDU_Data_Automation_App` after the pilot is proven on all four stations.

---

## Phased delivery

| Phase | Deliverable | Success criteria |
|-------|-------------|------------------|
| **1** | Separate project `C:\Projects\Active\PDU_Notifier` (button app) + OneDrive config + operators Teams group | All four stations can send test/sim messages; phones receive them |
| **2** | Shared notify module wired into PDU Data Automation | Real fail / complete / stuck / summary; automation unaffected by notify failures |
| **3+** | Optional: schedule polish, rollup, two-way bot | Only if operators still want it after phase 2 |

---

## Open decisions (resolve during implementation plan)

1. Exact OneDrive path under `svc-pdu`.  
2. Local station id mechanism (file vs env).  
3. Teams Workflow template details (Power Automate “Post to channel when webhook received” vs legacy connector).  
4. Whether pilot is Tauri/React mini-app, native dialog, or simplest Rust+egui/web UI—prefer **fastest reliable Windows desktop**.  
5. Whether phase 1 implements optional summary **schedule** or only manual summary (manual is enough for pilot buttons).  
6. Final folder/repo name if not `PDU_Notifier` (e.g. `PDU_Station_Notifier`).

---

## Summary

Ship a **tiny button-driven notifier** as a **separate project** at `C:\Projects\Active\PDU_Notifier` (not inside the main app repo). It posts to a **new operators-only Teams group** using a webhook, with **Test Station N** identity and **shared OneDrive config** under `svc-pdu`. Prove it on the floor, then integrate the same path into `PDU_Data_Automation_App` for problem, complete, stuck, and summary events—without two-way chat until a later phase.
