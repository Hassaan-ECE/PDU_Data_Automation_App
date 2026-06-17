# Migration Plan

This plan moves the current working Python/PyQt script bundle into a maintainable Tauri/React/Rust app without breaking the operator workflow.

## Phase 0 - Source Control And Baseline

Acceptance criteria:

- GitHub repository exists as `PDU_Data_Automation_App`.
- This scaffold is committed.
- Legacy source remains untouched and available at `C:\Projects\Active\PDU_Data_Automation`.
- Current legacy behavior is documented in this repo.
- Sample safe copies of templates and CSVs are identified for fixture creation.

Tasks:

- Add remote after the GitHub repo is created.
- Confirm final app name, identifier, and S-drive release root.
- Confirm whether the first release should be `0.1.0`.
- Capture screenshots of the current PyQt panel for UI parity.

## Phase 1 - App Skeleton

Acceptance criteria:

- Tauri 2 desktop app launches.
- React UI shell renders.
- Bun scripts match the `TE_Component_Inventory` workflow.
- Version source is consistent across `package.json`, `backend/Cargo.toml`, and `backend/tauri.conf.json`.
- Basic lint/test/build commands exist.

Tasks:

- Add root `package.json`, `bun.lock`, TypeScript configs, Vite config, and frontend source.
- Add backend `Cargo.toml`, Tauri config, capabilities, and Rust entry points.
- Add app icon placeholder.
- Add scripts for Bun and release staging.

## Phase 2 - UI Parity Shell

Acceptance criteria:

- UI keeps the legacy panel's functional layout.
- All major sections appear in the same order.
- Breaker groups are expandable.
- Test states are color-coded.
- Folder selection, start, pause/resume, reset, manual rerun, and open report buttons exist.

Tasks:

- Model task sections in TypeScript.
- Build the panel layout.
- Add responsive sizing suitable for the existing operator station.
- Add non-destructive mock data mode.

## Phase 3 - Layout Config Model

Acceptance criteria:

- App can load and validate a report-layout config file.
- Config describes STEP numbers, CSV source columns, scaling, report sheets, and target cells.
- Invalid config produces clear errors.
- No Excel writing is required yet.

Tasks:

- Define Rust and TypeScript types for layout profiles.
- Add schema-like validation.
- Start with `config/report-layouts/pdu500.layout.example.json`.
- Convert legacy mappings group by group.

## Phase 4 - Excel Template Spike

Acceptance criteria:

- Backend can open a copy of the current main report template.
- Backend can write a small set of cells.
- Saved workbook opens in Excel without repair.
- Formatting, formulas, merged cells, and print settings are preserved enough for production use.
- Spike result is documented before implementing full report writing.

Tasks:

- Test candidate Rust workbook crates against real template copies.
- Compare before/after workbook structure.
- Decide final workbook writer strategy.
- Document the decision under `docs/decisions/`.

## Phase 5 - CSV Discovery And Processing

Acceptance criteria:

- Backend discovers CSVs recursively by STEP number.
- Backend can tell file detected, stable, locked, missing, and unreadable states apart.
- CSV parsing distinguishes missing values from numeric zero.
- Processing returns structured results.

Tasks:

- Implement unit-folder scanning.
- Implement CSV source parser.
- Add fixture tests for representative CSV files.
- Add structured logs.

## Phase 6 - Report Writers By Section

Acceptance criteria:

- Each legacy processor group is replaced and tested before moving to the next group.
- Written cells match the legacy output for known fixture cases.
- Missing required data fails the task instead of writing fake zeroes.

Recommended order:

1. Transformer checks.
2. 208V system load tests.
3. 415V system load tests.
4. 208V breaker load tests.
5. 415V breaker load tests.
6. System burn-in after STEP71/STEP72 is resolved.
7. Breaker burn-in.

## Phase 7 - Release And Update Flow

Acceptance criteria:

- Current-user NSIS installer builds.
- Signed updater artifacts are generated.
- GitHub Release contains installer, signature, checksums, and `latest.json`.
- S-drive root contains one obvious current installer.
- Release support files live under a support/archive folder, not mixed with runtime data.

Tasks:

- Generate a PDU-specific updater key outside the repo.
- Add release staging script.
- Add manual smoke checklist.
- Validate install, launch, update check, and uninstall.

## Phase 8 - Cutover

Acceptance criteria:

- Operators can install and run the new app.
- A known unit folder produces a correct report.
- Legacy app remains available as fallback during initial rollout.
- Release notes clearly describe any intentional behavior corrections.

Tasks:

- Run side-by-side report comparison.
- Run operator smoke on the production machine.
- Tag first release.
- Archive the legacy app only after successful production use.
