# Operator notification catalog (v0.2.12)

What Teams can notify you about **today** while using PDU Data Automation on the test stations.

Destination: the operators **PDU Testing** Teams group chat (and phone push if Teams mobile notifications are on for that chat).

---

## Quick map

| Notification | Color / card | Automatic? | When it fires | Once per… |
|--------------|--------------|------------|---------------|-----------|
| **Test ping** | 🔵 Blue | Manual only | You press **Send test ping** in Settings | Each press |
| **Problem** | 🔴 Red | Automatic | A test step finishes as **fail** or **warning** | That step until it passes (then can alert again) |
| **Complete** | 🟢 Green | Automatic | Unit is **print-ready** (all steps done + Transformer SN saved) | That unit (one time) |
| **Stuck** | 🟠 Orange | **Not active** | — | Deferred (burn-in risk) |
| **End of shift summary** | 📊 Blue-ish | **Manual only** | Designated station posts from Settings → End of shift | After successful post, shared log clears |

---

## 1. Test ping (manual)

**Title example:** `🔵 Connection confirmed · Test Station 1`

**Body (typical):**
- Short note that notification delivery is working  
- Timestamp  

**How to send**
1. Cog (top right) → password (default `0601` unless changed)  
2. Confirm station + webhook are saved  
3. **Send test ping**  
4. Check **PDU Testing** in Teams  

**Use it for:** setup, after PC restart, or when you wonder if Teams is still working.

---

## 2. Problem (automatic) — “something needs attention”

**Title example:** `🔴 Problem · Test Station 3`

**Body (typical fields):**
| Field | Example |
|-------|---------|
| **UNIT** | Unit serial (e.g. `262343000072`) when known |
| **ISSUE** | Task name (e.g. `208V Breaker 3 · 100% Load`) |
| **DETAIL** | Failure message from the app (CSV / accuracy / process error text) |
| **CURRENT STEP** | e.g. `STEP24` |
| Timestamp | Local time |

**When it fires**
- After a step is **processed and committed** and the result is **`fail`** or **`warning`**  
  - Fail: real failure (bad reading, verification fail, etc.)  
  - Warning: includes cases like missing / not-ready CSV for that step  
- Only if **Notifications enabled** and the Problem toggle is on (defaults on)

**Spam control**
- Same failing step does **not** send a new card every rerun until that step later **passes**  
- After it passes, a later fail can send Problem again  

**What it is not**
- Not sent for: picking a folder, scan-only, layout config errors, blank Transformer SN write errors, print dialog failures (those stay in the app UI for now)

**What operators should do**
1. Note which **station** and **unit** are on the card  
2. Go to that station if free  
3. Read **ISSUE** / **DETAIL** / **CURRENT STEP**  
4. Fix (CSV, open workbook lock, re-run step, etc.) in the app  

---

## 3. Complete (automatic) — “unit is ready for print / sign-off”

**Title example:** `🟢 Complete · Test Station 1`

**Body (typical fields):**
| Field | Example |
|-------|---------|
| **UNIT** | Unit serial when known |
| **STATUS** | `Ready for print and operator sign-off` |
| Timestamp | Local time |

**When it fires**
Only when the app’s **print-readiness** check is fully green:
- Print report workbook is present  
- Transformer SN is saved  
- Every task is **pass** or explicitly **accepted**  

It can be checked after a successful pass (or after saving Transformer SN when that tips the unit into ready).

**Spam control**
- **One Complete card per unit** for that unit’s state (rerunning completed work should not spam greens)

**What operators should do**
1. Note **station** and **unit**  
2. Go print / sign off when ready (same as today’s print flow in the app)

---

## 4. Not active yet (do not expect these cards)

### Stuck (idle)
- **Not automatic in v0.2.12**  
- Planned later so we don’t false-alarm during long **system burn-in** (~2 hours)

### End-of-shift summary (manual, designated station)
- **Not automatic** — no timer fire in this build  
- **Who posts:** only the station chosen under **Settings → Shift timing → Station allowed to post end of shift** (default **PDU Lab**)  
- **Where:** Settings → **End of shift** → optional operator name / shift label → **Refresh preview** → **Post end of shift**  
- **Needs:** shared OneDrive folder on all stations + Problem/Complete activity in `shift_log.json`  
- After a successful post the shared log is cleared so the next shift starts clean  

### Stuck (idle)
- Still deferred

---

## How to practice / test on a station

### A. Connection
1. Settings → **Test ping**  
2. Confirm blue card in **PDU Testing**

### B. Problem (safe)
1. Use a **past unit folder** or a fixture folder with a known bad/missing step CSV  
2. Process that step until the app shows **fail** or **warning**  
3. Confirm **one** red Problem card (station + unit + step make sense)  
4. Process again while still failing → should **not** spam another card for the same step  
5. Fix/pass the step, then fail again → a **new** Problem is allowed  

### C. Complete (safe)
1. Use a unit that can reach real **print-ready** (all steps pass/accepted + Transformer SN saved)  
2. Confirm **one** green Complete card  
3. Repeat processing should **not** send another Complete for that unit  

### D. Shared folder (optional)
1. Same OneDrive shared folder selected on Stations 1, 3, 4  
2. After Problem/Complete, check that folder for activity under `shift_log.json` / `stations/…`  
3. This does **not** post a summary card by itself yet  

---

## Checklist for operators during a normal run

| Situation | Expect Teams card? |
|-----------|--------------------|
| App opens / folder selected | No |
| Step fails or warns after processing | **Yes — Problem** |
| Same step fails again without having passed | No (deduped) |
| Unit becomes fully print-ready | **Yes — Complete** (once) |
| Want to check chat still works | **Test ping** from Settings |
| Stuck / idle / end of shift rollup | Not yet |

---

## Settings access (two levels)

| Open (no password) | Password-locked (Advanced) |
|--------------------|----------------------------|
| **Shift & summary options** — windows, who posts, which stations appear on the card, enable/disable summary | **Station & Teams** — this PC identity, webhook, shared folder, test ping |
| **End of shift** — preview / post (if this PC is the poster) | **Change password** |

If the usual poster PC is down, any operator can open **Shift & summary options** and reassign the poster station (no password).

## Station notes (current floor)

- **Stations in Settings:** Test Station 1, 3, 4 and **PDU Lab** (Station 2 removed)  
- Lab PC selects **PDU Lab**; end-of-shift poster defaults to **PDU Lab**  
- Phones only ring if that person is in **PDU Testing** and has Teams notifications enabled for the chat  

---

## Related docs

- Setup / settings detail: [NOTIFICATIONS.md](./NOTIFICATIONS.md)  
- Release notes: [../release/v0.2.12.md](../release/v0.2.12.md)  
