# Architecture

This document describes the target architecture for the rebuilt PDU Data Automation App.

## Design Principles

- Preserve the working operator workflow.
- Move fragile test/report mappings out of source code.
- Keep file and Excel processing in backend code where it can be tested without the UI.
- Make every data-processing result explicit: success, missing data, parse failure, write failure, or user-cancelled.
- Keep release and update behavior aligned with the `TE_Component_Inventory` app.

## Target Runtime Shape

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
- display logs and actionable errors
- expose settings for template path, shared release root, and active layout profile when needed

The frontend should not contain Excel cell maps, CSV column maps, or report-writing logic.

## Backend Responsibilities

The Rust backend should own:

- reading app settings
- resolving template and unit-folder paths
- scanning unit folders for STEP CSV files
- checking if files are stable/readable
- parsing CSV files
- validating required columns and rows
- loading report-layout config
- writing Excel report workbooks
- writing processing logs
- emitting task progress events to the frontend
- opening reports or folders through native integration
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
7. Backend writes mapped cells into one or more report workbooks.
8. Backend saves the workbook or reports a locked-file error.
9. Backend returns a structured `ProcessingResult`.
10. Frontend updates the task state and displays summary/log details.

## Excel Writer Spike

This is the main technical risk.

The legacy app uses `openpyxl` to modify existing `.xlsx` files. The Rust replacement must prove that its chosen approach can:

- open the current main and print report templates
- preserve sheet names
- preserve formatting
- preserve formulas
- preserve merged cells
- preserve print settings
- write numeric and text values into target cells
- save a workbook Excel can open without repair prompts

Candidate approaches:

- Rust-only workbook editing with a crate that can read and write existing `.xlsx` files
- a small dedicated sidecar only if Rust cannot safely preserve templates
- Excel COM automation only as a last resort because it depends on installed Excel and is harder to test

The preferred direction is Rust-only if the spike passes.

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
- locked workbook behavior where practical

Frontend tests should cover:

- test-panel rendering
- task state transitions
- backlog/detected prompt behavior
- manual rerun behavior
- error and warning display
- updater status display

Release tests should cover:

- version consistency
- installer build
- updater artifact generation
- S-drive staging layout
