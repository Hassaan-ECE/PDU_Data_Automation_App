# Release And Deployment Plan

This document defines the intended release flow for the rebuilt PDU Data Automation App.

## Goals

- Build one current-user Windows installer.
- Keep the current installer easy to find on the S-drive.
- Keep release support files organized.
- Publish signed updater metadata through GitHub Releases.
- Avoid committing secrets or generated release artifacts.

## App Identity

| Field | Value |
| --- | --- |
| Product name | `PDU Data Automation` |
| Repository | `https://github.com/Hassaan-ECE/PDU_Data_Automation_App` |
| Tauri identifier | `com.te.lab.pdu-data-automation` |
| First released version | `0.1.0` |
| Installer type | Tauri NSIS, current-user install |
| Updater | Signed Tauri updater |

Version should stay synchronized across:

- `package.json`
- `backend/Cargo.toml`
- `backend/tauri.conf.json`

## S-Drive Layout

Current release root:

```text
S:\Engineering\Public\Syed_Hassaan_Shah\PDU_Data_Automation\
  PDU Data Automation_0.2.9_x64-setup.exe
  release-support\
    v0.2.9\
      latest.json
      PDU Data Automation_0.2.9_x64-setup.exe.sig
      SHA256SUMS.txt
  archive\
  shared\
    reserved-for-future-runtime-data\
```

Keep the S-drive root clean:

- one obvious current installer at the root
- updater metadata under `release-support\`
- old installers under `archive\`
- no source files at the release root

## GitHub Release Assets

Each release should publish:

- NSIS installer `.exe`
- updater signature `.sig`
- `latest.json`
- `SHA256SUMS.txt`
- release notes

`v0.2.9` has been published with these assets. Keep this list as the checklist for future releases.

The Tauri updater endpoint should point at:

```text
https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases/latest/download/latest.json
```

This URL should be updated if the GitHub owner or repo name changes.

## Signing Key

Generate a PDU-specific Tauri updater key outside the repo.

Suggested private key path:

```text
%USERPROFILE%\.tauri\pdu-data-automation-updater.key
```

Rules:

- never commit the private key
- never commit the signing password
- document the public key in `backend/tauri.conf.json`
- rotate the key before broad release if the development key is exposed

## Release Checklist

Before release:

```powershell
node scripts\run-bun.mjs run lint
node scripts\run-bun.mjs run test
node scripts\run-bun.mjs run build

Push-Location backend
cargo fmt -- --check
cargo check
cargo test
Pop-Location
```

When available:

```powershell
Push-Location backend
cargo clippy --all-targets -- -D warnings
cargo audit
Pop-Location
```

Build signed installer:

```powershell
$env:PATH = "$env:USERPROFILE\.bun\bin;$env:PATH"
$env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -LiteralPath "$env:USERPROFILE\.tauri\pdu-data-automation-updater.key" -Raw).Trim()
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
node scripts\run-bun.mjs run build:desktop
Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY
Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

Known `v0.2.6` build caveat:

- Tauri hit an NSIS file-lock error after generating the installer.
- The generated installer was manually signed, then `latest.json` and `SHA256SUMS.txt` were regenerated.
- The final GitHub Release and S-drive artifacts were verified.
- If this happens again, do not treat the release as complete until the manually signed installer, updater signature, `latest.json`, checksums, GitHub assets, S-drive installer, and updater endpoint have all been checked.

## Manual Smoke

For each release candidate:

- install on a clean user profile if possible
- launch the app
- confirm product name and version
- select a safe unit folder
- confirm inline unit selection shows only the selected unit SN and does not auto-select the latest detected folder
- enter a Transformer SN before Start and confirm it writes to `Test Summary!D1`
- edit Transformer SN after setup and confirm the late save updates `Test Summary!D1`
- confirm numeric-looking Transformer SN values such as `000123` remain text
- confirm report opening is blocked while Transformer SN is missing or unsaved
- confirm Print Report blocks while the runner is active, while report setup is missing, and while Transformer SN is missing or unsaved
- confirm Print Report writes the selected operator name to `Test Report #2!E39`
- confirm Print Report opens Excel's non-silent print UI with all visible print-report worksheets selected
- confirm a locked workbook shows a visible Transformer SN save/setup error and can be retried
- confirm CSV detection
- confirm current-step follow controls behave correctly while running, paused, manually scrolled, and after expand/collapse
- process at least one transformer check fixture
- process at least one system test fixture
- process at least one breaker fixture
- verify logs and error display
- open the generated report in Excel
- confirm Excel does not show repair warnings
- close and relaunch the app
- run updater check against release metadata
- uninstall cleanly

The installed `v0.1.0` app processed one known-good unit and produced an Excel workbook that opened without repair prompts. The `v0.2.6` release was smoke-tested with `C:\PDU500\262343000072`, and the generated data was manually reviewed as good. The `v0.2.8` release added the revised inline unit/Transformer SN UX, current-step follow controls, and readiness-based updater timing. The `v0.2.9` release adds the manual Print Report flow and final operator-name capture. `v0.1.0` and `v0.2.0` did not grant updater plugin permissions, so use `v0.2.1` or newer as the baseline for future updater smoke tests.

## Local Release Folder

The repo's `release/` folder is for notes and staging scripts only. Generated `.exe`, `.sig`, `.json`, and checksum files are ignored by Git.
