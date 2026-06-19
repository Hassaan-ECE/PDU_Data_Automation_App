# Backend

Tauri/Rust backend for the rebuilt PDU app.

Implemented areas:

- Tauri command handlers and event payloads
- task, state, report-layout, and processing-result models
- production layout profile loading and validation
- unit-folder setup, report discovery, and CSV discovery
- CSV parsing and typed value extraction
- Excel workbook patching and report-opening helpers
- processor implementations for transformer, 208V/415V system, 208V/415V breaker, system burn-in, and breaker burn-in tasks
- updater/release configuration through Tauri

Current state:

- Tauri command entry points are present
- `get_app_status` exposes the app/backend version baseline
- `load_layout_profile` parses and validates the active production layout profile
- `setup_unit_folder` copies/renames report templates and returns detected task state
- `scan_unit_folder` detects existing STEP CSV files
- `process_automation_task` parses CSV data, patches the report workbook, and returns structured pass/warning/fail results
- `open_report_path` and `open_report_location` open generated reports from the selected unit folder
- Rust unit tests cover layout validation, accuracy thresholds, CSV helpers, workbook patching, and task mappings

Remaining hardening:

- add scrubbed fixture tests for more representative production CSV/report cases
- broaden side-by-side comparisons against legacy output
- distinguish still-writing, locked, unreadable, missing, and stable CSV files more explicitly
- keep validating workbook preservation whenever templates or report-writing logic change
