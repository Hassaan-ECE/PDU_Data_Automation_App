# Scripts

Validation, release helpers, and local checks.

All normal usage is with direct `bun` commands (bun install, bun run desktop, bun run build, etc.).

## Root Tooling

The root `package.json` is the project task runner. It orchestrates frontend dev/build, desktop packaging, lint, and test commands. Dependencies and Bun lockfile live at the repo root.

Use `bun install` and `bun run <script>` directly (e.g. `bun run desktop`, `bun run build`).

This layout is intentional for the current pilot. Moving `package.json` into `frontend/` requires a separate migration plan (workspace layout, CI commands, Tauri build paths).

## Current Scripts

| Script | Purpose |
| ------ | ------- |
| `validate-local.mjs` | Runs the full local validation sequence used before cleanup/release work |
| `fixtures/validate-report-layout-schema.mjs` | Validates report-layout JSON files against `shared/schemas/report-layout.schema.json` |
| `release/check-version-consistency.mjs` | Confirms root package, Cargo, and Tauri versions match |

## Planned Categories

Subfolders are created only when the first real script is added. Do not create empty placeholder directories.

```text
scripts/
  build/      # Frontend/desktop build helpers (future)
  release/    # Version checks, checksums, S-drive staging
  fixtures/   # Fixture copy/setup helpers
  README.md
```

Near-term scripts (not yet added):

- release artifact checksum helper
- S-drive staging helper
- fixture copy/setup helper

## Full Local Validation Checklist

Run these before a structure-cleanup PR or release prep. All commands run from the repo root using Bun.

Preferred single command:

```powershell
bun run validate
```

Expanded sequence:

```powershell
bun run lint
bun run test
bun run build
bun run check:versions
bun run validate:report-layouts
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

- `bun install --frozen-lockfile`
- stable Rust + rustfmt
- `bun run validate` (and the other `bun run` commands)

CI will not initially build signed installers, publish releases, access S-drive paths, or require private updater keys.

## Script Rules

- Keep scripts simple and auditable.
- Do not hide signing secrets in scripts.
- Do not commit generated installers, signatures, checksums, `latest.json`, logs, or private keys.
- Scripts should print what they are doing and fail clearly.

Generated build and release artifacts should not be committed.
