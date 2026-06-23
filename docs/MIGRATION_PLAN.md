# Migration Plan

This plan moves the current working Python/PyQt script bundle into a maintainable Tauri/React/Rust app without breaking the operator workflow.

## Planning Update - Operator Workflow Improvements

Near-term priorities after the core workflow is stable:

1. Release and operator-test the inline unit selection and Transformer SN setup/save flow.
2. Verify and fix the error action that opens Excel near the failed step. Exact sheet/cell selection may require Excel automation; opening the correct workbook remains the fallback.
3. Operator-test the manual Print Report flow that captures the final operator name, writes it to `Test Report #2!E39`, and opens Excel's print UI for confirmation.
4. Improve the system burn-in timer so the UI can show the long burn-in countdown followed by the short STEP72 data-capture countdown, while still presenting burn-in as one operator workflow.
5. Add ATS/new-SN detection after the main setup and completion flows are reliable. The app should prompt before switching to or setting up a newly detected unit.
6. Harden failure recovery for locked workbooks, interrupted writes, app restarts, and unavailable network/S-drive paths.

Deferred or not planned right now:

- Do not add an in-app cutover checklist.
- Do not add report-confidence tooling yet.
- Do not add detailed CSV traceability views yet.
- Keep layout config validation improvements as a later item.
- Keep operator run history and manual override notes as a later item after the app works fully.
- Do not add built-in legacy comparison mode unless manual validation gets blocked by a hard-to-diagnose mismatch.

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
- Version is currently `0.2.9` in `package.json`, `backend/Cargo.toml`, and `backend/tauri.conf.json`.
- For `v0.2.9`, Rust formatting check, Rust check, Rust unit tests, frontend tests, frontend build, and frontend lint have been run.
- Signed NSIS current-user installers have been built for `0.1.0`, `0.2.0`, `0.2.1`, `0.2.2`, `0.2.3`, `0.2.4`, `0.2.5`, `0.2.6`, `0.2.7`, `0.2.8`, and `0.2.9`.
- A PDU-specific updater key has been generated outside the repo and the public key is configured in Tauri.
- The `0.1.0` installer has been staged at `S:\Engineering\Public\Syed_Hassaan_Shah\PDU_Data_Automation`.
- GitHub Releases `v0.1.0`, `v0.2.0`, `v0.2.1`, `v0.2.2`, `v0.2.3`, `v0.2.4`, `v0.2.5`, `v0.2.6`, `v0.2.7`, `v0.2.8`, and `v0.2.9` have been published with the installer, updater signature, `latest.json`, and `SHA256SUMS.txt`.
- `latest.json` resolves and points to the uploaded `v0.2.9` GitHub release asset.
- A real updater upgrade smoke test is pending from an installed older updater-capable build to `v0.2.9`. `v0.1.0` and `v0.2.0` cannot initiate the updater flow because their Tauri capability file did not grant updater permissions.

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
- `v0.2.8` frontend changes replace the Start-time setup modal with inline unit selection and inline Transformer SN entry.
- Unit selection is inline: the first row shows only the selected unit SN, uses `Select Test Unit...` as placeholder text, and selects a unit only through the `...` browse button. The UI no longer auto-selects the latest detected unit at startup.
- Transformer SN is inline: the field uses `Transformer SN...` as placeholder text and saves on Start, blur, Enter, or the icon-only `Save Transformer SN` button. A successful save displays `Saved` inside the input.
- Report opening is blocked while Transformer SN is missing or unsaved.
- Current-step follow behavior is explicit: `Start`, `Resume`, `Follow Step`, and `Current Step` enable follow mode and scroll to the active step; manual wheel/touch scroll and expand/collapse disable follow mode.
- The first updater check is now readiness-based: it waits for backend status and layout profile startup requests to settle, then runs after a short post-ready delay.
- Frontend tests cover inline unit/SN controls, no latest-unit auto-suggestion call, browse selection, Start setup with Transformer SN, setup error handling, previous-tests prompt, late Transformer SN save, current-step follow behavior, and readiness-based updater timing.
- `v0.2.9` adds a manual Print Report flow with side-by-side Open Report and Print Report actions, final operator-name capture, local saved-operator names, Transformer SN/report setup guards, and Excel print UI confirmation.
- Frontend tests also cover the Print Report action layout, operator modal, local saved-name behavior, backend save/dialog calls, print-dialog errors, and Transformer SN print guards.
- Local validation for the `v0.2.9` print release passed on 2026-06-23: Rust formatting check, Rust check, Rust tests, frontend tests, frontend build, and frontend lint.

Remaining:

- Continue operator-machine smoke testing for screen fit, long-session readability, and failure-state ergonomics.
- Continue operator-machine smoke testing for the manual Print Report flow.
- Verify that error actions open the intended report context, or clearly fall back to opening the workbook.

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
- A generic data-driven mapping processor path exists.
- The 208V and 415V transformer report writes are now represented as mappings in the production layout profile.
- System, breaker, and burn-in tasks still use built-in Rust processors as the fallback path.

