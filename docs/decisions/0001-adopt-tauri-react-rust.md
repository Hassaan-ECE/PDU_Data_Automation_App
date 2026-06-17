# Decision 0001: Adopt Tauri, React, TypeScript, Bun, And Rust

## Status

Proposed.

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

The user wants the PDU replacement to follow that style, keep a GitHub repo, stage releases on the S-drive, and ship as a single installer.

## Decision

Use the `TE_Component_Inventory` stack pattern for the PDU rebuild.

The app should use:

- React/TypeScript frontend for the operator panel.
- Rust/Tauri backend for file scanning, CSV parsing, report writing, and native commands.
- Bun for frontend/package scripts.
- Tauri NSIS for the Windows installer.
- Signed Tauri updater with GitHub Release `latest.json`.

## Consequences

Benefits:

- cleaner UI structure than the PyQt script
- native installer and updater path
- better testability for frontend and backend logic
- easier future UI refinement
- source-control-friendly release flow

Costs and risks:

- initial rebuild is larger than refactoring the Python scripts
- Rust Excel template editing must be validated early
- the team must maintain frontend and backend dependencies
- report mapping config needs careful validation to avoid moving errors from code to config

## Follow-Up

Create a technical spike for Excel template preservation before implementing full report writing.
