# Report Layout Profiles

This folder holds versioned report layout profiles.

The goal is to make Excel layout changes editable without touching source code.

Rules:

- Keep profiles machine-valid JSON until the project chooses another format.
- Name production profiles with product/report revision, for example `pdu500.rev02.layout.json`.
- Keep examples clearly marked as examples.
- Validate profiles in backend tests before using them with real reports.
- Do not store generated reports here.

See `docs/CONFIGURATION_MODEL.md`.
