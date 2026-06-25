# Decision 0002: Use Upgraded Legacy Scripts For Verification Behavior

## Status

Accepted.

## Context

Two Python/PyQt legacy folders are available:

```text
C:\Projects\Active\PDU_Data_Automation
C:\Projects\Active\Data Automation Upgraded
```

The older folder is documented and runnable. The upgraded folder has newer 208V/415V system and breaker scripts that add Python-side accuracy calculations, write computed accuracy cells, and return failure when threshold checks fail.

The upgraded folder also contains `NOTHING.py`, which is not valid Python (appears to be a scratch prototype).

## Decision

Use `C:\Projects\Active\Data Automation Upgraded` as the primary reference for 208V/415V system and breaker report-writing verification behavior.

Use the older `C:\Projects\Active\PDU_Data_Automation` as supporting context for other areas.

Do not treat `NOTHING.py` as production source.

## Preserved Verification Behavior

Preserve these threshold checks unless a later requirement changes them:

- Voltage / Current: +/- 0.3%
- Active / Apparent Power: +/- 0.6%
- Power Factor: +/- 2.0%
- Missing accuracy data fails verification
- Failed verification stops or pauses the run and surfaces a clear operator warning

Logs and UI text must remain clean ASCII / valid UTF-8.

## Burn-In Clarification (Preserved)

- STEP71 = long system burn-in / soak period
- STEP72 = quick burn-in data capture used for report values

The report writer uses STEP72 data. The UI presents burn-in as one operator workflow.
