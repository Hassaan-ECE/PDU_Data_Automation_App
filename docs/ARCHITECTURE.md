# Architecture

This document describes the current architecture and remaining target direction for the rebuilt PDU Data Automation App.

## Design Principles

- Preserve the working operator workflow.
- Move fragile test/report mappings out of source code.
- Keep file and Excel processing in backend code where it can be tested without the UI.
- Make every data-processing result explicit: success, missing data, parse failure, write failure, or user-cancelled.
- Keep release and update behavior aligned with the `TE_Component_Inventory` app.

## Runtime Shape

```text
React frontend
  |
  | Tauri invoke commands/events
  v
Rust backend
  |
  |-- unit folder scanner
  |-- CSV discovery and parsing
  |-- report-layout config loader
  |-- report writer
  |-- logs and diagnostics
  |-- release/update integration
  v
Unit folder
  |
  |-- instrument CSV files
  |-- main report workbook
  |-- print report workbook
  |-- processing log
```

## Frontend Responsibilities

The frontend should own operator interaction only:

- render the test panel
- show timers, status, and progress
- show detected/running/pass/fail states
- allow folder selection
- allow start, pause, resume, reset, manual rerun, and open report actions
- collect and save setup metadata such as Transformer SN
- keep current-step follow behavior explicit and operator-controlled
- prompt for final operator name and print confirmation after a full pass
- prompt before switching to a newly detected ATS-created unit/SN
- display logs and actionable errors
- expose settings for template path, shared release root, and active layout profile when needed
- keep the burn-in UI as one operator workflow even if backend timing tracks the long burn-in and short data-capture periods separately

The frontend should not contain Excel cell maps, CSV column maps, or report-writing logic.

## Backend Responsibilities

The Rust backend should own:

- reading app settings
- resolving template and unit-folder paths
- scanning unit folders for STEP CSV files
- suggesting the latest likely ATS-created unit folder/SN for Start-time setup
- checking if files are stable/readable
- parsing CSV files
- validating required columns and rows
- computing accuracy and pass/fail checks from configured thresholds
- loading report-layout config
- applying data-driven report mappings when a task defines them
- writing Excel report workbooks
- writing setup and completion metadata, including Transformer SN and final operator name
- writing processing logs
- emitting task progress events to the frontend
- opening reports, folders, and print dialogs through native integration
- returning enough workbook context for the UI to open or explain failure locations
- supporting updater and release metadata

## Core Domain Concepts

| Concept | Meaning |
| --- | --- |
| `TaskDefinition` | A logical test item such as `208V System - 100% Load` or `415V Breaker 2 - 50% Load` |
| `StepDefinition` | The instrument STEP number and CSV pattern associated with a task |
| `ReportLayoutProfile` | Versioned config describing report templates, sheets, target cells, and source CSV columns |
| `TaskState` | UI/process state: off, detected, waiting, processing, pass, warning, fail |
| `ProcessingResult` | Backend result with written cells, skipped cells, warnings, errors, and source files |
| `UnitSession` | A selected unit folder plus inferred serial number, reports, discovered files, and current task state |
| `UnitSetupMetadata` | Operator-supplied setup values such as Transformer SN |
| `CompletionAction` | End-of-test action such as final operator-name write and print confirmation |
| `NewUnitCandidate` | Newly detected ATS unit/SN/folder offered to the operator before switching sessions |
| `VerificationRule` | Configured pass/fail rule for computed accuracy values |

## Backend Command Contract

Current setup-related commands:

| Command | Purpose |
| --- | --- |
| `find_latest_unit_candidate` | Returns the newest likely ATS-created unit folder/SN candidate, or `candidate: null` when none is found. |
| `setup_unit_folder_with_transformer_sn` | Runs existing setup behavior for a selected unit folder and writes Transformer SN to `Test Summary!D1`. |
| `save_transformer_sn` | Writes a later Transformer SN edit to the selected unit's main report workbook at `Test Summary!D1`. |

`find_latest_unit_candidate` should return candidate data useful for future detection flows: serial number, display label, full folder path, detection source/reason, and timestamp. The current inline UI does not auto-suggest or auto-select the latest candidate at startup.

`setup_unit_folder_with_transformer_sn` accepts the selected unit folder, optional unit serial number, and required Transformer SN. The Transformer SN is written as text, not as a number.

`save_transformer_sn` accepts the operator-confirmed unit folder and required Transformer SN. It does not auto-detect or switch folders. It writes the value as text so numeric-looking values such as `000123` are preserved.

Known structured setup/save error codes include `unit_folder_missing`, `blank_transformer_sn`, `workbook_locked`, `main_report_missing`, `report_sheet_missing`, `report_cell_invalid`, and `report_write_failed`.

## State Model

The replacement should use a clearer state model than the legacy app:

| State | Meaning |
| --- | --- |
| `off` | No source file detected and not processed |
| `detected` | Matching source file exists but has not been processed |
| `waiting` | The app is waiting for a source file to appear or stabilize |
| `processing` | Backend is parsing CSV and writing report cells |
| `pass` | Required data was written successfully |
| `warning` | Processing completed with non-blocking warnings |
| `fail` | Required data was missing, invalid, or could not be written |

