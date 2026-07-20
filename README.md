# PDU Data Automation

**Windows desktop app for PDU test-floor operators** — watch instrument CSVs, track every step, and write Excel test reports without fighting spreadsheets by hand.

[![Release](https://img.shields.io/github/v/release/Hassaan-ECE/PDU_Data_Automation_App?label=release&color=0ea5e9)](https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases)
[![Stack](https://img.shields.io/badge/stack-Tauri%202%20%7C%20React%20%7C%20Rust-111827)](https://github.com/Hassaan-ECE/PDU_Data_Automation_App)
[![Platform](https://img.shields.io/badge/platform-Windows-0078D4?logo=windows&logoColor=white)](https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases)

## Screenshots

<table>
  <tr>
    <td align="center" width="50%" valign="top">
      <img src="docs/images/pdu-app-screenshot.png" alt="PDU Data Automation operator panel — timer, unit serial, 415V breaker progress, and resume controls" width="360" /><br />
      <sub><b>Operator panel</b> — countdown, unit / transformer SN, color-coded breakers, Open / Print Report (v0.2.15)</sub>
    </td>
    <td align="center" width="50%" valign="top">
      <img src="docs/images/teams-notifications.png" alt="Microsoft Teams channel with Complete and Changeover Adaptive Cards from PDU stations" width="360" /><br />
      <sub><b>Teams on the floor</b> — Complete and Changeover cards from multiple test stations in the shared channel</sub>
    </td>
  </tr>
</table>

---

## What it does

PDU Data Automation is the pilot replacement for the legacy Python test-panel scripts. Operators pick a unit folder; the app:

1. **Detects** STEP-numbered CSV files from the test instruments  
2. **Waits** until files are ready (no half-written scrapes)  
3. **Validates** readings against accuracy thresholds before writing  
4. **Patches** the Excel test report (Open XML — formatting and formulas stay intact)  
5. **Tracks** progress with a large timer, section status, and expandable breaker groups  
6. **Notifies** the floor over Microsoft Teams (Complete, Problem, Changeover) so stations stay in sync without chasing people down the aisle  

It ships as a **current-user Windows installer** with **signed in-app updates** via GitHub Releases.

**Current pilot release:** [v0.2.15](https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases/tag/v0.2.15)

---

## Highlights

| Area | Capability |
|------|------------|
| **Operator workflow** | Familiar panel layout: unit folder, timer/status, 208V / 415V sections, burn-in, expandable breakers, manual rerun |
| **CSV pipeline** | STEP-based detection, readiness waiting, strict parsing (missing values never become silent zeros) |
| **Excel reports** | Template copy + transactional patch; transformer mappings driven by config under `config/report-layouts/` |
| **Print Report** | Final operator name capture, then Excel’s native print UI |
| **Teams notifications** | Adaptive cards for **Complete**, **Problem**, and **Changeover** (e.g. 208V done → shut down and retap for 415V), station identity, shared floor settings |
| **Updates** | Signed Tauri updater + NSIS installer; floor PCs can pull newer pilots in-app |

---

## Stack

| Layer | Tech |
|-------|------|
| Desktop shell | [Tauri 2](https://tauri.app/) |
| UI | React 19, TypeScript, Vite, Tailwind CSS |
| Backend | Rust (CSV, Open XML zip patching, file scan, notifications) |
| Tooling | Bun |
| Install / update | NSIS current-user installer · signed GitHub Releases updater |

---

## For operators

1. Install the latest setup EXE from [Releases](https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases) (or your site’s S-drive package if you use that channel).  
2. Browse to the unit’s data folder.  
3. Confirm / save Transformer SN, then **Start** or **Resume**.  
4. Let the panel follow the active step; use **Open Report** / **Print Report** when the unit is complete.  
5. For multi-PC Teams / floor identity settings, point every station at the **same** shared `.PDU_Notifications` folder (do not hard-code different paths per machine).

> **Note:** Keep the legacy automation app available until your team has processed several production units and compared reports. Pilot does not mean full cutover.

---

## For developers

```powershell
bun install
bun run desktop          # Tauri desktop (frontend + Rust backend)
bun run dev:frontend     # UI only
bun run build
bun run test
bun run lint
bun run validate         # full local check before a release
```

Backend:

```powershell
cargo test --manifest-path backend\Cargo.toml
cargo fmt --manifest-path backend\Cargo.toml --check
```

### Repository layout

```text
backend/                 Tauri + Rust (scan, CSV, reports, notifications)
frontend/                React operator UI
config/report-layouts/   Data-driven Excel / CSV mappings
docs/                    Architecture, legacy notes, release process
fixtures/                Synthetic CSV / workbook test data
release/                 Per-version release notes
scripts/                 Validation and release helpers
```

### Documentation

| Doc | Purpose |
|-----|---------|
| [docs/OVERVIEW.md](docs/OVERVIEW.md) | Status, features, remaining work |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Runtime shape and data flow |
| [docs/LEGACY_BEHAVIOR.md](docs/LEGACY_BEHAVIOR.md) | Behaviors to preserve or correct |
| [docs/CONFIGURATION_MODEL.md](docs/CONFIGURATION_MODEL.md) | Report layout profiles |
| [docs/RELEASE_AND_DEPLOYMENT.md](docs/RELEASE_AND_DEPLOYMENT.md) | Signing, GitHub, S-drive practice |
| [docs/NOTIFICATIONS.md](docs/NOTIFICATIONS.md) | Teams / floor notification setup |
| [release/](release/) | Version-by-version release notes |

---

## Design principles

- **Preserve the floor workflow** unless a change is explicitly documented.  
- **Config over hardcoding** for report cell maps (`config/report-layouts/`).  
- **Fail honestly** — bad or missing CSV data must not look like a valid zero.  
- **Excel fidelity** — generated workbooks must open without repair prompts.  
- **No secrets in git** — updater private keys and installers stay out of source control.

---

## Project status

This is a **production pilot**. Core workflow, report writing, installer, and updater path are in use on the floor. Full cutover waits on more side-by-side report checks against the legacy pipeline and broader station rollout.

| | |
|--|--|
| **Latest** | [v0.2.15](https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases/tag/v0.2.15) |
| **Repo** | https://github.com/Hassaan-ECE/PDU_Data_Automation_App |
| **Author** | Syed Hassaan Shah |

---

## License / internal use

Internal engineering tool for TE lab / production test use. Contact the maintainer for distribution outside that context.
