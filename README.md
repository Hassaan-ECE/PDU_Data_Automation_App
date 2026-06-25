# PDU Data Automation App

Released pilot build for the rebuilt PDU data automation desktop app.

This repository is the pilot replacement for the current working legacy script bundle at:

```text
C:\Projects\Active\PDU_Data_Automation
```

The new app will live here:

```text
C:\Projects\Active\PDU_Data_Automation_App
```

## Status

`v0.2.9` is the current released pilot build. `v0.1.0` remains the first released pilot.

Current release:

- tag: `v0.2.9`
- GitHub release: `https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases/tag/v0.2.9`
- S-drive installer: `S:\Engineering\Public\Syed_Hassaan_Shah\PDU_Data_Automation\PDU Data Automation_0.2.9_x64-setup.exe`

Implemented in the pilot:

- Tauri 2/Rust backend and React/TypeScript/Vite/Tailwind frontend
- Bun package workflow (`bun run desktop`, etc.)
- operator-panel workflow with unit folder selection, timer/status area, expandable breaker groups, rerun controls, and report opening
- active production layout profile at `config/report-layouts/pdu500.rev02.layout.json`
- unit-folder setup, template copying/renaming, report discovery, CSV detection/parsing, and Excel workbook patching
- built-in Rust processors for transformer, 208V/415V system, 208V/415V breaker, system burn-in, and breaker burn-in tasks
- faster unit-folder detection by scanning CSVs before report setup completes
- delayed startup updater checks, unused font cleanup, and lightweight startup timing logs
- updater plugin permissions for future in-app updates
- three-step reset flow: collapse tests, reset current SN, then clear SN selection
- inline unit selection and Transformer SN setup/save flow that writes Transformer SN to `Test Summary!D1`
- manual Print Report flow that saves the final operator name to `Test Report #2!E39` and opens Excel's print UI for operator confirmation
- explicit current-step follow controls and readiness-based updater scheduling
- legacy-style CSV readiness waiting for active ATS-written files
- mid-test countdown resume from detected CSV start time
- total countdown uses current active-step time plus future unpassed step time
- transformer report source columns and target cells now run from the production layout profile
- signed NSIS installer, updater signature, `latest.json`, and release checksum publication

A known-good unit folder has been run through the installed app successfully, and the generated Excel workbook opened without repair prompts. Keep the legacy app available until several production units have been processed cleanly.

GitHub repository:

```text
https://github.com/Hassaan-ECE/PDU_Data_Automation_App.git
```

## Goal

Rebuild the working PDU automation panel in a cleaner, maintainable desktop app while preserving the operator workflow and layout.

The replacement should:

- keep the current test-panel workflow and section layout
- keep support for unit-folder selection and report opening
- detect instrument CSV files by STEP number
- wait for CSV completion before processing
- fill the same Excel report workbooks
- make report layouts easier to edit through config files
- ship as a single current-user Windows installer
- support GitHub Release based updates
- keep the current installer available on the S-drive
- live in source control from the beginning

## Target Stack

Use the same general stack as `TE_Component_Inventory`:

- Tauri 2 desktop shell
- React 19 frontend
- TypeScript
- Vite
- Tailwind CSS v4
- Bun package workflow
- Rust backend commands
- Tauri NSIS current-user installer
- signed Tauri updater with GitHub Releases metadata

The backend patches the Excel Open XML package directly. Any workbook/template change should be revalidated against safe copies to confirm formatting, formulas, merged cells, print settings, and existing sheets remain intact.

## Project Layout

```text
backend/                 Tauri/Rust backend source, report engine, file scanning, native commands
config/report-layouts/   Versioned data-driven Excel/CSV mapping files
docs/                    Current documentation (overview, architecture, legacy constraints, config model)
old-docs/                Historical plans, detailed audit notes, and superseded documents (archive)
fixtures/                Synthetic CSV/workbook fixtures for tests
frontend/                React/Vite UI source
release/                 Local staging notes only; generated release files stay untracked
scripts/                 Validation, release helpers, and checks (use `bun` directly)
```

## Documentation

Current authoritative documentation lives in `docs/`:

- `docs/README.md` — Documentation index
- `docs/OVERVIEW.md` — Current status, pilot features, stack, and remaining work
- `docs/ARCHITECTURE.md` — Runtime shape, responsibilities, data flow, Excel approach
- `docs/LEGACY_BEHAVIOR.md` — Constraints to preserve and behaviors to correct (read this)
- `docs/CONFIGURATION_MODEL.md` — How to work with report layout profiles and mappings
- `docs/PROJECT_STRUCTURE.md` — Repository layout and ownership
- `docs/RELEASE_AND_DEPLOYMENT.md` — Release, signing, S-drive, and GitHub practice
- `docs/decisions/` — Key architectural decisions

Historical and superseded detailed planning material remains in `old-docs/` for reference. Prefer the documents under `docs/`.

## Remaining Work Before Broad Cutover

- Run several more real or copied production unit folders, including known-good, known-fail, borderline, missing CSV, missing template, and workbook-open-in-Excel cases.
- Compare generated reports against legacy output cell-by-cell for representative units.
- Test the real updater upgrade path from this patched build to a newer release.
- Add scrubbed fixture coverage for representative CSV/report cases so regressions are caught without private production data.
- Continue hardening CSV readiness diagnostics for unusual unreadable-file cases.
- Keep the legacy Python app as fallback during the initial pilot.
- Continue moving system, breaker, and burn-in processor cell logic into the production layout profile over time.

## Completed Implementation Milestones

1. Create the Tauri/React/Bun/Rust skeleton.
2. Build a UI shell that visually matches the legacy panel.
3. Define typed test/task models and state transitions.
4. Promote the active production layout profile to `pdu500.rev02.layout.json`.
5. Implement Excel workbook patching against copied report templates.
6. Implement CSV detection/parsing and report writes for the current PDU500 Rev02 workflow.
7. Add signed installer and GitHub Release updater artifacts.
8. Stage the current installer on the S-drive.

## Current Development Commands

```powershell
bun install
bun run desktop          # start full Tauri desktop dev (frontend + backend)
bun run dev:frontend     # frontend dev server only
bun run build
bun run test
bun run lint
```

Backend checks:

```powershell
cargo test --manifest-path backend\Cargo.toml
cargo fmt --manifest-path backend\Cargo.toml --check
```

## Legacy Source Reference

Use these legacy references deliberately:

```text
C:\Projects\Active\PDU_Data_Automation\README.md
C:\Projects\Active\PDU_Data_Automation\docs\PROJECT_MAP.md
C:\Projects\Active\PDU_Data_Automation\docs\RUNBOOK.md
C:\Projects\Active\PDU_Data_Automation\docs\KNOWN_ISSUES.md
C:\Projects\Active\Data Automation Upgraded
```

`Data Automation Upgraded` should be treated as the better behavior reference for 208V/415V system and breaker processing because those scripts add Python-side accuracy calculations and pass/fail verification. The transformer and burn-in scripts match the older legacy folder.
