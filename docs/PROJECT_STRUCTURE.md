# Project Structure

The repository is split by clear responsibility.

```
PDU_Data_Automation_App/
├── .github/workflows/          # CI (source validation only on windows-latest)
├── backend/                    # Tauri 2 + Rust crate (the real backend)
│   ├── src/
│   │   ├── automation/         # CSV, processors, mapped writes, reports, state
│   │   ├── config/             # layout profile + accuracy loading
│   │   ├── commands.rs         # thin Tauri command boundary
│   │   └── lib.rs / main.rs
│   ├── tests/                  # integration-style tests (fixtures, Excel fidelity)
│   └── tauri.conf.json
├── config/report-layouts/      # Versioned data-driven profiles (source of truth for mappings)
├── docs/                       # Current project documentation (this folder)
├── fixtures/                   # Safe synthetic CSVs + minimal workbooks for tests
├── frontend/                   # React + TS + Vite + Tailwind
│   └── src/features/test-panel/  # The operator panel (main UI surface)
├── release/                    # Local release notes only (generated artifacts ignored)
├── scripts/                    # Validation, release helpers, and checks
├── shared/                     # Cross-cutting contracts
│   └── schemas/                # report-layout.schema.json
├── AGENTS.md
├── package.json                # Root task runner (Bun)
└── README.md
```

## Key Areas

- `backend/` — owns file system work, CSV parsing, verification, Excel patching, and all report logic. Commands in `commands.rs` are intentionally thin.
- `frontend/` — owns operator interaction only. Currently the majority of the UI lives in `OperatorPanel.tsx` (will be split as complexity grows).
- `config/report-layouts/` — the production `pdu500.rev02.layout.json` and accuracy thresholds. Prefer editing here over code.
- `fixtures/` — deliberately small, safe data. Tests copy them to temp dirs.
- `docs/` — living documentation. Historical plans and old detailed notes live in `old-docs/`.
- `scripts/` — validation scripts, version checks, and local release helpers. Use `bun run validate` (and other `bun run` commands) directly. See `scripts/README.md` for details.

## What Belongs Where

- New report mappings or threshold changes → `config/report-layouts/`
- New tasks or panel layout changes → update layout profile + frontend task model (eventually profile-driven)
- New backend behavior → Rust under `automation/` + tests
- Operator UX or state changes → frontend
- Release/installer behavior → validate with `build:desktop` locally; keep generated artifacts out of git

Do not add large new top-level folders without a clear ownership reason.
