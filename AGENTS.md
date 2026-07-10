# AGENTS.md

## Project Role

This repository is the pilot replacement for the legacy PDU data automation script bundle at:

```text
C:\Projects\Active\PDU_Data_Automation
```

The goal is to preserve the current operator layout and workflow while rebuilding the app with the same general stack used by `TE_Component_Inventory`: Tauri 2, React, TypeScript, Vite, Tailwind CSS, Bun, Rust, GitHub Releases, signed updater support, and a current-user Windows installer.

## Current Status

This repository has a `v0.2.12` pilot release of the replacement app. The released build includes the Tauri 2 / React / TypeScript / Vite / Tailwind / Bun / Rust stack, the production PDU500 Rev02 layout profile, CSV detection/parsing, Excel workbook patching, inline unit selection, inline Transformer SN setup/save support, manual Print Report support with final operator-name capture, explicit current-step follow controls, readiness-based updater scheduling, a generic data-driven mapping path for transformer report writes, built-in Rust processor fallback for the remaining current workflow, bundled layout resource loading, setup-on-folder-selection behavior, in-app Teams operator notifications (Problem/Complete, password-gated settings, optional shared OneDrive shift log), a signed current-user NSIS installer, GitHub Release updater artifacts, and S-drive staging.

Treat it as pilot-ready, not fully cut over. Keep the legacy Python app available until several production units have been processed cleanly, generated reports have been compared against legacy output, and the updater upgrade path has been tested with a newer release.

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
- Package manager: Bun (use `bun run desktop`, `bun run dev:frontend`, `bun run build`, etc. directly).
- Installer: Tauri NSIS current-user installer.
- Updates: signed Tauri updater with `latest.json` hosted through GitHub Releases.

## Architecture Direction

The replacement app should separate these concerns:

- UI state and operator workflow in the frontend.
- File scanning, CSV parsing, report writing, and release/runtime path logic in the Rust backend.
- Test definitions, CSV source columns, Excel sheet names, and target cells in versioned layout config files.
- Legacy behavior notes and migration decisions in `docs/LEGACY_BEHAVIOR.md` and `docs/decisions/`.

## Validation Expectations

For meaningful changes:

- Add or update tests when practical.
- Validate report-layout config parsing with sample fixtures before touching real workbooks.
- Do not claim installer, updater, or report-writing behavior works unless it was actually built and tested.
- Keep a migration checklist updated as legacy scripts are replaced.

## Release / Updater Notes For Agents

- Read `docs/RELEASE_AND_DEPLOYMENT.md` before doing release work.
- Current released pilot is `v0.2.12`; the updater signing key was rotated in `v0.2.10`. Older installs should be updated manually with the current S-drive installer before relying on updater flow; future updater releases should be signed with the same replacement key.
- The updater private key and local DPAPI passphrase helper live outside the repo under `%USERPROFILE%\.tauri\`. Never print, paste, commit, or upload either secret.
- GitHub updater assets use dot-normalized installer names, for example `PDU.Data.Automation_0.2.12_x64-setup.exe`. The S-drive operator-facing installer uses the space-name form, for example `PDU Data Automation_0.2.12_x64-setup.exe`.
- A GitHub updater release needs `latest.json` and a matching installer signature. Upload the installer, `.sig`, `latest.json`, and `SHA256SUMS.txt`; the `.exe` alone is only enough for manual installs.
- Keep the S-drive root clean: only the current operator installer should be visible at the root; versioned updater support files belong under `release-support\vX.Y.Z`; superseded files belong under `archive\`.
- If `bun run check:versions` hits the known Bun shim crash, run `bun scripts/release/check-version-consistency.mjs` directly and report that substitution.

## Important Legacy Risks To Carry Forward

- The legacy GUI currently treats processor exit code `2` as `detected`, then converts that to `pass`; this must not be copied.
- System burn-in uses two related steps: STEP71 is the long burn-in/soak period, and STEP72 is the quick burn-in data capture used for report values.
- `C:\Projects\Active\Data Automation Upgraded` is the stronger reference for 208V/415V system and breaker verification logic because it adds Python-side accuracy calculations and pass/fail checks.
- Missing or unparsable CSV values must not silently become valid-looking zeroes.
- Excel template preservation must continue to be checked whenever workbook templates or report-writing logic change. Generated workbooks should open in Excel without repair prompts and preserve required formatting, formulas, merged cells, and sheets.
