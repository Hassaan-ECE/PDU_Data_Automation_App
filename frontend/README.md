# Frontend

React/Vite frontend for the rebuilt PDU operator panel.

Source areas:

- `app` - root app shell and styling
- `features/test-panel` - main operator workflow shell and typed task model
- `integrations/tauri` - typed Tauri bridge for backend commands and native dialogs
- `shared/lib` - formatting, state helpers, and frontend validation

Future areas, if needed:

- `features/settings` - template path, layout profile, and release settings
- `shared/components` - reusable buttons, sections, dialogs, and status widgets

Current state:

- renders folder selection, serial number, timer, state counts, section panels, expandable breaker groups, manual rerun controls, update status, and open-report controls
- calls the Tauri backend for production layout loading, unit folder setup, CSV scanning, task processing, and report opening
- keeps a browser-only mock fallback for frontend development outside Tauri
- displays processing, pass, warning, fail, and error-card states returned by the backend
- uses the active PDU500 Rev02 task model and production layout profile

Remaining frontend work:

- keep validating the operator flow on real production-machine screen sizes
- smoke-test failure, missing CSV, workbook-open, and updater states during pilot rollout
- add targeted UI regression tests once workflows stabilize

Current frontend work should keep STEP71/STEP72 clear in the UI:

- STEP71 is the long system burn-in/soak period.
- STEP72 is the quick system burn-in data capture used by the report writer.
- The panel can present this as one operator workflow, but labels/tooltips should avoid implying that STEP71 is the report data source.
