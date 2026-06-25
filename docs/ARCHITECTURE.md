# Architecture

## Design Principles

- Preserve the operator workflow and visual panel exactly unless a change is explicitly documented.
- Move fragile mappings (cells, columns, steps) into versioned config under `config/report-layouts/`.
- Keep all file/CSV/Excel processing in the Rust backend (testable without the UI).
- Make every processing result explicit (success / missing / parse error / verification fail / locked / etc.).
- Never silently turn bad or missing data into pass or zero.
- Excel writes must preserve template formatting, formulas, merged cells, and open without repair prompts.

## Runtime Shape

```
React frontend (operator panel)
        ↓ Tauri commands + events
Rust backend (Tauri crate)
        ├── unit folder scanning + report discovery
        ├── CSV discovery, stability wait, parsing
        ├── layout profile loading + validation
        ├── accuracy verification
        ├── Excel OpenXML patching (zip direct)
        ├── unit_state.json persistence
        └── native dialogs / print / open report
        ↓
Unit folder on disk
        ├── STEP*.csv files from ATS
        ├── main report workbook (SN renamed)
        ├── print report workbook
        └── unit_state.json (sidecar)
```

## Responsibilities

### Frontend (React + TypeScript)
- Render the test panel, timers, status, sections, and color states.
- Folder selection, inline Transformer SN entry + save.
- Current-step follow controls + auto-scroll.
- Backlog prompt, print readiness checks, operator name capture.
- Manual rerun, reset (3-stage), report opening.
- Updater UI.

The frontend owns **no** CSV column knowledge, cell addresses, or report writing logic.

### Backend (Rust)
- All scanning, parsing, verification, and writing.
- Loading and validating the active `ReportLayoutProfile`.
- Dual dispatch: mapped path (config) vs built-in processor path.
- Transactional workbook patching with lock files + backups.
- `unit_state.json` for restart resilience and idempotency (fingerprints).
- Error codes that the UI can surface clearly (`workbook_locked`, `blank_transformer_sn`, print blockers, etc.).

## Core Domain Concepts

- `AutomationTask` / `TaskKind` (65 tasks matching the legacy panel)
- `ReportLayoutProfile` + mappings or processor tag
- `ProcessorResult` / `TaskProcessResult`
- `UnitState` (persisted per unit folder)
- CSV fingerprint for idempotency
- `PrintReadinessResult` (gate before final operator name + print)

## Data Flow (Normal Processing)

1. Operator selects unit folder + enters Transformer SN → setup writes `Test Summary!D1`.
2. During run: scan → detect stable CSV → process.
3. For a task:
   - Find latest matching CSV by pattern.
   - Wait for stable size + mtime.
   - Parse required columns/rows (strict).
   - Compute values + accuracy (thresholds from config).
   - If mapped: build cell updates from profile.
   - If processor path: use built-in logic.
   - **Verify accuracy first**.
   - Only on success: patch workbook(s) transactionally.
   - Record result + fingerprint + timestamp in `unit_state.json`.
4. On full pass + readiness: prompt for operator name → write to print report → open Excel print dialog.

## Excel Patching Approach

The backend edits `.xlsx` files by direct manipulation of the OpenXML zip package (no Excel automation, no openpyxl equivalent in the hot path).

Safety measures:
- Per-workbook lock file.
- `.bak` backup before write.
- Transactional multi-workbook updates with rollback on any failure.
- Remove calc chain + force recalc on save.
- Use `inlineStr` for text values (Transformer SN, operator name) to avoid scientific notation / number conversion.
- Tests assert style preservation, merged cells, formulas, shared formula expansion, and that workbooks open cleanly in Excel.

Any change to `reports.rs`, `patch_workbook`, or the templates must be re-validated.

## State Model

Task states surfaced to the UI:
- `off`, `detected`, `waiting`, `processing`, `pass`, `warning`, `fail`

Additional persisted concepts:
- `accepted` (operator override of a failure)
- CSV fingerprint + processed_at for restart + rerun safety.

## Configuration & Extensibility

- Layout profile drives as much as possible.
- Accuracy thresholds are hot-reloadable.
- Future product revisions should be new profile files (`pdu500.rev03.layout.json`) rather than code changes.

See:
- `config/report-layouts/pdu500.rev02.layout.json`
- `docs/CONFIGURATION_MODEL.md`
- `backend/src/automation/`, `backend/src/config/`
- Tests in `backend/tests/` for fidelity guarantees.
