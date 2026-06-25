# PDU Data Automation App — Overview

**Current release**: v0.2.9 (pilot)  
**Repository**: https://github.com/Hassaan-ECE/PDU_Data_Automation_App  
**Status**: Suitable for continued production pilot use alongside the legacy app.

This is the Tauri 2 + React + Rust replacement for the legacy Python/PyQt PDU data automation scripts. It preserves the exact operator workflow and panel layout while moving file handling, CSV logic, and report writing into a maintainable, well-tested backend and making report mappings data-driven.

## Goals (from AGENTS.md)

- Keep the familiar test-panel experience (large timer, 208V/415V sections, expandable breakers, color states, manual rerun, report opening).
- Make Excel report mappings editable via `config/report-layouts/` instead of code.
- Use the same modern stack as the sibling `TE_Component_Inventory` project.
- Preserve working legacy behavior unless a correction is explicitly documented.
- Ship as a single current-user NSIS installer with signed GitHub updater support.
- Never commit generated installers, signatures, or secrets.

## Technology Stack

| Area            | Choice                                      |
|-----------------|---------------------------------------------|
| Desktop         | Tauri 2                                     |
| Frontend        | React 19, TypeScript, Vite, Tailwind CSS v4 |
| Package manager | Bun (direct `bun run` commands; no run-bun helper) |
| Backend         | Rust (csv, zip for OpenXML, serde, etc.)    |
| Installer       | Tauri NSIS current-user                     |
| Updates         | Signed Tauri updater + `latest.json`        |
| Config          | JSON + JSON Schema                          |

## Current Features (v0.2.9)

**Operator Workflow (preserved)**
- Browse unit folder (native dialog).
- Inline Transformer SN entry (saved on Start/blur/Enter). Written as text to `Test Summary!D1`.
- Large remaining-time display + current step.
- All legacy sections + expandable breakers + color states.
- Manual rerun of any task.
- Explicit "Follow Step" / "Current Step" with auto-scroll (disabled on manual scroll or collapse).
- Backlog prompt for previously detected CSVs.
- Mid-test resume using CSV timestamps.
- Three-stage reset.

**Print Report Flow**
- Validates all tasks are pass/accepted + Transformer SN present.
- Prompts for (or selects from local list of) final operator name.
- Writes name to `Test Report #2!E39`.
- Opens Excel's native print dialog.

**Processing & Safety**
- 65 tasks.
- Strict CSV parsing (no silent zeros). Fixtures test blank, malformed, and missing-column cases.
- Waits for stable files (size + mtime) before processing.
- Fingerprinting + `unit_state.json` for idempotency and restart resilience.
- Accuracy verification (thresholds from config) runs **before** any Excel write for system/breaker tasks.
- `validate_ready_for_print` gate before final operator name or print dialog.
- Transactional workbook patching with lock + backup + rollback.

**Data-Driven Direction**
- Transformer (208V + 415V) writes are fully driven by mappings in `pdu500.rev02.layout.json`.
- System, breaker, and burn-in still use built-in Rust processors (generic mapped path is ready).
- Accuracy thresholds are externalized and hot-reloadable.

**Release**
- Single current-user signed installer.
- Version kept consistent across `package.json`, `backend/Cargo.toml`, `backend/tauri.conf.json`.
- Artifacts staged on S-drive + published to GitHub Releases with `latest.json`.

## Development Commands

```powershell
bun install
bun run desktop          # Tauri desktop dev
bun run dev:frontend
bun run build
bun run test
bun run lint
bun run validate         # recommended before releases / PRs
```

## Validation & Quality

- Backend: ~64 tests covering CSV edges, Excel fidelity (styles, formulas, merged cells, text preservation), profile validation, accuracy, state, discovery.
- Frontend: setup, scroll/follow, updater, print flows.
- Full local validation: `bun run validate` (lint + test + build + version check + schema + Rust checks + cargo test). Prefer `bun run desktop` for development.
- The old `scripts/run-bun.mjs` helper has been removed; all commands now use `bun` (or `bun run`) directly.
- CI runs the same on Windows.
- Excel workbooks must open without repair prompts.

## Remaining Work (High Priority)

- Process and cell-by-cell compare several more real production units against legacy output.
- Complete real updater upgrade testing from an installed older build.
- Continue migrating system/breaker/burn-in logic into the layout profile.
- Expand scrubbed representative fixtures.
- Add more production validation before retiring the legacy app.

See `docs/LEGACY_BEHAVIOR.md` for constraints that must be respected.
See `docs/ARCHITECTURE.md` and `docs/CONFIGURATION_MODEL.md` for deeper technical detail.
