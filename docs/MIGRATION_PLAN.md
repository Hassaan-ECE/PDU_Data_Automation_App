# Migration Plan

This plan moves the current working Python/PyQt script bundle into a maintainable Tauri/React/Rust app without breaking the operator workflow.

## Phase 0 - Source Control And Baseline

Acceptance criteria:

- GitHub repository exists as `PDU_Data_Automation_App`.
- Initial app and release work is committed.
- Legacy source remains untouched and available at `C:\Projects\Active\PDU_Data_Automation`.
- Newer verification reference remains available at `C:\Projects\Active\Data Automation Upgraded`.
- Current legacy behavior is documented in this repo.
- Sample safe copies of templates and CSVs are identified for fixture creation.

Current status:

- GitHub remote is configured at `https://github.com/Hassaan-ECE/PDU_Data_Automation_App.git`.
- The working sample data and templates are available under `C:\PDU500`.
- Representative old and upgraded processors have been smoke-tested against copied sample folders.
- STEP71/STEP72 system burn-in behavior is now understood: STEP71 is the long burn-in period, STEP72 is the quick report-data capture.

Remaining tasks:

- Capture screenshots of the current PyQt panel for UI parity.

## Phase 1 - App Skeleton

Current status:

- Initial Tauri 2, React, TypeScript, Vite, Tailwind, Bun, and Rust skeleton is present.
- Version is currently `0.2.3` in `package.json`, `backend/Cargo.toml`, and `backend/tauri.conf.json`.
- Frontend build, frontend test, frontend lint, Rust formatting, and Rust unit tests have been run.
- Signed NSIS current-user installers have been built for `0.1.0`, `0.2.0`, `0.2.1`, `0.2.2`, and the CSV-readiness `0.2.3` release.
- A PDU-specific updater key has been generated outside the repo and the public key is configured in Tauri.
- The `0.1.0` installer has been staged at `S:\Engineering\Public\Syed_Hassaan_Shah\PDU_Data_Automation`.
- GitHub Releases `v0.1.0`, `v0.2.0`, `v0.2.1`, `v0.2.2`, and `v0.2.3` have been published with the installer, updater signature, `latest.json`, and `SHA256SUMS.txt`.
- `latest.json` resolves and points to the uploaded GitHub release asset.
- A real updater upgrade smoke test is pending from `v0.2.2` to `v0.2.3`. `v0.1.0` and `v0.2.0` cannot initiate the updater flow because their Tauri capability file did not grant updater permissions.

Acceptance criteria:

- Tauri 2 desktop app launches.
- React UI shell renders.
- Bun scripts match the `TE_Component_Inventory` workflow.
- Version source is consistent across `package.json`, `backend/Cargo.toml`, and `backend/tauri.conf.json`.
- Basic lint/test/build commands exist.

Completed setup work:

- Add root `package.json`, `bun.lock`, TypeScript configs, Vite config, and frontend source.
- Add backend `Cargo.toml`, Tauri config, capabilities, and Rust entry points.
- Add app icon asset.
- Add scripts for Bun and release builds.

## Phase 2 - UI Parity Shell

Acceptance criteria:

- UI keeps the legacy panel's functional layout.
- All major sections appear in the same order.
- Breaker groups are expandable.
- Test states are color-coded.
- Folder selection, start, pause/resume, reset, manual rerun, and open report buttons exist.

Current status:

- Operator panel renders the current PDU500 workflow with expandable 208V, 415V, and burn-in groups.
- Folder selection, setup, start/pause, reset, rerun, open report, update status, and error-card controls are present.
- Browser-only mock fallback exists for frontend development outside Tauri.

Remaining:

- Continue operator-machine smoke testing for screen fit, long-session readability, and failure-state ergonomics.

## Phase 3 - Layout Config Model

Acceptance criteria:

- App can load and validate a report-layout config file.
- Config describes STEP numbers, CSV source columns, scaling, report sheets, and target cells.
- Invalid config produces clear errors.

Current status:

- Rust and TypeScript layout-summary types are present.
- The active bundled profile is `config/report-layouts/pdu500.rev02.layout.json`.
- The production profile validates with zero warnings and matches the 65 built-in task IDs.
- `PDU_LAYOUT_PROFILE_PATH` can override the bundled profile for testing an edited profile.
- Current production tasks use `processor` fields where report writing is still handled by built-in Rust processors.

Remaining:

- Move more hardcoded processor cell/source logic into data-driven mappings when the workflow is stable.
- Add fixture validation for any future profile revision before making it the default.

## Phase 4 - Excel Template Spike

