# Scripts

Build, validation, smoke-test, and release helper scripts.

## Root Tooling

The root `package.json` is the project task runner. It orchestrates frontend dev/build, desktop packaging, lint, and test commands. Dependencies and Bun lockfile live at the repo root; `scripts/run-bun.mjs` resolves the Bun binary the same way as `TE_Component_Inventory`.

This layout is intentional for the current pilot. Moving `package.json` into `frontend/` requires a separate migration plan (workspace layout, CI commands, Tauri build paths).

## Current Scripts

| Script | Purpose |
| ------ | ------- |
| `run-bun.mjs` | Bun runner helper; use via `node scripts/run-bun.mjs <args>` |
| `validate-local.mjs` | Runs the full local validation sequence used before cleanup/release work |
| `fixtures/validate-report-layout-schema.mjs` | Validates report-layout JSON files against `shared/schemas/report-layout.schema.json` |
| `release/check-version-consistency.mjs` | Confirms root package, Cargo, and Tauri versions match |

## Planned Categories

Subfolders are created only when the first real script is added. Do not create empty placeholder directories.

```text
scripts/
  build/      # Frontend/desktop build helpers
  release/    # Version checks, checksums, S-drive staging
  fixtures/   # Fixture copy/setup helpers
  run-bun.mjs
  README.md
```

Near-term scripts (not yet added):

- release artifact checksum helper
- S-drive staging helper
- fixture copy/setup helper

## Full Local Validation Checklist

Run these before a structure-cleanup PR or release prep. All commands run from the repo root.

Preferred single command:

```powershell
node scripts\run-bun.mjs run validate
```

Expanded sequence:

```powershell
node scripts\run-bun.mjs run lint
node scripts\run-bun.mjs run test
node scripts\run-bun.mjs run build
node scripts\run-bun.mjs run check:versions
node scripts\run-bun.mjs run validate:report-layouts
cargo fmt --manifest-path backend\Cargo.toml --check
cargo check --manifest-path backend\Cargo.toml
cargo test --manifest-path backend\Cargo.toml
```

Phase-specific subsets:

| Phase / scope | Commands |
| ------------- | -------- |
| Frontend only | `lint`, `test`, `build:frontend` |
| Backend only | `cargo fmt --check`, `cargo check`, `cargo test` |
| Desktop packaging | `build:desktop` (requires signing secrets; not for CI) |

## CI Direction

`.github/workflows/ci.yml` runs on pushes and pull requests using `windows-latest`.

CI scope:

- install Bun dependencies with the frozen lockfile
- install stable Rust with `rustfmt`
- run `node scripts\run-bun.mjs run validate`

CI will not initially build signed installers, publish releases, access S-drive paths, or require private updater keys.

## Script Rules

- Keep scripts simple and auditable.
- Do not hide signing secrets in scripts.
- Do not commit generated installers, signatures, checksums, `latest.json`, logs, or private keys.
- Scripts should print what they are doing and fail clearly.

Generated build and release artifacts should not be committed.
