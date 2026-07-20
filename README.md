<table>
<tr>
<td width="42%" valign="middle">

# PDU Data Automation

**Test-floor automation for PDU stations** — CSVs in, Excel reports out, Teams in the loop.

Windows desktop app for production test stations. Operators pick a unit folder; the app watches instrument CSVs, tracks every step on a familiar panel, validates readings, and writes the Excel test report without hand-editing spreadsheets.

Pilot replacement for the legacy Python automation scripts. Same workflow on the floor — cleaner stack underneath (Tauri 2, React, Rust), data-driven report mappings, and signed in-app updates.

- Detects STEP CSVs and waits until files are ready  
- Validates accuracy before any Excel write  
- Color-coded breakers, burn-in, Open / Print Report  
- Optional Teams Complete / Problem / Changeover cards  

**[Download v0.2.15 →](https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases/tag/v0.2.15)** · [All releases](https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases)

</td>
<td width="58%" valign="middle" align="center">

<img alt="PDU Data Automation operator panel" src="docs/images/pdu-app-screenshot.png" width="100%" />

</td>
</tr>
</table>

## Download

Get the current pilot installer from the [latest GitHub release](https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases/latest), or use the S-drive package your site already stages for operators.

| | |
| --- | --- |
| **Current release** | [v0.2.15](https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases/tag/v0.2.15) |
| **Platform** | Windows · current-user NSIS installer |
| **Updates** | Signed in-app updater (after the first install from the matching key era) |

The app can check for updates when the station is ready. Keep the legacy tool available until your team has run several production units side by side.

## Get started

1. Install the setup EXE and open **PDU Data Automation**.
2. Browse to the unit’s data folder.
3. Confirm / save the Transformer SN, then **Start** or **Resume**.
4. Let the panel follow the active step — green for pass, amber for in progress, expandable breakers for load steps.
5. When the unit is done, use **Open Report** or **Print Report** (final operator name, then Excel’s print UI).

For multi-PC Teams and floor identity settings, point every station at the **same** shared `.PDU_Notifications` folder. Do not hard-code different paths per machine.

## Teams on the floor

<table>
<tr>
<td width="42%" valign="middle">

Stations post Adaptive Cards into a shared Microsoft Teams channel so the floor sees **Complete**, **Problem**, and **Changeover** events without walking the aisle — for example when 208V work is finished and the unit needs to shut down and retap for 415V.

Setup is password-gated in Advanced Settings. Every PC on the floor should browse to the same shared `.PDU_Notifications` folder.

Details: [docs/NOTIFICATIONS.md](docs/NOTIFICATIONS.md)

</td>
<td width="58%" valign="middle" align="center">

<img alt="Teams Complete and Changeover cards from PDU test stations" src="docs/images/teams-notifications.png" width="100%" />

</td>
</tr>
</table>

## How it works

1. **Detect** STEP-numbered CSVs from the instruments  
2. **Wait** until files are stable and ready to read  
3. **Validate** against accuracy thresholds before any write  
4. **Patch** the Excel workbook (Open XML — formatting and formulas stay intact)  
5. **Track** remaining time, section status, and breaker progress on the panel  

Missing or bad values never become silent zeros. Report cell maps prefer config under `config/report-layouts/` over hardcoding in source.

## Develop

```powershell
bun install
bun run desktop          # full Tauri desktop app
bun run dev:frontend     # UI only
bun run test
bun run lint
bun run validate         # full local check before a release
```

```powershell
cargo test --manifest-path backend\Cargo.toml
```

| Path | Role |
| --- | --- |
| `backend/` | Tauri + Rust (scan, CSV, reports, notifications) |
| `frontend/` | React operator UI |
| `config/report-layouts/` | Excel / CSV mappings |
| `docs/` | Architecture, legacy notes, release process |
| `fixtures/` | Synthetic test data |
| `release/` | Per-version release notes |

## Learn more

- [Overview](docs/OVERVIEW.md) — status and remaining pilot work  
- [Architecture](docs/ARCHITECTURE.md) — runtime shape and data flow  
- [Legacy behavior](docs/LEGACY_BEHAVIOR.md) — what to preserve or correct  
- [Configuration model](docs/CONFIGURATION_MODEL.md) — report layout profiles  
- [Release & deployment](docs/RELEASE_AND_DEPLOYMENT.md) — signing, GitHub, S-drive  
- [Notifications](docs/NOTIFICATIONS.md) — Teams and floor settings  
- [Release notes](release/) — version-by-version history  

## Status

**Production pilot.** Core workflow, report writing, installer, and updater path are in use on the floor. Full cutover waits on more report comparisons against the legacy pipeline and broader station rollout.

Built by Syed Hassaan Shah.

## License

Internal engineering tool for TE lab / production test use. Contact the maintainer for distribution outside that context.
