# Configuration Model

The rebuild should make report layout editing a data/config problem instead of a source-code change.

## Goals

- Let future Excel layout updates happen by editing a versioned config file.
- Keep source code generic: parse CSV, apply mappings, write cells.
- Validate config before a run starts.
- Make changes easy to diff in GitHub.
- Support multiple report/template versions if the workbook layout changes later.

## Config Location

Report layouts live under:

```text
config/report-layouts/
```

Active production profile:

```text
config/report-layouts/pdu500.rev02.layout.json
```

Reference examples should stay clearly marked as examples. Future production profiles should use explicit names, for example:

```text
pdu500.rev02.layout.json
pdu500.rev03.layout.json
```

## Layout Profile Responsibilities

A layout profile should define:

- profile id and schema version
- supported product/report revision
- report template names
- report discovery patterns
- unit folder serial-number rules
- task groups and display order
- STEP numbers and CSV file patterns
- CSV parsing mode
- source columns and row selection
- scaling and rounding rules
- computed accuracy rules and pass/fail thresholds
- destination workbook, sheet, cell, and number format
- required vs optional mappings

As of `v0.2.7`, the app has a generic data-driven mapping processor path. The current production profile uses that path for the 208V and 415V transformer report writes. System, breaker, and burn-in tasks still use built-in Rust processors as the fallback path and can carry full `mappings` later as each processor is moved to data-driven layout execution.

## Suggested Shape

```json
{
  "schema_version": 1,
  "profile_id": "pdu500.rev02",
  "display_name": "PDU500 0.2CT Rev02",
  "templates": {
    "default_template_root": "C:/PDU500/00_Template",
    "main_report_template": "PDUD500442AM088_Test Report_0.2CT_Rev02_SN##.xlsx",
    "print_report_template": "PDUD500442AA088_0.2CT Test Report Print.xlsx"
  },
  "workbooks": {
    "main": {
      "file_pattern": "PDUD500442AM088_Test Report_0.2CT_Rev02_SN*.xlsx"
    },
    "print": {
      "file_pattern": "PDUD500442AA088*.xlsx"
    }
  },
  "tasks": []
}
```

## Verification Rules

The upgraded legacy scripts calculate accuracy values in Python before deciding pass/fail. The new layout profile should represent those checks explicitly instead of hiding them in code.

Initial threshold behavior to preserve for 208V/415V system and breaker checks:

| Metric | Threshold |
| --- | --- |
| Voltage | +/-0.3% |
| Current | +/-0.3% |
| Active/Apparent Power | +/-0.6% |
| Power Factor | +/-2.0% |

Suggested shape:

```json
{
  "label": "Voltage A Accuracy",
  "formula": "percent_error",
  "meter_cell": "E17",
  "detect_cell": "F17",
  "target": {
    "workbook": "main",
    "sheet": "System Test - 480_208",
    "cell": "G17",
    "number_format": "0.00"
  },
  "pass_fail": {
    "max_abs": 0.3,
    "missing_is_fail": true
  }
}
```

Power factor uses a different formula in the upgraded scripts: `(detect - meter) * 100`. Other metrics use `(detect - meter) / meter * 100`.

## Mapping Rules

Each mapping should say:

- which source CSV column to read
- which source row to use
- whether the value is required
- how to scale the value
- how to round or format it
- which workbook/sheet/cell to write

Example:

```json
{
  "label": "Active Power",
  "source": {
    "column": "AC",
    "row": "last_numeric",
    "required": true
  },
  "transform": {
    "scale_by": 1000,
    "round": 2
  },
  "target": {
    "workbook": "main",
    "sheet": "System Test - 480_208",
    "cell": "E12",
    "number_format": "0.00"
  }
}
```

## Validation Rules

The app should reject a layout profile when:

- required top-level fields are missing
- duplicate task ids exist
- a task has no step number
- a mapping has no target workbook, sheet, or cell
- a mapping references an unknown workbook key
- source column names are invalid
- scaling is zero or nonnumeric
- required mappings do not define source rules
- verification rules reference missing meter/detect/target cells

The app should warn, not fail, when:

- optional mappings are missing source rules
- a sheet or cell only appears in optional mappings
- a profile has unknown future fields

## Future Config Candidates

Some workflow values are report-layout targets and should become config-driven later. For now, the backend implements the Transformer SN setup write directly in the setup command:

- Transformer SN setup target: main report workbook, `Test Summary!D1`.
- Final operator-name target: print report workbook, `Test Report #2!E39`.
- Error-navigation targets should be derived from the task's existing report mappings when possible, rather than duplicated in a separate map.

Some values are app settings, not report-layout config:

- Saved operator-name dropdown options.
- Default behavior for opening the print dialog after a full pass.
- New-unit detection preferences, once the reliable ATS signal is known.

## Editing Workflow

For future Excel layout changes:

1. Copy the active layout profile.
2. Change `profile_id` and `display_name`.
3. Update only changed sheet/cell/source mappings.
4. Run config validation.
5. Run fixture tests against safe workbook copies.
6. For mapped tasks, run a real or safe copied unit through the generic mapping path and manually inspect the written cells.
7. Commit the new profile.
8. Change the app default profile only after validation, or set `PDU_LAYOUT_PROFILE_PATH` to test a profile outside the repo.

## Why JSON For Now

JSON is strict, easy to diff, easy to parse from Rust and TypeScript, and does not add another dependency decision. If comments become necessary, the project can later move to JSONC or TOML, but the first production profile should stay simple and machine-valid.
