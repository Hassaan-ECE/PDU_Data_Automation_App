# Shared Contracts

Shared files in this directory must have a clear source of truth and validation path.

Current scope:

- `schemas/report-layout.schema.json` documents and validates the report-layout config files under `config/report-layouts/`.

Rules:

- Do not put hand-maintained Rust and TypeScript duplicate IPC types here.
- Generated files must be labeled as generated and include the generation command.
- Runtime IPC DTOs remain owned by the Rust command structs and the typed frontend Tauri wrappers until a generation strategy is adopted.
- Schema changes must be validated with `node scripts/fixtures/validate-report-layout-schema.mjs`.
