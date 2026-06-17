# PDU Data Automation App

Planning scaffold for the rebuilt PDU data automation desktop app.

This repository will replace the current working legacy script bundle at:

```text
C:\Projects\Active\PDU_Data_Automation
```

The new app will live here:

```text
C:\Projects\Active\PDU_Data_Automation_App
```

## Status

This repository now has an initial runnable skeleton for the rebuild:

- Tauri 2/Rust backend project
- React/TypeScript/Vite/Tailwind frontend
- Bun package workflow through `scripts/run-bun.mjs`
- operator-panel UI shell with mock task states
- report-layout JSON parsing and validation tests

It is not a functional replacement for the legacy app yet. CSV processing, Excel report writing, installer signing, updater metadata, and production S-drive staging still need to be implemented and validated.

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

The exact Rust Excel writer must be validated early. The legacy app modifies existing Excel templates with `openpyxl`; the replacement must preserve workbook formatting, formulas, merged cells, and existing sheets.

## Proposed Project Layout

```text
backend/                 Tauri/Rust backend source, report engine, file scanning, native commands
config/report-layouts/   Versioned data-driven Excel/CSV mapping files
docs/                    Architecture, migration, release, and legacy behavior notes
fixtures/                Synthetic CSV/workbook fixtures for tests
frontend/                React/Vite UI source
release/                 Local staging notes only; generated release files stay untracked
scripts/                 Build, release, smoke-test, and helper scripts
```

## Documentation Index

- `docs/ARCHITECTURE.md` - target architecture and responsibilities
- `docs/MIGRATION_PLAN.md` - phased plan from legacy scripts to the new app
- `docs/LEGACY_BEHAVIOR.md` - behavior that must be preserved or intentionally corrected
- `docs/CONFIGURATION_MODEL.md` - data-driven report layout design
- `docs/RELEASE_AND_DEPLOYMENT.md` - GitHub, updater, S-drive, and installer plan
- `docs/decisions/0001-adopt-tauri-react-rust.md` - stack decision record

## Current Acceptance Criteria For The Rebuild

The rebuilt app is not done until it can:

- select a unit folder and infer or accept the unit serial number
- copy or locate the required report templates
- scan existing CSV files and mark matching tests as detected
- run or process the same logical test sequence as the legacy app
- handle 208V transformer, system, breaker, 415V transformer, system, breaker, system burn-in, and breaker burn-in data
- write values into the correct sheets and cells using versioned layout config
- distinguish missing data from numeric zero
- produce clear per-step logs
- keep the UI responsive while watching and processing files
- build a current-user NSIS installer
- publish/update through GitHub Release metadata
- stage the current installer and release support files on the S-drive

## First Implementation Milestones

1. Create the Tauri/React/Bun/Rust skeleton. Initial pass complete.
2. Build a UI shell that visually matches the legacy panel. Initial mock shell complete.
3. Define typed test/task models and state transitions. Initial frontend model complete.
4. Parse the example report-layout config. Initial Rust parser and validation tests complete.
5. Spike Excel template modification in Rust against real copies of the current reports.
6. Add CSV fixture tests for the known STEP files.
7. Implement report writes one processor group at a time.
8. Add installer and signed updater flow.

## Current Development Commands

Use the Bun helper so this repo does not depend on a broken global shim:

```powershell
node scripts/run-bun.mjs install
node scripts/run-bun.mjs run dev:frontend
node scripts/run-bun.mjs run build
node scripts/run-bun.mjs run test
node scripts/run-bun.mjs run lint
```

Backend checks:

```powershell
cd backend
cargo test
cargo fmt --check
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
