# Configuration Model — Report Layouts

The goal is to move Excel report mappings, CSV source rules, templates, and verification thresholds out of source code and into versioned, editable JSON files.

## Location and Files

Production configuration lives under:

```
config/report-layouts/
├── pdu500.rev02.layout.json          ← active production profile
├── pdu500.accuracy-thresholds.json   ← verification thresholds (hot-reloadable)
├── pdu500.layout.example.json        ← reference example
└── README.md
```

A JSON Schema is at:

```
shared/schemas/report-layout.schema.json
```

Validation runs via:
- `bun run validate:report-layouts`
- `bun run validate`
- Backend unit tests
- CI (uses `bun` directly)

## Profile Structure (Key Concepts)

A layout profile defines:

- `schema_version`, `profile_id`, `display_name`
- `templates`: default template root + main/print report template filenames (with `##` placeholder for SN)
- `workbooks`: discovery patterns for main and print reports
- `serial_number`: folder regex + metadata filenames to help extract SN
- `task_groups`: ordered groups for the operator panel (208V, 415V, burn-in, etc.)
- `tasks`: each task has
  - `id`, `label`, `step`
  - `csv_pattern`
  - Either:
    - `"processor": "transformer" | "system" | "breaker" | ...` (built-in Rust logic still used)
    - or `"mappings": [...]` array for data-driven writes (preferred)

Each mapping specifies:
- `source`: column, row rule (`first_data_after_header`, `last_numeric`, etc.), `required`
- `transform`: optional `scale_by`, `round`
- `target`: `workbook` (main/print), `sheet`, `cell`, `number_format`

Accuracy thresholds live in a separate file (or can be embedded later) and are used by both mapped and built-in paths.

## Current State (v0.2.10)

- 208V and 415V Transformer tasks are fully data-driven via mappings in `pdu500.rev02.layout.json`.
- System, breaker, and burn-in tasks still declare `"processor"` and fall back to Rust code in `backend/src/automation/processors.rs` (and `mapped.rs` for the generic path).
- A generic mapped processor exists and is ready for expansion.
- Accuracy thresholds are external and reloaded per processing step.

## Editing Workflow

1. Copy the active profile to a new revision (e.g. `pdu500.rev03.layout.json`).
2. Update only the changed mappings, patterns, or thresholds.
3. Run schema validation + fixture tests.
4. For mapped tasks, run representative unit data and inspect the written cells.
5. Update the default in code/config only after validation.
6. To test a profile without changing the default, set the environment variable `PDU_LAYOUT_PROFILE_PATH=/path/to/profile.json`.

Never edit production reports directly — always work against safe copies during development.

## Runtime Loading Order

The app resolves layout configuration in this order:

1. Explicit environment override (`PDU_LAYOUT_PROFILE_PATH` for the layout profile, `PDU_ACCURACY_THRESHOLDS_PATH` for thresholds).
2. Bundled Tauri resources registered from the app resource directory. Current release output stores these under `_up_/config/report-layouts/`.
3. Development/source-tree paths such as `config/report-layouts/` and `../config/report-layouts/`.
4. External pilot path `C:/PDU500/config/report-layouts/`.
5. Compile-time built-in defaults via `include_str!` as the last safety fallback.

Use the environment override when testing an edited profile or threshold file without rebuilding the app.

## Why JSON

- Strict, diff-friendly, no extra parser dependency.
- Easy to validate with JSON Schema from both Rust and scripts.
- If comments are later required, the project can adopt JSONC or another format.

See `config/report-layouts/pdu500.rev02.layout.json` and `pdu500.accuracy-thresholds.json` for the live definitions.
See `backend/src/config/profile.rs` and `shared/schemas/report-layout.schema.json` for the implementation contract.
