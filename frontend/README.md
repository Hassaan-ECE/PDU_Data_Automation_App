# Frontend

Initial React/Vite frontend for the rebuilt PDU operator panel.

Planned areas:

- `app` - root app shell and styling
- `features/test-panel` - main operator workflow shell and typed task model
- `features/settings` - template path, layout profile, and release settings
- `integrations/tauri` - typed Tauri bridge for backend commands and native dialogs
- `shared/components` - reusable buttons, sections, dialogs, and status widgets
- `shared/lib` - formatting, state helpers, and frontend validation

Current state:

- renders folder selection, serial number, timer, state counts, section panels, expandable breaker groups, manual rerun controls, and open-report controls
- uses mock task states for layout parity only
- does not process CSV files or write Excel reports yet

Current frontend work should keep STEP71/STEP72 clear in the UI:

- STEP71 is the long system burn-in/soak period.
- STEP72 is the quick system burn-in data capture used by the report writer.
- The panel can present this as one operator workflow, but labels/tooltips should avoid implying that STEP71 is the report data source.
