# Operator notification catalog

What Teams can notify you about **today** while using PDU Data Automation on the test stations.

Destination: the operators **PDU Testing** Teams group chat (and phone push if Teams mobile notifications are on for that chat).

---

## Quick map

| Notification | Color / card | Automatic? | When it fires | Once per… |
|--------------|--------------|------------|---------------|-----------|
| **Test ping** | 🔵 Blue | Manual only | You press **Send test ping** in Settings | Each press |
| **Problem** | 🔴 Red | Automatic | A test step finishes as **fail** or **warning** | That step until it passes (then can alert again) |
| **Changeover** | 🟡 Yellow | Automatic | STEP41 (`208V Breaker 8 – 20% Load`) passes | That unit (one time) |
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
- After a step is **processed and committed** and the result is **`fail`** or a blocking **`warning`**
  - Fail: real failure (bad reading, verification fail, etc.)  
  - A CSV that is still timing, locked, or waiting for the required STEP72 burn-in capture remains **waiting** and does not send a false Problem card
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

## 3. Changeover (automatic) — “perform the STEP42 tap change”

**Title example:** `🟡 Changeover · Test Station 1`

**Body:**

| Field | Text |
|-------|------|
| **UNIT** | Unit serial |
| **ACTION** | `208V testing complete — shut down the PDU and change transformer taps for 415V` |
| **NEXT STEP** | `STEP43 · 415V Transformer Check` |

**When it fires**

- Only after `208v-breaker-8-20% Load` (STEP41) is processed and commits a **pass**
- STEP42 is the operator's physical shutdown/tap-change action; STEP43 is the next automated test
- Only if Notifications and the Advanced **Changeover card** toggle are enabled

**Spam and safety behavior**

- One accepted Changeover card per unit; rerunning STEP41 does not send another
- A Teams failure stores no receipt, so a later committed STEP41 pass can retry
- Delivery failure never changes the task result and never blocks moving to STEP43

## 4. Complete (automatic) — “unit is ready for print / sign-off”

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

## 5. Not active yet (do not expect these cards)

### Stuck (idle)
- **Not automatic**
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

### D. Changeover (safe)

1. Use a fixture/copy without an existing Changeover receipt
2. Process `208V Breaker 8 – 20% Load` to a committed pass
3. Confirm one yellow card with the STEP42 action and STEP43 next step
4. Rerun STEP41 and confirm no duplicate card

### E. Shared folder (optional)
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
| STEP41 / 208V Breaker 8 – 20% Load passes | **Yes — Changeover** (once) |
| Unit becomes fully print-ready | **Yes — Complete** (once) |
| Want to check chat still works | **Test ping** from Settings |
| Stuck / idle / end of shift rollup | Not yet |

---

## Settings access (two levels)

| Open (no password) | Password-locked (Advanced) |
|--------------------|----------------------------|
| **Shift & summary options** — windows, who posts, which Floor Stations appear on the card, enable/disable summary | **Station & Identities** — searchable this-PC identity, shared folder, add/rename identities |
|  | **Teams & Notifications** — destination, webhook, enable/disable notifications, Changeover toggle, test ping |
| **End of shift** — preview / post (if this PC is the poster) | **Change password** (shared across PCs when the shared folder is set) |

If the usual poster PC is down, any operator can open **Shift & summary options** and reassign the poster station (no password). With a shared folder configured, that Main assignment syncs to other PCs within about 45 seconds.

## Identity notes (current floor)

- The original stable ids remain: `test-station-1`, `test-station-3`, `test-station-4`, `pdu-lab`
- Advanced → **Manage identities** can rename an existing identity or add a new **Floor Station** / **Admin Identity**; generated stable ids do not change when names change
- A Floor Station may be Main and appear in summary controls. An Admin Identity may identify a desk PC but never appears in Main/summary choices
- **Upgrade every PC that uses the shared floor before adding the first new identity.** That first add upgrades `floor_settings.json` to schema 2
- Lab PC can keep the **pdu-lab** identity; end-of-shift poster defaults to it
- Phones only ring if that person is in **PDU Testing** and has Teams notifications enabled for the chat
- Admin can point any copy of the app at the same shared folder and change floor settings from a desk PC

---

## Related docs

- Setup / settings detail: [NOTIFICATIONS.md](./NOTIFICATIONS.md)  
- Latest released baseline notes: [../release/v0.2.14.md](../release/v0.2.14.md)
