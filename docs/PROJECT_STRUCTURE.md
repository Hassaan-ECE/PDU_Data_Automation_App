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

Planned Tauri/Rust backend.

Expected future responsibilities:

- Tauri command handlers
- unit-folder scanning
- CSV parsing
- report-layout config loading
- Excel report writing
- native file dialogs and open-report behavior
- updater/release integration

## `frontend/`

Planned React/Vite frontend.

Expected future responsibilities:

- operator test panel
- status and timer display
- task state rendering
- settings screens
- log/error display
- updater status display

## `config/report-layouts/`

Versioned report layout profiles.

These files should describe the Excel/CSV mapping that was hardcoded in the legacy scripts.

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

Likely scripts:

- Bun runner helper
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
