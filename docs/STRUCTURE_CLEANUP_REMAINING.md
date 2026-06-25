# Structure Cleanup Remaining

This file tracks structure-cleanup items that were intentionally skipped or deferred.

Nothing here should be treated as approval for a broad restructure. Each item should be handled as
a small, separately validated change.

## Deferred Small Steps

### Add `fixtures/expected/`

Status: skipped.

Waiting on:

- Snapshot-style tests that need expected output files.
- A stable expected-output format that will not churn on every workbook implementation change.

Do not add expected snapshots until they are used by tests.

### Add `scripts/build/`

Status: skipped.

Waiting on:

- A real build helper that does more than duplicate existing package scripts.
- A clear reason to wrap or compose frontend/desktop build commands.

The current root package scripts remain enough for source validation.

### Add Release Checksum Helper

Status: skipped.

Waiting on:

- Confirmed release artifact naming.
- Confirmed release output directory.
- Decision on checksum format and whether checksums are local-only or published.
- A release workflow that proves generated checksum files stay ignored unless intentionally documented.

### Add S-Drive Staging Helper

Status: skipped.

Waiting on:

- Confirmed S-drive destination path.
- Access/permission check from the release machine.
- Decision on dry-run behavior.
- Decision on whether staging should copy only the installer or also release notes/checksums.
- Validation that the helper never writes secrets or generated release artifacts into source control.

### Add Fixture Copy/Setup Helper

Status: skipped.

Waiting on:

- More than one fixture setup path.
- A repeated manual fixture preparation step that is worth scripting.
- A clear rule for temp output location, likely under `.tmp/`.

Current tests already copy fixtures to temp dirs directly.

### Add `backend/tests/layout_profiles.rs`

Status: skipped.

Waiting on:

- A reason to test layout profiles through integration tests instead of existing config unit tests and JSON Schema validation.
- Additional layout profile fixtures or profile variants.

Current coverage includes config unit tests and `validate:report-layouts`.

### Add Reliable Locked-Workbook Integration Test

Status: skipped.

Waiting on:

- A reliable Windows-only way to simulate Excel-style workbook locking in CI or a clearly marked local-only test.
- Agreement on whether this belongs in CI or remains a manual/operator-PC validation.

Current error mapping for locked workbooks is covered by backend tests.

### Generate IPC Types Into `shared/generated/`

Status: skipped.

Waiting on:

- A chosen source of truth for IPC DTOs, preferably Rust structs or JSON Schema.
- A deterministic generator.
- A validation step that fails when generated types are stale.
- Enough IPC surface growth or drift risk to justify the added build complexity.

Current IPC ownership remains in `frontend/src/integrations/tauri/backend.ts`.

### Move Root `package.json` Into `frontend/`

Status: skipped.

Waiting on:

- A dedicated workspace migration plan.
- Updated Bun install behavior.
- Updated CI commands.
- Updated Tauri dev/build commands.
- Validation that desktop packaging still resolves frontend output correctly.

Root `package.json` remains the project task runner.

### Move Frontend Config Files

Status: skipped.

Waiting on:

- A frontend package/workspace migration.
- Proof that Vite, TypeScript, ESLint, Vitest, and Tauri build paths still work.

There is no current need to move these files independently.

### Build Signed Desktop Installer In CI

Status: skipped.

Waiting on:

- Secure signing secret configuration.
- Decision on whether CI is allowed to produce signed release artifacts.
- Artifact retention and publishing policy.
- Validation that generated installers/signatures/updater metadata stay out of source control.

Normal CI intentionally runs source validation only.

### Publish Releases From CI

Status: skipped.

Waiting on:

- Confirmed GitHub release permissions.
- Release approval process.
- Updater metadata signing workflow.
- S-drive staging decision.
- Rollback process.

Release automation should stay separate until signing and publishing are fully understood.

### Access S-Drive From CI

Status: skipped.

Waiting on:

- Confirmation that the CI runner can safely access the S-drive.
- Credential and network-path policy.
- A staging design that cannot publish partial or unapproved artifacts.

For now, S-drive actions remain manual or local release-machine work.

## Dedicated Migration: `backend/src-tauri/`

Status: deferred.

This is not part of the completed cleanup. It should only be done if there is a concrete packaging,
tooling, or long-term maintenance reason.

Waiting on:

- A dedicated branch or PR that contains only this migration.
- A known-good baseline where current desktop dev and installer build work before the move.
- Confirmation of Tauri CLI path assumptions.
- Updated `tauri.conf.json` paths if needed.
- Updated icon, capability, build script, package script, release script, and documentation paths if needed.
- `node scripts\run-bun.mjs run desktop` validation.
- `node scripts\run-bun.mjs run build` validation.
- `cargo test --manifest-path backend\Cargo.toml` validation.
- `node scripts\run-bun.mjs run build:desktop` validation.
- Generated NSIS installer launch validation.
- Updater metadata generation validation.

Do not merge this migration unless packaging is validated. Source checks alone are not enough.

## Reconsider Bigger Cleanup When

Revisit broader restructuring only when one or more of these are true:

- `v0.3.0` or another stable pilot milestone has shipped.
- Operator feedback has stabilized.
- A second major product workflow is added beyond the current test-panel workflow.
- The team wants CI to build signed installers or release artifacts.
- IPC command growth creates real type drift or maintenance pain.
- Packaging or release automation requires a more standard Tauri layout.
