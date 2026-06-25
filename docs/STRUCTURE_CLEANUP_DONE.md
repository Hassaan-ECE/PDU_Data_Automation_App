# Structure Cleanup Done

This file records the structure-cleanup work that has been completed and validated.

## Completed Phases

### Phase 0: Audit Before Cleanup

Completed.

What was checked:

- Current dirty worktree state.
- Existing top-level folders.
- Current `backend/` Tauri crate layout.
- Current package/version files.
- Active package scripts.
- Root and project instructions.

### Phase 1: Documentation and Root Hygiene

Completed.

What changed:

- Added this cleanup documentation set.
- Updated [`PROJECT_STRUCTURE.md`](PROJECT_STRUCTURE.md).
- Updated [`fixtures/README.md`](../fixtures/README.md).
- Updated [`scripts/README.md`](../scripts/README.md).
- Kept root `package.json` as the project task runner.
- Kept `.tmp/` as ignored scratch only.

### Phase 2: Backend Command Boundary Cleanup

Completed.

What changed:

- Added `backend/src/commands.rs` as the thin Tauri command boundary.
- Reduced `backend/src/lib.rs` to app wiring, plugin setup, status timing, and command registration.
- Kept `backend/` as the Tauri crate root.
- Kept command names and serialized behavior unchanged.
- Left CSV parsing, report writing, unit-folder scanning, and config loading in domain modules.

### Phase 3: Shared Contract Strategy

Completed for the first shared-contract target.

What changed:

- Added `shared/README.md`.
- Added `shared/schemas/report-layout.schema.json`.
- Added `scripts/fixtures/validate-report-layout-schema.mjs`.
- Added package script `validate:report-layouts`.
- Kept IPC DTO ownership in `frontend/src/integrations/tauri/backend.ts` for now.

### Phase 4: Frontend Structure Cleanup

Completed for the active cleanup target.

What changed:

- Extracted operator-name persistence/autocomplete helpers to
  `frontend/src/features/test-panel/operatorNames.ts`.
- Kept the test-panel workflow inside `frontend/src/features/test-panel/`.
- Did not move feature code into frontend shared folders without a second consumer.

### Phase 5: Fixtures and Backend Integration Tests

Completed for the first fixture/test slices.

What changed:

- Added synthetic unit-folder fixture:
  `fixtures/unit-folders/basic-detected/262343000072/unit_STEP14_TRANSFORMER_TEST_DATA_AVG.csv`.
- Added synthetic CSV regression fixtures under `fixtures/csv/`.
- Added synthetic workbook discovery fixtures under `fixtures/workbooks/`.
- Added `backend/tests/unit_folder_detection.rs`.
- Added `backend/tests/report_discovery.rs`.
- Added `backend/tests/report_writes.rs`.
- Added `backend/tests/csv_parsing.rs`.
- Tests copy fixture data to temp dirs before mutation.
- CSV parsing tests generate minimal temp workbooks only inside test temp dirs.

Covered by these slices:

- Unit-folder detection from fixture data.
- STEP14 transformer detection from a synthetic CSV.
- Final operator name written as text to `Test Report #2!E39` in a print report workbook.
- Missing required CSV columns fail clearly.
- Blank required numeric CSV values fail clearly.
- Malformed required numeric CSV values fail clearly.
- Required CSV failures are not treated as successful zero-value writes.
- STEP72 remains the report-value capture step for system burn-in.
- STEP71 burn-in/soak data is detected but does not satisfy STEP72 report capture.
- Main report workbook discovery.
- Print report workbook discovery.
- Missing main and print report errors from an empty unit folder.
- Unrelated `.xlsx` files do not satisfy report discovery.
- Multiple matching main reports follow current category priority and latest-SN behavior.
- Multiple matching print reports follow current latest-modified behavior.
- Transformer SN writes use an inline text cell.
- Long numeric-looking Transformer SN values preserve exact text and leading zeros.
- Transformer SN writes are not converted to numbers or scientific notation.
- Final operator name writes use an inline text cell.
- Written workbook ZIPs can still be loaded by backend test code after mutation.

### Phase 6: Scripts and CI Source Validation

Completed for source validation.

What changed:

- Added `scripts/validate-local.mjs`.
- Added `scripts/release/check-version-consistency.mjs`.
- Added package scripts:
  - `validate`
  - `check:versions`
  - `validate:report-layouts`
- Added `.github/workflows/ci.yml`.
- CI runs source validation on `windows-latest`.
- CI does not build signed installers, publish releases, access S-drive paths, or require updater keys.

## Validation Run

Passed:

```powershell
node scripts\run-bun.mjs run validate
git diff --check
```

The full validation command covered:

- frontend lint
- frontend tests
- frontend build
- version consistency check
- report-layout schema validation
- Rust formatting check
- Rust compile check
- Rust unit and integration tests

## Not Part Of This Completed Set

The following were intentionally left out of the completed cleanup because they need separate
prerequisites or real release/packaging validation:

- Optional `backend/src-tauri/` migration.
- Signed desktop installer build validation.
- Generated installer launch validation.
- Updater metadata generation validation.
- Release checksum helper.
- S-drive staging helper.
- Fixture copy/setup helper.
- Full IPC type generation.

See [`STRUCTURE_CLEANUP_REMAINING.md`](STRUCTURE_CLEANUP_REMAINING.md) for what each item is
waiting on.
