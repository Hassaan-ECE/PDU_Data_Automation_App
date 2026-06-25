# Project Structure

The repository is intentionally split by responsibility.

For planned structure cleanup and migration guardrails, see
[`STRUCTURE_CLEANUP_PLAN.md`](STRUCTURE_CLEANUP_PLAN.md).

```text
PDU_Data_Automation_App/
  .github/
    workflows/
  backend/
  config/
    report-layouts/
  docs/
    decisions/
  fixtures/
  frontend/
  release/
  scripts/
  shared/
```

## `backend/`

Tauri/Rust backend.

Current responsibilities:

- Tauri command handlers
- unit-folder scanning
- CSV parsing
- report-layout config loading
- Excel report writing
- native file dialogs and open-report behavior
- updater/release integration

`backend/src/commands.rs` is the thin Tauri command boundary. Domain behavior stays under
`backend/src/automation/` and config/profile loading stays under `backend/src/config/`.

## `frontend/`

React/Vite frontend.

Current responsibilities:

- operator test panel
- status and timer display
- task state rendering
- log/error display
- updater status display

Possible future responsibilities:

- settings screens for template path, active layout profile, and release/update controls

## `config/report-layouts/`

Versioned report layout profiles.

These files describe the production task/profile shape. The current profile uses `processor` fields for tasks whose cell/source logic still lives in Rust processors.

## `fixtures/`

Synthetic or sanitized test data.

Current and expected contents:

- sample unit folder structures
- small CSV files for each test type
- safe workbook templates
- expected output snapshots where useful

Do not store confidential production data here.

## `scripts/`

Build, smoke-test, and release helper scripts.

Current scripts:

- Bun runner helper
- version consistency check
- fixture smoke script
- full local validation runner

Possible future scripts:

- release staging script
- S-drive publishing helper

## `shared/`

Cross-cutting contracts that have an explicit source-of-truth rule.

Current contents:

- JSON Schema for report-layout config validation

Do not add duplicated handwritten Rust/TypeScript IPC types here without a generation or schema
strategy.

## `.github/workflows/`

Source validation CI.

Current workflow:

- installs Bun and Rust on Windows
- runs the local validation script
- does not build signed installers, publish releases, touch S-drive paths, or require updater keys

## `release/`

Release staging notes only.

Generated installers and updater artifacts should stay ignored by Git and be copied to GitHub/S-drive during release.

## `docs/`

Durable project documentation:

- architecture
- migration plan
- release plan
- legacy behavior
- configuration model
- decision records
