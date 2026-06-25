# Fixtures

Sanitized test data for backend integration tests and future fixture smoke scripts.

Do not store confidential production data, customer data, or real private test records here unless they have been sanitized and approved for source control.

## Rules

- Prefer synthetic or sanitized samples.
- Keep fixture names descriptive.
- Document what each fixture covers when adding files.
- Keep fixture workbooks small enough for practical test runs.
- Do not store temporary Excel lock files (`~$*.xlsx`).
- Tests must not mutate canonical fixture files directly; copy to a temp directory first.

## Target Layout

Subfolders are created only when the first real fixture file is added. Do not create empty placeholder directories.

```text
fixtures/
  unit-folders/   # Sample unit folder structures (CSV + workbook layout)
  csv/            # Small CSV files per STEP/test type
  workbooks/      # Safe Excel templates for report-write tests
  expected/       # Expected cell values or output snapshots
  README.md
```

## High-Value Test Coverage

When fixtures are added, backend integration tests under `backend/tests/` should cover:

- Main report workbook discovery
- Print report workbook discovery
- Transformer SN text preservation
- Final operator name text write
- Missing or locked workbook errors (where practical)
- Missing sheet/cell errors
- CSV missing-value handling (must not silently become zeroes)
- STEP71/STEP72 burn-in behavior

## Backend Test Direction

Integration tests live under `backend/tests/`. Current files:

```text
backend/tests/
  csv_parsing.rs
  report_discovery.rs
  report_writes.rs
  unit_folder_detection.rs
```

Planned future files:

```text
backend/tests/
  layout_profiles.rs
```

Unit tests already exist inside `backend/src/` modules. Integration tests will load fixtures from this directory via relative paths and copy workbooks to temp dirs before mutation.

## Current Status

Current fixture files:

- `unit-folders/basic-detected/262343000072/unit_STEP14_TRANSFORMER_TEST_DATA_AVG.csv`
  - Synthetic STEP14 transformer CSV used by backend integration tests for unit-folder detection.
  - Does not contain production data.
- `csv/missing_required_column_STEP14_TRANSFORMER_TEST_DATA_AVG.csv`
  - Synthetic STEP14 transformer CSV with too few columns for required column `Z`.
- `csv/blank_numeric_STEP14_TRANSFORMER_TEST_DATA_AVG.csv`
  - Synthetic STEP14 transformer CSV with required column `Z` present but blank.
- `csv/malformed_numeric_STEP14_TRANSFORMER_TEST_DATA_AVG.csv`
  - Synthetic STEP14 transformer CSV with required column `Z` set to `not-a-number`.
- `csv/sample_STEP71_SYSTEM_BURN_IN_SOAK.csv`
  - Synthetic STEP71 burn-in/soak CSV used to assert STEP71 is not report-value capture.
- `csv/valid_STEP72_SYSTEM_ACCURACY_TEST_DATA_AVG_report_capture.csv`
  - Synthetic STEP72 system burn-in report capture CSV with valid numeric values.
- `csv/sample_STEP72_SYSTEM_ACCURACY_TEST_DATA_AVG_report_capture.csv`
  - Synthetic STEP72 system burn-in report capture CSV used alongside STEP71 to assert STEP72 remains the report-value capture step.
- `workbooks/PDUD500442AM088_Test Report_0.2CT_Rev02_SN262343000072.xlsx`
  - Minimal synthetic main report workbook fixture for discovery tests.
- `workbooks/PDUD500442AM088_Test Report_0.2CT_Rev02_SN999999999999.xlsx`
  - Minimal synthetic alternate main report workbook fixture for multiple-match discovery tests.
- `workbooks/PDUD500442AM088_Test Report_0.2CT_Rev02_DRAFT.xlsx`
  - Minimal synthetic main-report-prefix fixture for discovery priority tests.
- `workbooks/PDUD500442AM088_misc.xlsx`
  - Minimal synthetic broad main-report-prefix fixture for discovery priority tests.
- `workbooks/PDUD500442AA088_0.2CT Test Report Print.xlsx`
  - Minimal synthetic print report workbook fixture for discovery tests.
- `workbooks/PDUD500442AA088_ALT_Print.xlsx`
  - Minimal synthetic alternate print report workbook fixture for multiple-match discovery tests.
- `workbooks/PDUD500442AA088_Write Smoke Test Report Print.xlsx`
  - Minimal synthetic print report workbook fixture with `Test Report #2` for final operator-name write tests.
- `workbooks/Unrelated_Report.xlsx`
  - Minimal synthetic unrelated workbook fixture used to assert discovery ignores non-matching workbooks.

Add future subfolders and fixture files together in small, validated steps per `docs/STRUCTURE_CLEANUP_PLAN.md` Phase 5.
