# Documentation

This folder contains the current, useful project documentation.

## Start Here

- [OVERVIEW.md](./OVERVIEW.md) — High-level status, stack, features in the current pilot (v0.2.9), and remaining work.
- [ARCHITECTURE.md](./ARCHITECTURE.md) — Current responsibilities, runtime shape, data flow, Excel patching approach, and state model.
- [LEGACY_BEHAVIOR.md](./LEGACY_BEHAVIOR.md) — What must be preserved from the original app and what must be corrected. Critical constraints.
- [CONFIGURATION_MODEL.md](./CONFIGURATION_MODEL.md) — How report layouts, CSV mappings, and thresholds are driven by config files.
- [NOTIFICATIONS.md](./NOTIFICATIONS.md) — Password-gated in-app Teams settings, Adaptive Card event scope, runtime status, security limits, optional shift log, and validation.
- [OPERATOR_NOTIFICATION_CATALOG.md](./OPERATOR_NOTIFICATION_CATALOG.md) — Operator-facing list of every Teams notification (active + not yet), when it fires, and how to practice on a station.

## Supporting References

- [PROJECT_STRUCTURE.md](./PROJECT_STRUCTURE.md) — Current repository layout and ownership.
- [RELEASE_AND_DEPLOYMENT.md](./RELEASE_AND_DEPLOYMENT.md) — Current release, signing, S-drive, and GitHub practice.
- `decisions/` — Architectural decision records (small number of high-signal decisions).

## For Operators & New Contributors

The best orientation is:
1. Read `OVERVIEW.md`
2. Skim `LEGACY_BEHAVIOR.md` (especially the "Must Not Copy" and "Key Risks" sections)
3. Look at the active profile: `config/report-layouts/pdu500.rev02.layout.json`

## Running the Project

Use Bun directly:

```powershell
bun install
bun run desktop          # full Tauri desktop (frontend + backend)
bun run dev:frontend
bun run build
bun run test
bun run lint
bun run validate         # full local validation (lint + test + build + checks + cargo test)
```

Some helper scripts are still executed via Node (version checks, schema validation), but you invoke everything through `bun run <script>`.

## Design history

Feature design specs and implementation plans for shipped work live under `docs/superpowers/` (specs + plans). Prefer the top-level docs in this folder for day-to-day constraints; use `superpowers/` when you need the decision trail for a specific feature.

## Documentation Principles

- Keep living constraints and current design intent here.
- Prefer concise, accurate documents over exhaustive planning notes.
- Source of truth for mappings and thresholds is the JSON files under `config/`, validated by schema + tests.
- Update docs when behavior or structure actually changes.
