# Backend

Initial Tauri/Rust backend for the rebuilt PDU app.

Planned modules:

- `api` - Tauri command handlers and event payloads
- `domain` - task, state, report-layout, and processing-result models
- `config` - report-layout profile loading and validation
- `scanner` - unit-folder and CSV discovery
- `csv` - CSV parsing and typed value extraction
- `excel` - workbook editing and save behavior
- `runtime` - settings, logging, updater, and release paths

Current state:

- Tauri command entry points are present
- `get_app_status` exposes the app/backend version baseline
- `load_example_layout_profile` parses and validates the example layout profile
- Rust unit tests cover the current layout validation rules
- CSV scanning and Excel writing are still pending
