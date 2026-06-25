# Code Review: PDU_Data_Automation_App (v0.2.9 pilot)

**Scope**: Full codebase review (not a PR diff). Covers architecture, critical paths (unit detection, CSV, processors/mapped, Excel patching, unit_state, print readiness, transformer/operator writes), error handling, duplication, frontend complexity, testing, packaging, and alignment with AGENTS.md + LEGACY_BEHAVIOR.md + docs/.

**Process**: Listed root + subdirs (backend/src, frontend/src, config/, shared/, docs/, fixtures/, scripts/); read AGENTS.md, all core *.rs (mod.rs, commands.rs, automation/*, config/*), key *.tsx/*.ts, tauri.conf.json, Cargo.toml, package.json, layout JSONs + schema, tests, docs (LEGACY, ARCHITECTURE, etc.); grepped for unwrap/expect, TODO/legacy, hardcoded paths, clones/locks, task definitions; analyzed dispatch, patching, persistence, state flows.

**Date of review**: 2026-06-25

## Summary

The v0.2.9 pilot is a solid, well-engineered replacement that largely succeeds at preserving the legacy operator workflow and visual model while moving toward the intended architecture. Strengths include strict no-silent-zero CSV handling, high-fidelity Excel patching with extensive fidelity tests, unit_state-driven restart resilience and idempotency, a working mapped data-driven path for at least transformer writes, and clear error surfacing. The separation of concerns (frontend UI/workflow, Rust file/CSV/Excel logic, config-driven mappings) is mostly respected.

Main remaining risk areas: runtime config discovery ships Tauri resources but does not resolve them at runtime; OperatorPanel.tsx is still monolithic (~2500 LOC); task definitions are duplicated across taskModel.ts / tasks.rs / layout JSONs; profile/config loading is repeated frequently; and the report-write Windows replace path is still delete+rename rather than an atomic replace. The app is pilot-ready for limited production comparison but carries maintenance and deployment risks if not addressed before full cut-over.

## Architecture & Structure

### Alignment with AGENTS.md and docs
- Strong overall: UI state/workflow is in React (OperatorPanel + supporting), file/CSV/report logic is entirely in Rust `automation/` and `config/`. Test definitions + cell maps are (partially) moving into versioned JSON under `config/report-layouts/`.
- Excel report mappings: partially data-driven (transformer tasks use mappings in `pdu500.rev02.layout.json`; system/breaker/burn-in still use built-in processors in `processors.rs`). AGENTS.md goal of "Prefer editable config files ... over hardcoded cell maps" is in progress but incomplete.
- Legacy behavior: docs/LEGACY_BEHAVIOR.md, ARCHITECTURE.md, and decisions/ are present and referenced. Code avoids the forbidden "exit code 2 -> pass" mapping (code 2 is only used for "waiting"). STEP71 (soak detection/timer) vs STEP72 (data capture) is respected: burn-in processors require STEP72.
- Missing/unparsable values: correctly produce "fail"/"warning", never silent 0 (backed by fixtures + csv_parsing.rs + report_writes tests).
- Template preservation: rewrite_workbook + patch_cell_xml + tests aggressively protect formatting, inlineStr text, shared formulas, calcChain removal, merged cells, etc.

### Package / Release structure
- tauri.conf.json (in backend/) correctly declares resources for layouts and uses NSIS currentUser + signed updater.
- Build scripts and package.json use Bun as specified.
- No private keys in repo (good).
- Generated build artifacts correctly ignored (target/, dist/, etc.).

### Positive observations
- `automation/mod.rs` cleanly dispatches to mapped vs processor; `build_summary`, `task_csv_match`, and readiness checks centralize state merging.
- Layout profile loading now fails loudly for unreadable, invalid JSON, or semantically invalid profiles. Scan/build-summary and task processing use a validated profile instead of silently falling back to built-in processors.
- Report setup and latest-unit suggestion now use `profile.templates.default_template_root` instead of the old hardcoded `C:/PDU500/00_Template` directory.
- Report setup, report discovery, built-in processor report lookup, and mapped workbook lookup now use profile-derived template names and `workbooks.*.file_pattern` for main/print reports.
- `reports.rs` Excel path is strong overall (WorkbookLock file mutex, .bak, transactional multi-patch rollback, force recalc, style preservation), with the Windows replace caveat called out below.
- `csv_data.rs`: stability polling, fingerprinting (fnv1a64 + size + mtime), strict `required_number` + typed errors for blank/missing/non-numeric.
- `unit_state.rs`: audit log, an acceptance data shape, fingerprint idempotency, temp+rename+backup save, explicit corrupt-state errors, and a `unit_state.lock` around read-modify-write paths. `is_print_ready` and `already_processed_fingerprint` are directionally correct, but acceptance is not exposed through a command/UI yet (see Issue 15).
- Frontend preserves exact legacy panel layout, color states, timers, follow controls, backlog prompts, operator name capture.
- Tests exist for risky areas (csv failures without zero fallback, report writes fidelity, discovery, unit candidates).
- Schema + validation script (`scripts/fixtures/validate-report-layout-schema.mjs`) + `bun run validate:report-layouts`.

## Detailed Issues

### Issue 1 -- Severity: bug
- File: backend/src/automation/mod.rs and backend/src/automation/unit_state.rs
- Description: Originally, `unit_state::load_or_default(...).unwrap_or_default()` swallowed read/parse/permission errors and proceeded with an empty state. This has been fixed: `process_task`, `build_summary`, and print readiness now propagate unit-state errors; corrupt JSON is mapped to `unit_state_corrupt`; and tests cover corrupt sidecars in scan, print readiness, and process paths.
- Suggestion: No further action for the original bug. Keep the behavior that a missing `unit_state.json` defaults cleanly, while invalid existing state fails loudly.
- Status: resolved on 2026-06-25

### Issue 2 -- Severity: bug
- File: backend/src/automation/mod.rs and backend/src/config/profile.rs
- Description: Originally, `mapped_csv_match` and `process_task_with_profile_mapping` called `load_layout_profile().ok()?`, discarding profile load failures and silently falling back to built-in task matching/processors. This has been fixed: the profile loader now parses and validates before returning, automation paths load a validated profile and pass it through mapped dispatch/CSV matching, and layout-profile errors map to visible command error codes.
- Suggestion: Treat the original bug as resolved. Remaining follow-up: add an end-to-end command test with a deliberately invalid `PDU_LAYOUT_PROFILE_PATH` once environment-variable isolation is available, so the user-visible command path is covered directly rather than only helper/error-mapping paths.
- Status: resolved on 2026-06-25, with end-to-end invalid-profile command coverage as a future test hardening item

### Issue 3 -- Severity: bug
- File: backend/src/automation/reports.rs, backend/src/automation/mod.rs, backend/src/automation/unit_candidates.rs
- Description: Originally, hardcoded `TEMPLATE_DIR = "C:/PDU500/00_Template"` was used by report setup and unit candidate discovery while the active profile also declared `templates.default_template_root`. This has been fixed: setup flows load the layout profile, pass `profile.templates.default_template_root` into report setup, and latest-unit discovery derives its search root from the profile template root's parent.
- Suggestion: Treat the original directory-root bug as resolved. The broader template filename/discovery migration has also since been completed (see Issue 16), with only minor hardening items left around serial placeholder configurability and shared wildcard helpers.
- Status: resolved on 2026-06-25

### Issue 4 -- Severity: bug
- File: backend/src/config/profile.rs:220 (load_layout_profile and candidate_profile_paths)
- Description: Runtime discovery only searches relative to cwd ("config/...", "../config/...") and hard `C:/PDU500/config/...`. The `resources` entry in `backend/tauri.conf.json:35` ("../config/report-layouts/*.json") ships the files, but the Rust loaders do not resolve the bundled resource location. The built-in `include_str!` fallback prevents a hard failure, but it also means installed builds do not actually exercise the packaged JSON resource unless an external `C:/PDU500/config/...` file exists.
- Suggestion: Thread an app/resource path into the profile and accuracy loaders, or add Tauri-aware loader variants that resolve the bundled `config/report-layouts` resource directory before falling back to dev paths and built-in defaults. Add an integration test that simulates a bundled layout and distinguishes "loaded from resource" from "loaded from include_str".
- Status: open

### Issue 5 -- Severity: bug
- File: backend/src/automation/unit_state.rs and backend/src/automation/mod.rs
- Description: Originally, concurrent scan/process/print flows could race on `unit_state.json` read-modify-write and load errors were defaulted away. This has been fixed with `UnitStateLock`, `load_or_ensure_task_entries`, locked `record_processor_result`, and error propagation through the Tauri command layer. A concurrency test now verifies multiple task updates are preserved.
- Suggestion: Treat the original race as resolved. Remaining hardening opportunity: stale `unit_state.lock` files after a crash will block state access until manually removed, matching the current workbook-lock pattern but still worth documenting or improving later with PID/age-based stale-lock recovery.
- Status: resolved on 2026-06-25, with stale-lock cleanup as a future hardening item

### Issue 6 -- Severity: suggestion
- File: frontend/src/features/test-panel/OperatorPanel.tsx (~2500 lines, entire file); see also taskModel.ts:1-100, types.ts
- Description: One giant component (~2500 LOC) owns timers, runner loop, follow-mode scroll logic, 30+ useState/useRef/useEffect/useCallback, backlog prompts, failure notices, transformer SN dirty tracking, operator name capture, update UI, and panel rendering. This violates separation of concerns and makes the file hard to reason about or unit-test in isolation (frontend tests are narrow).
- Suggestion: Extract: runner/useTaskRunner hook, usePanelScroll, TaskSection, StatusHeader, OperatorNamePrompt, useUnitStateSync, etc. Move static layout data out of component. Follow the note in PROJECT_STRUCTURE.md that "the majority of the UI lives in OperatorPanel.tsx (will be split)".
- Status: open

### Issue 7 -- Severity: suggestion
- File: frontend/src/features/test-panel/taskModel.ts:50 (legacyPanelItems) + backend/src/automation/tasks.rs:97 (automation_tasks) + config/report-layouts/pdu500.rev02.layout.json (task_groups)
- Description: Task definitions (ids, labels, steps, detection_steps, structure) are duplicated in three places with different representations. The profile drives only mapped vs processor dispatch and CSV patterns; the UI still hard-codes the tree in TS and Rust still hard-codes the full 65-task list. Drift risk on renames, added breakers, or rev03.
- Suggestion: Drive the React panel structure from the loaded profile (or a derived view model sent from backend via `load_layout_profile` / scan summary). At minimum, centralize task metadata in the layout JSON and generate or validate the TS/Rust lists from it. Keep only UI-specific presentation in frontend.
- Status: open

### Issue 8 -- Severity: suggestion
- File: backend/src/automation/reports.rs:141 (setup), backend/src/automation/mod.rs:760 (mapped_csv_match), and profile loading
- Description: `load_layout_profile()` is called repeatedly (inside `process_task_with_profile_mapping`, `mapped_csv_match`, unit_candidates, etc.) with no caching. Each time it may walk candidate paths + serde. Accuracy thresholds are reloaded every process step (documented as intentional). For a desktop app this is minor but adds unnecessary FS/alloc pressure during a running test.
- Suggestion: Cache the validated profile (and thresholds) in a `OnceLock` or `Mutex` after first successful load (or use Tauri's state management). Reload only on explicit command or file watcher (future). Keep the per-process reload behavior only if the JSONs are expected to change mid-run.
- Status: open

### Issue 9 -- Severity: suggestion
- File: backend/src/automation/reports.rs:350 (patch_workbooks_transactional) and callers
- Description: `rewrite_workbook` does write a sibling temp zip and keeps a `.bak`, which is good. The weak point is `replace_file` on Windows: it removes the existing workbook and then renames the temp file. A process crash or power loss between those two operations can leave the report path missing, with recovery dependent on the sidecar `.bak`. Multi-workbook rollback also only runs when Rust receives an error from a later patch, not on a crash.
- Suggestion: Use an atomic Windows replace primitive (`ReplaceFileW` or `MoveFileExW` with replace semantics) or a small well-tested crate that wraps it. Keep the backup as defense in depth. Add tests around failed replacement and startup/report discovery behavior when `.bak` exists but the primary workbook is missing.
- Status: open

### Issue 10 -- Severity: suggestion
- File: backend/src/automation/mod.rs:350 (validate_ready_for_print_path) and 460 (task loop)
- Description: The print readiness validator iterates every task from `automation_tasks()` and checks persisted state. It re-creates seeds from scratch each time. If a task id exists only in the profile but not the built-in list (or vice-versa), blockers or missing state entries can appear. Also, "detected" state blocks print even if CSV was good but never manually processed (per design), but the message may be confusing to operators.
- Suggestion: Unify task enumeration: always source the authoritative list from the active profile (after profile-driven panel work). Improve blocker messages and consider an "auto-accept on clean CSV" mode if that matches legacy "detected" handling for certain steps.
- Status: open

### Issue 11 -- Severity: nit
- File: backend/src/automation/csv_data.rs:314, reports.rs:290 and many others, mapped.rs:436
- Description: Regex compilation with `.expect("... is valid")` happens at function scope in hot or repeated paths. These are literal patterns, so this is not an operator-input crash risk, but it creates repeated compilation overhead and scatters pattern ownership across the codebase.
- Suggestion: Centralize repeated literal regexes as `static OnceLock<Regex>`/`LazyLock<Regex>` or replace simple cases with direct string parsing. Add a unit test that the central patterns compile.
- Status: open

### Issue 12 -- Severity: nit
- File: frontend/src/integrations/tauri/backend.ts:120 (chooseUnitFolder and many mocks)
- Description: Non-Tauri path returns a hardcoded demo folder and fake results. Useful for browser dev, but the mock data can silently diverge from real backend shapes (e.g., task ids, state transitions).
- Suggestion: Extract mock data from the same sources used by production (or load a fixture profile) so dev mode stays in sync. Consider a "demo mode" flag that re-uses real fixtures.
- Status: open

### Issue 13 -- Severity: suggestion
- File: backend/src/automation/processors.rs (full) + mapped.rs (full) + tasks.rs
- Description: Hard-coded cell addresses, sheet names, column mappings, and burn-in row math still live in the built-in processor path for system/breaker/burn-in. Only transformers have moved to mappings. This recreates the legacy duplication problem the architecture was meant to solve.
- Suggestion: Accelerate migration: add the remaining mappings + verification rules to `pdu500.rev02.layout.json` (or a future rev) and delete or thin the processor branches. Keep processors only for complex calculations that cannot be expressed in the mapping DSL (e.g., multi-row aggregation or special CF logic).
- Status: open

### Issue 14 -- Severity: nit
- File: backend/src/lib.rs:32 and main entry
- Description: Top-level `.expect("error while running PDU Data Automation")` on `tauri::Builder::run`. In a production desktop app this produces a poor user experience on setup failure.
- Suggestion: Replace with a graceful startup failure path: log the error, show a native message box if available at that point, then exit with a non-zero status. Avoid relying on app plugins for errors that happen before the Tauri app is fully running.
- Status: open

### Issue 15 -- Severity: suggestion
- File: backend/src/automation/unit_state.rs:35-47, backend/src/automation/mod.rs:502 and 694, frontend/src/features/test-panel/OperatorPanel.tsx
- Description: `UnitTaskState` has an `accepted` structure and print readiness treats accepted failures as print-ready, but there is no backend command or frontend action that sets `accepted.accepted = true`. As written, failed/warning tasks must effectively be rerun to pass unless someone edits `unit_state.json` outside the app.
- Suggestion: Decide the intended policy. If explicit operator acceptance is required, add a command with operator name/reason, audit entry, and UI affordance in the failure notice. If acceptance is not intended for the pilot, remove or hide the path from readiness wording so operators are not told about an unavailable action.
- Status: open

### Issue 16 -- Severity: suggestion
- File: backend/src/automation/reports.rs, backend/src/automation/mod.rs, backend/src/automation/processors.rs, backend/src/automation/mapped.rs, backend/src/config/profile.rs
- Description: Originally, template filenames and workbook discovery were still hardcoded in Rust after the template root became profile-driven. This has been fixed with `ReportFileConfig`: report setup uses `templates.main_report_template` and `templates.print_report_template`; discovery and require paths use `workbooks.main.file_pattern` and `workbooks.print.file_pattern`; built-in processors receive the report config; mapped workbook lookup now consistently uses profile workbook patterns; and profile validation requires `main` and `print` workbook definitions.
- Suggestion: Treat the original filename/discovery migration as resolved. Remaining hardening: `SN##` serial substitution is still an implicit convention in `ReportFileConfig::main_report_name`, wildcard conversion is duplicated with `mapped.rs`, invalid/generated-impossible patterns quietly produce no discovery results, and there is not yet a full `setup_unit_folder` + `process_task` integration test using a modified profile JSON with non-default template names/patterns.
- Status: resolved on 2026-06-25, with serial placeholder and pattern-helper cleanup as future hardening

## Testing & Validation Observations

- Strong coverage of CSV error cases (blank, malformed, missing column) in `backend/tests/csv_parsing.rs` and fixtures/. The tests assert that failures produce "fail" + code 1 with the exact error text and no success path.
- Report write fidelity tests (`report_writes.rs`) check inlineStr for leading-zero SNs and operator names, no E+ scientific notation, workbook reloads cleanly.
- Unit folder / candidate / discovery tests exist.
- Schema validation script and `validate:report-layouts` are good.
- New `unit_state` tests cover corrupt sidecars in scan, print readiness, and process paths, plus concurrent state writes preserving all task updates.
- New layout-profile tests cover invalid JSON, validation failures, stable error-code mapping, and preservation of built-in processor fallback when a valid profile task intentionally has no mappings.
- New template-root tests cover setup using a supplied template root and unit-candidate roots following the profile template root parent.
- New report file config tests cover custom template filenames and custom workbook discovery patterns; profile tests now require `main` and `print` workbook definitions.
- Gaps: no automated test that a freshly bundled installer can load its packaged layout JSON without C:/PDU500 or cwd layout. Limited frontend coverage of the full runner + state machine (mostly setup/scroll/updater tests). No stress test for concurrent process + scan + print at the command/UI orchestration level.

## Packaging, Bundling, Updater, Installer Notes

- Resources declaration is present but unused by the current Rust loaders at runtime (see Issue 4). Built-in `include_str!` defaults reduce install-time failure risk but bypass the packaged resource files.
- Updater is readiness-based and uses GitHub latest.json (correct per AGENTS).
- NSIS currentUser installer is the right choice.
- Version consistency script exists (`scripts/release/check-version-consistency.mjs`).
- Risk: if packaged layout JSONs are not resolved after install, the app uses compile-time include defaults unless an external `C:/PDU500/config/...` file exists. Invalid external layout profiles now fail visibly instead of silently falling back.

## Other Positive Details

- Error types are rich (`AutomationCommandError`, `ReportError`, `CsvDataError`) and mapped to user-facing codes/messages (`workbook_locked`, `blank_transformer_sn`, etc.).
- Idempotent re-process short-circuit using fingerprints is well implemented in both mapped and processor paths.
- Transformer SN and final operator name both use `CellValue::Text` + inlineStr path.
- `validate_ready_for_print` is called both before writing operator name and before opening the dialog.
- Docs are accurate and useful (LEGACY_BEHAVIOR.md, ARCHITECTURE.md, CONFIGURATION_MODEL.md).

## Recommendations (priority order)
1. Implement proper Tauri resource resolution for layouts (Issue 4) and test in a packaged build.
2. Split OperatorPanel.tsx and drive more of the panel from the profile (Issues 6, 7).
3. Continue migrating remaining tasks to mappings + remove duplication.
4. Decide and implement the acceptance override policy (Issue 15).
5. Add a "packaged layout load" test.
6. Later hardening: cache profile/config loads (Issue 8), stale-lock recovery for `unit_state.lock` and workbook locks, explicit serial placeholder config, shared wildcard matching helper, and an end-to-end invalid-profile command test.

This review has been updated after the `unit_state` error-handling/locking fixes, layout-profile failure-handling fixes, profile-driven template-root fix, and profile-driven report filename/discovery fix landed.

## Files Referenced (selected)
- backend/src/automation/{mod.rs, reports.rs, processors.rs, mapped.rs, csv_data.rs, tasks.rs, unit_state.rs, unit_candidates.rs}
- backend/src/{commands.rs, config/{mod.rs, profile.rs, accuracy.rs}, lib.rs}
- frontend/src/features/test-panel/{OperatorPanel.tsx (~2500 LOC), taskModel.ts, types.ts, backend.ts}
- config/report-layouts/{pdu500.rev02.layout.json, pdu500.accuracy-thresholds.json}
- shared/schemas/report-layout.schema.json
- backend/tauri.conf.json, backend/tests/*.rs, docs/*.md, AGENTS.md
