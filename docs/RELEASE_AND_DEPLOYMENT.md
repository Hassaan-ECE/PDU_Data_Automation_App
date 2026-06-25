# Release and Deployment

## Goals
- One current-user Windows installer.
- Easy to find on the S-drive.
- Signed updater metadata via GitHub Releases.
- Generated artifacts and secrets never committed.

## Current Practice (v0.2.9)

**Version source of truth** (must be identical):
- `package.json`
- `backend/Cargo.toml`
- `backend/tauri.conf.json`

**GitHub Release assets** (per version):
- `PDU Data Automation_<ver>_x64-setup.exe`
- `.sig` (updater signature)
- `latest.json`
- `SHA256SUMS.txt`
- Release notes

Tauri updater URL resolves through:
`https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases/latest/download/latest.json`

**S-drive layout** (example):
```
S:\Engineering\Public\Syed_Hassaan_Shah\PDU_Data_Automation\
├── PDU Data Automation_0.2.9_x64-setup.exe
├── release-support\
│   └── v0.2.9\
│       ├── latest.json
│       ├── ... .sig
│       └── SHA256SUMS.txt
├── archive\
└── shared\
```

Keep the S-drive root clean: current installer at top level, versioned support files under `release-support/`, older installers in `archive/`.

## Signing

- PDU-specific Tauri updater key lives outside the repo (typically `%USERPROFILE%\.tauri\...`).
- Private key and password are set only for the `build:desktop` step and immediately cleared.
- Never commit the key or password.

## Build & Validation Checklist (before release)

```powershell
bun install
bun run validate          # full lint/test/build + Rust + schema + versions
bun run build:desktop     # with signing env vars (requires private key env)
```

Note: The `validate` script internally runs some `.mjs` helpers via the Node compatibility in Bun. The old `node scripts/run-bun.mjs` wrapper is no longer used.

After build:
- Verify the installer launches cleanly on a test profile.
- Smoke a real or safe unit folder end-to-end (Transformer SN, processing, print report).
- Confirm generated workbook opens in Excel with no repair prompt.
- Confirm `latest.json` points at the new version.
- Copy installer + support files to S-drive.

## Local `release/` Folder

Contains only human notes. All generated `.exe`, `.sig`, `.json`, and checksums are git-ignored.
