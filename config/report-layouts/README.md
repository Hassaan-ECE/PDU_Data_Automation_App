# Report Layout Profiles

This folder holds versioned report layout profiles and report verification configuration.

The goal is to make Excel layout changes editable without touching source code.

Rules:

- Keep profiles machine-valid JSON until the project chooses another format.
- The active bundled production profile is `pdu500.rev02.layout.json`.
- Name future production profiles with product/report revision, for example `pdu500.rev03.layout.json`.
- To test an edited profile outside the repo, set `PDU_LAYOUT_PROFILE_PATH` to the JSON file path.
- Keep accuracy thresholds in `pdu500.accuracy-thresholds.json` until they are folded into a full production layout profile.
- `processor` marks tasks currently handled by built-in Rust processors; add `mappings` as those processors become data-driven.
- Restarting the app is not required for threshold edits; the backend reloads the threshold file each time a processing step runs.
- Keep examples clearly marked as examples.
- Validate profiles in backend tests before using them with real reports.
- Do not store generated reports here.

See [docs/CONFIGURATION_MODEL.md](../../docs/CONFIGURATION_MODEL.md).
