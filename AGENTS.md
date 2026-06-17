# AGENTS.md

## Project Role

This repository is the planned replacement for the legacy PDU data automation script bundle at:

```text
C:\Projects\Active\PDU_Data_Automation
```

The goal is to preserve the current operator layout and workflow while rebuilding the app with the same general stack used by `TE_Component_Inventory`: Tauri 2, React, TypeScript, Vite, Tailwind CSS, Bun, Rust, GitHub Releases, signed updater support, and a current-user Windows installer.

## Current Status

This repository is currently a planning scaffold. Do not present it as a runnable app until the Tauri frontend/backend files and build tooling are actually added and validated.

## High-Priority Constraints

- Preserve the working legacy app behavior unless a behavior change is explicitly documented.
- Keep the test-panel layout familiar: unit folder selection, large timer/status area, sections for 208V, 415V, burn-in, expandable breaker groups, color-coded task states, manual rerun controls, and report opening.
- Make Excel report mappings data-driven. Prefer editable config files under `config/report-layouts/` over hardcoded cell maps in source code.
- Keep release/update behavior compatible with a single installer users can get from the S-drive and update through GitHub Release metadata.
- Keep generated installers, updater signatures, checksums, build output, and runtime logs out of source control.
- Never commit private updater keys or signing passwords.

## Preferred Stack

- Frontend: React, TypeScript, Vite, Tailwind CSS, lucide-react.
- Desktop shell: Tauri 2.
- Backend: Rust commands invoked from Tauri.
- Package manager: Bun via the `scripts/run-bun.mjs` helper pattern from `TE_Component_Inventory`.
- Installer: Tauri NSIS current-user installer.
- Updates: signed Tauri updater with `latest.json` hosted through GitHub Releases.

## Architecture Direction

The replacement app should separate these concerns:

- UI state and operator workflow in the frontend.
- File scanning, CSV parsing, report writing, and release/runtime path logic in the Rust backend.
- Test definitions, CSV source columns, Excel sheet names, and target cells in versioned layout config files.
- Legacy behavior notes and migration decisions in `docs/`.

## Validation Expectations

For meaningful changes:

- Add or update tests when practical.
- Validate report-layout config parsing with sample fixtures before touching real workbooks.
- Do not claim installer, updater, or report-writing behavior works unless it was actually built and tested.
- Keep a migration checklist updated as legacy scripts are replaced.

## Important Legacy Risks To Carry Forward

- The legacy GUI currently treats processor exit code `2` as `detected`, then converts that to `pass`; this must not be copied.
- The legacy app has a STEP71/STEP72 system burn-in mismatch that must be resolved with real instrument data.
- Missing or unparsable CSV values must not silently become valid-looking zeroes.
- Excel template preservation is a technical spike. Confirm that the chosen Rust Excel writer preserves the required workbook formatting, formulas, merged cells, and sheets before committing to it.
