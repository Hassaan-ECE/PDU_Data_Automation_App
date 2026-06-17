# Decision 0002: Use Upgraded Legacy Scripts For Verification Behavior

## Status

Accepted.

## Context

Two Python/PyQt legacy folders are available:

```text
C:\Projects\Active\PDU_Data_Automation
C:\Projects\Active\Data Automation Upgraded
```

The older folder is documented and runnable. The upgraded folder has newer 208V/415V system and breaker scripts. Those scripts add Python-side accuracy calculations, write computed accuracy cells, and return failure when threshold checks fail.

The upgraded folder also contains `NOTHING.py`, which is not valid Python and appears to be a scratch HTML/CSS/JS prototype.

## Decision

Use `C:\Projects\Active\Data Automation Upgraded` as the primary reference for 208V/415V system and breaker report-writing verification behavior.

Use the older `C:\Projects\Active\PDU_Data_Automation` docs and scripts as supporting context.

Do not treat `NOTHING.py` as production source.

## Preserved Verification Behavior

The rebuild should preserve these threshold checks unless a later requirement changes them:

- voltage: +/-0.3%
- current: +/-0.3%
- active/apparent power: +/-0.6%
- power factor: +/-2.0%
- missing accuracy data fails verification
- failed verification stops or pauses the automated run and shows an obvious operator warning

The rebuild should keep the logs and UI text clean ASCII or valid UTF-8. The upgraded scripts currently display a garbled threshold symbol in console output on this machine.

## Burn-In Clarification

System burn-in uses two related steps:

- STEP71 is the long system burn-in/soak period.
- STEP72 is the quick burn-in data capture used for report values.

The report writer should read STEP72 for system burn-in values. The UI should make the relationship clear instead of representing it as an unresolved mismatch.
