# Decision 0001: Adopt Tauri, React, TypeScript, Bun, And Rust

## Status

Accepted.

## Context

The legacy PDU automation app is a working Python/PyQt script bundle. It is useful but hard to maintain because UI behavior, test sequencing, file scanning, CSV extraction, and Excel cell mappings are spread across several scripts.

The newer `TE_Component_Inventory` project uses a stack that is better suited for a versioned, installable, updateable desktop app:

- Tauri 2
- React
- TypeScript
- Vite
- Tailwind CSS
- Bun
- Rust backend
- NSIS current-user installer
- signed Tauri updater
- GitHub Releases

The goal is to follow that style, keep a GitHub repo, stage releases on the S-drive, and ship as a single installer.

## Decision

Use the `TE_Component_Inventory` stack pattern for the PDU rebuild.

The app uses:

- React/TypeScript frontend for the operator panel.
- Rust/Tauri backend for file scanning, CSV parsing, report writing, and native commands.
- Bun for frontend/package scripts (invoke via `bun run ...` directly).
- Tauri NSIS for the Windows installer.
- Signed Tauri updater with GitHub Release `latest.json`.

## Consequences

Benefits:

- Cleaner UI structure than the PyQt script
- Native installer and updater path
- Better testability for frontend and backend logic
- Easier future UI refinement
- Source-control-friendly release flow

Costs and risks:

- Initial rebuild is larger than refactoring the Python scripts
- Rust Excel template editing must be validated on template changes
- Frontend and backend dependencies must be maintained
- Report mapping config needs careful validation (JSON Schema + tests)

## Follow-Up

Excel template preservation (styles, merged cells, formulas, no repair prompts) is validated on every relevant change via unit tests and manual review of generated workbooks.
