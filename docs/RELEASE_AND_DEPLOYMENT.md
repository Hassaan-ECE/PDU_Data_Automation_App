# Release and Deployment

## Goals
- One current-user Windows installer.
- Easy to find on the S-drive.
- Signed updater metadata via GitHub Releases.
- Generated artifacts and secrets never committed.

## Current Practice (v0.2.11)

**Version source of truth** (must be identical):
- `package.json`
- `backend/Cargo.toml`
- `backend/tauri.conf.json`

**GitHub Release assets** (per version):
- `PDU.Data.Automation_<ver>_x64-setup.exe` (dot-normalized asset name used by `latest.json`)
- `PDU.Data.Automation_<ver>_x64-setup.exe.sig` (updater signature)
- `latest.json`
- `SHA256SUMS.txt`
- Release notes

The S-drive operator installer keeps the space-name form:
`PDU Data Automation_<ver>_x64-setup.exe`

For manual installation the `.exe` alone is enough. For updater support, publish the installer, `.sig`, `latest.json`, and `SHA256SUMS.txt` together.

Tauri updater URL resolves through:
`https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases/latest/download/latest.json`

**S-drive layout**:
```
S:\Engineering\Public\Syed_Hassaan_Shah\PDU_Data_Automation\
├── PDU Data Automation_0.2.11_x64-setup.exe
├── release-support\
│   └── v0.2.11\
│       ├── latest.json
│       ├── ... .sig
│       └── SHA256SUMS.txt
├── archive\
└── shared\
```

Keep the S-drive root clean: current installer at top level, versioned support files under `release-support/`, older installers in `archive/`.

## Signing

- PDU-specific Tauri updater key lives outside the repo (typically `%USERPROFILE%\.tauri\...`).
- The current replacement updater key was generated for `v0.2.10` at `%USERPROFILE%\.tauri\pdu-data-automation-updater.key`.
- The key is passphrase-protected; the local passphrase helper is stored as a Windows DPAPI secret at `%USERPROFILE%\.tauri\pdu-data-automation-updater.key.password.dpapi`.
- Private key and password are set only for the `build:desktop` step and immediately cleared.
- Never commit the key or password.
- Because this key replaced the earlier public key, older installed builds may need a manual install of the current S-drive installer before future updater releases signed with this key are trusted.

## Build & Validation Checklist (before release)

```powershell
bun install
bun run validate          # full lint/test/build + Rust + schema + versions
bun run build:desktop     # with signing env vars (requires private key env)
```

Local signed build helper:

```powershell
$keyPath = Join-Path $env:USERPROFILE ".tauri\pdu-data-automation-updater.key"
$passwordPath = "$keyPath.password.dpapi"
$secure = Get-Content -LiteralPath $passwordPath | ConvertTo-SecureString
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
try {
  $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -LiteralPath $keyPath -Raw).Trim()
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
  bun run build:desktop
} finally {
  if ($bstr -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
  Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
  Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
}
```

Note: The `validate` script internally runs some `.mjs` helpers via the Node compatibility in Bun. The old `node scripts/run-bun.mjs` wrapper is no longer used.

If `bun run check:versions` crashes in Bun's Node compatibility layer, run the script directly:

```powershell
bun scripts/release/check-version-consistency.mjs
```

If the NSIS bundle creates the installer but Tauri fails at the final signing step with Windows `os error 1224` ("user-mapped section open"), wait briefly and sign the produced installer directly:

Use temporary environment variables for the direct signer fallback. Do not pass the password with `--password`; some wrappers echo command-line arguments.

```powershell
$keyPath = Join-Path $env:USERPROFILE ".tauri\pdu-data-automation-updater.key"
$passwordPath = "$keyPath.password.dpapi"
$installer = "backend\target\release\bundle\nsis\PDU Data Automation_<ver>_x64-setup.exe"
$secure = Get-Content -LiteralPath $passwordPath | ConvertTo-SecureString
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
try {
  $env:TAURI_SIGNING_PRIVATE_KEY_PATH = $keyPath
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
  bun tauri signer sign "$installer"
} finally {
  if ($bstr -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
  Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PATH -ErrorAction SilentlyContinue
  Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
}
```

After build:
- Verify the installer launches cleanly on a test profile.
- Smoke a real or safe unit folder end-to-end (Transformer SN, processing, print report).
- Confirm generated workbook opens in Excel with no repair prompt.
- Confirm `latest.json` points at the new version.
- Copy installer + support files to S-drive.

## Release Sequence

1. Update the version in `package.json`, `backend/Cargo.toml`, and `backend/tauri.conf.json`.
2. Update `release/vX.Y.Z.md`.
3. Run validation. At minimum run version consistency, frontend tests/build, Rust tests/checks, and report-layout validation.
4. Build the signed desktop installer.
5. Prepare GitHub assets in `backend\target\release\bundle\nsis\`:
   - dot-named installer: `PDU.Data.Automation_<ver>_x64-setup.exe`
   - dot-named signature: `PDU.Data.Automation_<ver>_x64-setup.exe.sig`
   - `latest.json` whose URL points at the dot-named GitHub asset
   - `SHA256SUMS.txt`
6. Commit the release source changes, tag the commit as `vX.Y.Z`, push `main`, and push the tag.
7. Create the GitHub Release with the four assets above and the release notes:

```powershell
gh release create vX.Y.Z `
  "backend\target\release\bundle\nsis\PDU.Data.Automation_X.Y.Z_x64-setup.exe" `
  "backend\target\release\bundle\nsis\PDU.Data.Automation_X.Y.Z_x64-setup.exe.sig" `
  "backend\target\release\bundle\nsis\latest.json" `
  "backend\target\release\bundle\nsis\SHA256SUMS.txt" `
  --repo Hassaan-ECE/PDU_Data_Automation_App `
  --title "vX.Y.Z" `
  --notes-file "release\vX.Y.Z.md"
```

8. Verify the release and updater endpoint:

```powershell
gh release view vX.Y.Z --repo Hassaan-ECE/PDU_Data_Automation_App
Invoke-WebRequest `
  -Uri "https://github.com/Hassaan-ECE/PDU_Data_Automation_App/releases/latest/download/latest.json" `
  -UseBasicParsing
```

9. Stage the S-drive:
   - Copy `PDU Data Automation_<ver>_x64-setup.exe` to the S-drive root.
   - Copy `latest.json`, `.sig`, and `SHA256SUMS.txt` to `release-support\vX.Y.Z`.
   - Move superseded root installers/support folders to `archive\`; do not leave older versions visible at the root.

## Local `release/` Folder

Contains only human notes. All generated `.exe`, `.sig`, `.json`, and checksums are git-ignored.
