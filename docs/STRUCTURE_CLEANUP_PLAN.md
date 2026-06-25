# Structure Cleanup Plan

This file is now the index for the structure-cleanup work.

The old mixed roadmap has been split so completed work and remaining work do not drift together.

## Status Files

- Completed and validated work: [`STRUCTURE_CLEANUP_DONE.md`](STRUCTURE_CLEANUP_DONE.md)
- Remaining deferred work: [`STRUCTURE_CLEANUP_REMAINING.md`](STRUCTURE_CLEANUP_REMAINING.md)
- Current repository structure reference: [`PROJECT_STRUCTURE.md`](PROJECT_STRUCTURE.md)

## Current Decision

The safe cleanup pass is complete.

Do not start the optional `backend/src-tauri/` migration or release automation work from this
index alone. Those items are still deferred and require the prerequisites listed in
[`STRUCTURE_CLEANUP_REMAINING.md`](STRUCTURE_CLEANUP_REMAINING.md).

## Guardrails

- Preserve operator-facing behavior unless a behavior change is explicitly requested.
- Keep `backend/` as the Tauri crate root unless a dedicated packaging migration proves otherwise.
- Keep generated installers, signatures, checksums, updater metadata, build output, logs, and secrets out of source control.
- Keep release, updater, installer, and signing behavior unchanged unless explicitly assigned and validated.
- Add fixtures, scripts, and folders only when they have real content and clear ownership.

## Full Local Validation

Run this before merging structure-cleanup work:

```powershell
node scripts\run-bun.mjs run validate
git diff --check
```

`build:desktop` remains a separate packaging validation step and is not part of normal source
validation because it can require signing/release environment setup.
