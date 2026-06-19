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
  PDU Data Automation_0.2.4_x64-setup.exe
  release-support\
    v0.2.4\
      latest.json
      PDU Data Automation_0.2.4_x64-setup.exe.sig
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

`v0.2.4` has been published with these assets. Keep this list as the checklist for future releases.

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

## Manual Smoke

For each release candidate:

- install on a clean user profile if possible
- launch the app
- confirm product name and version
- select a safe unit folder
- confirm CSV detection
- process at least one transformer check fixture
- process at least one system test fixture
- process at least one breaker fixture
- verify logs and error display
- open the generated report in Excel
- confirm Excel does not show repair warnings
- close and relaunch the app
- run updater check against release metadata
- uninstall cleanly

The installed `v0.1.0` app has processed one known-good unit and produced an Excel workbook that opened without repair prompts. `v0.1.0` and `v0.2.0` did not grant updater plugin permissions, so use `v0.2.1` or newer as the baseline for future updater smoke tests.

## Local Release Folder

The repo's `release/` folder is for notes and staging scripts only. Generated `.exe`, `.sig`, `.json`, and checksum files are ignored by Git.