Do not reuse the legacy behavior where missing data can become pass.

## Data Flow For A Test Step

1. Frontend asks backend to start or process a task.
2. Backend resolves the task from the active layout profile.
3. Backend finds the latest matching CSV source file.
4. Backend verifies the file is stable/readable.
5. Backend parses required source columns and rows.
6. Backend validates missing, nonnumeric, or out-of-range values.
7. Backend computes configured accuracy/verification values.
8. Backend writes mapped cells into one or more report workbooks, or falls back to the task's built-in Rust processor when that logic has not been moved into config yet.
9. Backend saves the workbook or reports a locked-file error.
10. Backend returns a structured `ProcessingResult`.
11. Frontend updates the task state and displays summary/log details.

Current `v0.2.9` status:

- The published `v0.2.9` release uses inline unit selection and Transformer SN entry/save instead of the setup modal.
- The frontend includes a manual Print Report flow that captures the final operator name before opening Excel's print UI.
- The generic data-driven mapping processor path exists.
- The 208V and 415V transformer report writes are driven by mappings in `config/report-layouts/pdu500.rev02.layout.json`.
- System, breaker, and burn-in tasks still use built-in Rust processors as the fallback path.

## Planned Operator Workflow Additions

These additions should stay aligned with the existing operator workflow:

- Inline setup: unit selection is done through the `...` browse button, the visible unit field shows only the selected unit SN, and Transformer SN is entered inline. Start saves the Transformer SN through `setup_unit_folder_with_transformer_sn`; later edits save through `save_transformer_sn` on blur, Enter, or the icon-only save button.
- Report opening should block while Transformer SN is missing or unsaved.
- Current-step follow: `Start`, `Resume`, `Follow Step`, and `Current Step` enable follow mode and scroll to the active task. Manual wheel/touch scroll and expand/collapse disable follow mode.
- Updater timing: the first updater check should wait for backend status and layout profile startup requests to settle, then run after a short post-ready delay. Interval, focus, and visibility checks remain.
- Error navigation: when a task fails, expose the best available workbook/sheet/cell context. If exact Excel navigation is unreliable, open the workbook and show a clear location summary in the app.
- Completion prompt: after the full test passes, collect or select the final operator name, write it to `Test Report #2!E39` in the print report workbook, save, then open the print dialog for confirmation.
- New unit detection: watch for a newly started ATS unit/SN only after the active session is idle, complete, or explicitly dismissed. The app should prompt before setup or switching sessions.
- Burn-in timing: keep the UI as a single burn-in flow while allowing the timer to transition from the long burn-in wait to the short STEP72 capture wait.

## Workbook Patching

The legacy app uses `openpyxl` to modify existing `.xlsx` files. The Rust backend now patches the workbook Open XML package directly for report writes.

This remains an area to validate whenever templates or report-writing logic change. The implementation must continue to:

- open the current main and print report templates
- preserve sheet names
- preserve formatting
- preserve formulas
- preserve merged cells
- preserve print settings
- write numeric and text values into target cells
- save a workbook Excel can open without repair prompts

Current coverage:

- unit tests cover patched-cell style preservation, inserted rows/cells, shared formula expansion, calc-chain removal, and workbook recalculation flags
- the installed `v0.1.0` app processed one known-good unit and produced a workbook that opened in Excel without repair prompts
- the `v0.2.6` release was smoke-tested with `C:\PDU500\262343000072`, and the generated data was manually reviewed as good

Remaining coverage:

- broaden validation across more real or copied units
- manually review generated reports against expected or legacy values when needed
- test workbook-open-in-Excel and failure/error paths

## Configuration Boundary

All of these should live in versioned config, not scattered source constants:

- report template names
- report filename patterns
- sheet names
- STEP numbers
- CSV filename patterns
- CSV source columns
- source row selection rules
- scaling rules
- computed accuracy rules and pass/fail thresholds
- Excel target cells
- required vs optional fields
- numeric formatting rules

See `CONFIGURATION_MODEL.md`.

## Testing Strategy

Backend tests should cover:

- STEP-to-task mapping
- CSV discovery
- CSV parsing and required-field validation
- report-layout config validation
- cell-write planning without touching real files
- workbook writes against fixture copies
- setup metadata and completion metadata writes against fixture copies
- locked workbook behavior where practical

Frontend tests should cover:

- test-panel rendering
- task state transitions
- backlog/detected prompt behavior
- inline setup metadata behavior, including folder browsing, no startup latest-unit auto-selection, Transformer SN validation, successful setup, later save, report-open blocking, and setup/save error display
- current-step follow behavior, including auto-scroll while following and disabling follow mode on manual scroll or expand/collapse
- readiness-based updater timing before the first updater check
- new-unit detection prompt behavior
- end-of-test operator-name and print-confirmation flow
- manual rerun behavior
- error and warning display
- updater status display

Release tests should cover:

- version consistency
- installer build
- updater artifact generation
- S-drive staging layout
