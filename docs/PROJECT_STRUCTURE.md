# Project Structure

The repository is intentionally split by responsibility.

```text
PDU_Data_Automation_App/
  backend/
  config/
    report-layouts/
  docs/
    decisions/
  fixtures/
  frontend/
  release/
  scripts/
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

Expected future contents:

- sample unit folder structures
- small CSV files for each test type
- safe workbook templates
- expected output snapshots where useful

Do not store confidential production data here.

## `scripts/`

Build, smoke-test, and release helper scripts.

Current script:

- Bun runner helper

Possible future scripts:

- release staging script
- version consistency check
- fixture smoke script
- S-drive publishing helper

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
