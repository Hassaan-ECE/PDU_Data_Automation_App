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

Initial example:

```text
config/report-layouts/pdu500.layout.example.json
```

Production profiles should use explicit names, for example:

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
- destination workbook, sheet, cell, and number format
- required vs optional mappings

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

The app should warn, not fail, when:

- optional mappings are missing source rules
- a sheet or cell only appears in optional mappings
- a profile has unknown future fields

## Editing Workflow

For future Excel layout changes:

1. Copy the active layout profile.
2. Change `profile_id` and `display_name`.
3. Update only changed sheet/cell/source mappings.
4. Run config validation.
5. Run fixture tests against safe workbook copies.
6. Commit the new profile.
7. Change the app default profile only after validation.

## Why JSON For Now

JSON is strict, easy to diff, easy to parse from Rust and TypeScript, and does not add another dependency decision. If comments become necessary, the project can later move to JSONC or TOML, but the first production profile should stay simple and machine-valid.