Remaining:

- Move more hardcoded system, breaker, and burn-in processor cell/source logic into data-driven mappings when the workflow is stable.
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
- Backend support now writes Transformer SN as inline text to the main report `Test Summary!D1` through `setup_unit_folder_with_transformer_sn`.
- Backend support now also saves later Transformer SN edits through `save_transformer_sn`, preserving numeric-looking values such as `000123` as text.
- Backend validation for the Transformer SN setup slice passed: `cargo fmt --manifest-path backend\Cargo.toml --check`, `cargo check --manifest-path backend\Cargo.toml`, and `cargo test --manifest-path backend\Cargo.toml`.
- The frontend inline setup/save flow is connected to `setup_unit_folder_with_transformer_sn` for Start-time setup and `save_transformer_sn` for later edits.

Remaining:

- Repeat workbook validation across more real or copied units, including reports with existing formulas and workbooks already open in Excel.
- Repeat manual Print Report validation on the operator PC after installing `v0.2.9`.

## Phase 5 - CSV Discovery And Processing

Acceptance criteria:

- Backend discovers CSVs recursively by STEP number.
- Backend can treat active ATS-written CSV sharing violations as not ready without failing the task.
- CSV parsing distinguishes missing values from numeric zero.
- Processing returns structured results.

Current status:

- Unit-folder scanning detects existing STEP CSV files and maps them to task states.
- Backend command `find_latest_unit_candidate` suggests the newest likely unit folder/SN from likely PDU roots, using layout/template-root context and ignoring support/template folders.
- CSV parsing and required numeric extraction are implemented.
- Processing distinguishes missing/unparsable required values from valid numeric zeroes.
- Per-task processing returns structured state, code, message, log, report paths, and failure detail.
- Active ATS-written CSV files now keep the task in a waiting state instead of showing an I/O-bound processing failure.
- When starting mid-test, detected CSVs that still have nominal time remaining are treated as the current waiting task instead of skipped backlog.
- The operator timer now uses current active-step remaining time plus future unpassed step time, so mid-test countdowns do not bounce between partial totals.

Remaining:

- Add scrubbed fixture tests for representative production CSV files.
- Add more structured diagnostics for unusual unreadable-file cases beyond active writer locks.
- Add more structured logging around CSV selection and parse failures.
- Operator-test the inline unit browse/start setup path. The backend still has `find_latest_unit_candidate`, but the current frontend no longer auto-suggests or auto-selects the latest unit.
- Later, add passive detection of newly started ATS unit/SN folders and prompt before setting up or switching the app to the new unit.

## Phase 6 - Report Writers By Section

Acceptance criteria:

- Each legacy processor group is replaced and tested before moving to the next group.
- Written cells match the legacy output for known fixture cases.
- Missing required data fails the task instead of writing fake zeroes.

Current status:

- The generic data-driven mapping path handles the 208V and 415V transformer report writes from `config/report-layouts/pdu500.rev02.layout.json`.
- Rust processors remain for system, breaker, and burn-in tasks.
- 208V and 415V breaker accuracy now preserve the upgraded Python scripts' voltage-specific rounding behavior: 208V verifies from rounded report values at 4 decimals, while 415V verifies from raw scaled values at 2 decimals.
- The installed `v0.1.0` app processed one known-good unit successfully, and the generated Excel report opened without repair prompts.
- The `v0.2.6` release was smoke-tested with `C:\PDU500\262343000072`; the generated data was manually reviewed and looked good.

Remaining validation order:

1. Transformer checks.
2. 208V system load tests.
3. 415V system load tests.
4. 208V breaker load tests.
5. 415V breaker load tests.
6. System burn-in using STEP72 report-data capture while representing the STEP71 burn-in period in the workflow.
7. Breaker burn-in.

Use `C:\Projects\Active\Data Automation Upgraded` as the primary source for 208V/415V system and breaker verification thresholds and failure behavior.

Burn-in timing note:

- Keep the operator UI as a single burn-in workflow, but support a long burn-in countdown followed by the short STEP72 capture countdown used for report values.

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
- The signed `v0.2.9` installer, `.sig`, `latest.json`, and checksums have been published to GitHub Release `v0.2.9`.
- The `v0.2.9` installer has been staged on the S-drive at `S:\Engineering\Public\Syed_Hassaan_Shah\PDU_Data_Automation\PDU Data Automation_0.2.9_x64-setup.exe`.
- The installed app launches without a console window.
- The updater endpoint has been verified to return `0.2.9`.

Remaining:

- Test a real updater upgrade from an installed older updater-capable build to `v0.2.9` on the operator PC.
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
- The `v0.2.6` release has been run against `C:\PDU500\262343000072`, and the generated data was manually reviewed as good.

Remaining:

- Manually review generated reports for representative known-good and known-fail units.
- Run operator smoke on the production machine across normal, warning, failure, and rerun paths.
- Keep the legacy app available as fallback during initial pilot use.
- Archive the legacy app only after successful production use across multiple units.