Acceptance criteria:

- Backend can open a copy of the current main report template.
- Backend can write a small set of cells.
- Saved workbook opens in Excel without repair.
- Formatting, formulas, merged cells, and print settings are preserved enough for production use.
- Spike result is documented before implementing full report writing.

Tasks:

- Keep testing workbook patching against safe template copies.
- Compare before/after workbook structure when templates change.
- Document the final workbook patching decision under `docs/decisions/`.

Current status:

- The backend patches workbook XML directly inside the `.xlsx` package instead of using Excel automation for report writes.
- Unit tests cover cell insertion/replacement, style preservation for patched cells, shared formula expansion, calc chain removal, and recalculation flags.
- A known-good unit was processed through the installed `v0.1.0` app, and the generated workbook opened in Excel without repair prompts.

Remaining:

- Repeat workbook validation across more real or copied units, including reports with existing formulas and workbooks already open in Excel.

## Phase 5 - CSV Discovery And Processing

Acceptance criteria:

- Backend discovers CSVs recursively by STEP number.
- Backend can treat active ATS-written CSV sharing violations as not ready without failing the task.
- CSV parsing distinguishes missing values from numeric zero.
- Processing returns structured results.

Current status:

- Unit-folder scanning detects existing STEP CSV files and maps them to task states.
- CSV parsing and required numeric extraction are implemented.
- Processing distinguishes missing/unparsable required values from valid numeric zeroes.
- Per-task processing returns structured state, code, message, log, report paths, and failure detail.
- Active ATS-written CSV files now keep the task in a waiting state instead of showing an I/O-bound processing failure.

Remaining:

- Add scrubbed fixture tests for representative production CSV files.
- Add more structured diagnostics for unusual unreadable-file cases beyond active writer locks.
- Add more structured logging around CSV selection and parse failures.

## Phase 6 - Report Writers By Section

Acceptance criteria:

- Each legacy processor group is replaced and tested before moving to the next group.
- Written cells match the legacy output for known fixture cases.
- Missing required data fails the task instead of writing fake zeroes.

Current status:

- Rust processors exist for transformer, 208V/415V system, 208V/415V breaker, system burn-in, and breaker burn-in.
- 208V and 415V breaker accuracy now preserve the upgraded Python scripts' voltage-specific rounding behavior: 208V verifies from rounded report values at 4 decimals, while 415V verifies from raw scaled values at 2 decimals.
- The installed `v0.1.0` app has processed one known-good unit successfully, and the generated Excel report opened without repair prompts.

Remaining validation order:

1. Transformer checks.
2. 208V system load tests.
3. 415V system load tests.
4. 208V breaker load tests.
5. 415V breaker load tests.
6. System burn-in using STEP72 report-data capture while representing the STEP71 burn-in period in the workflow.
7. Breaker burn-in.

Use `C:\Projects\Active\Data Automation Upgraded` as the primary source for 208V/415V system and breaker verification thresholds and failure behavior.

## Phase 7 - Release And Update Flow

Acceptance criteria:

- Current-user NSIS installer builds.
- Signed updater artifacts are generated.
- GitHub Release contains installer, signature, checksums, and `latest.json`.
- S-drive root contains one obvious current installer.
- Release support files live under a support/archive folder, not mixed with runtime data.

Current status:

- A PDU-specific updater key has been generated outside the repo.
- The public updater key is configured in Tauri.
- The signed `v0.1.0` installer, `.sig`, `latest.json`, and checksums have been published to GitHub Release `v0.1.0`.
- The corrected installer has been staged on the S-drive at `S:\Engineering\Public\Syed_Hassaan_Shah\PDU_Data_Automation`.
- The installed app launches without a console window.

Remaining:

- Test a real updater upgrade from `v0.2.2` to `v0.2.3` on the operator PC.
- Validate uninstall and reinstall behavior on the production machine.
- Keep the S-drive root clean as new releases are staged.

## Phase 8 - Cutover

Acceptance criteria:

- Operators can install and run the new app.
- A known unit folder produces a correct report.
- Legacy app remains available as fallback during initial rollout.
- Release notes clearly describe any intentional behavior corrections.

Current status:

- The installed `v0.1.0` app has been run against one known-good unit and produced a report that opened cleanly in Excel.

Remaining:

- Run side-by-side report comparison against legacy output for representative known-good and known-fail units.
- Run operator smoke on the production machine across normal, warning, failure, and rerun paths.
- Keep the legacy app available as fallback during initial pilot use.
- Archive the legacy app only after successful production use across multiple units.
