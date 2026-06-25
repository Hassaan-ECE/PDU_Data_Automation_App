# Legacy Behavior and Constraints

This document records behavior from the original Python/PyQt application that the replacement must **preserve** or **intentionally correct**.

Reference legacy locations (for historical comparison only):

- `C:\Projects\Active\PDU_Data_Automation`
- `C:\Projects\Active\Data Automation Upgraded` (preferred reference for system/breaker verification logic)

## Preserve

### Operator Workflow
- Select unit test folder.
- Automatic or explicit report template handling + renaming by SN.
- Scan for existing STEP CSVs.
- Prompt for previously detected CSVs on start (backlog).
- Process steps as data arrives, show progress.
- Allow manual rerun of individual tasks when idle/paused.
- Open main report and print report workbooks.
- Final step: capture operator name, write it to the print report, then open Excel print dialog.

### Visual / Mental Model
Keep the familiar test panel layout:
- Large remaining time + current step/status at top.
- Unit folder selection.
- 208V and 415V sections (Transformer, System, expandable Breakers).
- System Burn-In and Breaker Burn-In sections.
- Color-coded states: off, detected, waiting, processing, pass, warning, fail.
- Manual rerun, follow-step controls, report open actions.

### File Conventions
- CSV files contain `_STEP##_` in the name.
- Main report filename pattern: `PDUD500442AM088_Test Report_0.2CT_Rev02_SN*.xlsx`.
- Main report template pattern ends with `SN##.xlsx`.
- Print report template: `PDUD500442AA088_0.2CT Test Report Print.xlsx`.
- Default template root: `C:/PDU500/00_Template`.
- Unit folders may contain `SN.txt`, `serial_number.txt`, etc.

### Important Semantics
- **Transformer SN**: Written as text to `Test Summary!D1` (must preserve leading zeros and exact value).
- **Final operator name**: Written to `Test Report #2!E39` in the print report.
- **STEP71 vs STEP72 (system burn-in)**:
  - STEP71 = long soak / burn-in period.
  - STEP72 = the short data-capture step used for report values.
- **Verification thresholds** (from upgraded legacy):
  - Voltage / Current: ±0.3%
  - Active/Apparent Power: ±0.6%
  - Power Factor: ±2.0%
  - Missing data or out-of-tolerance → fail (do not write or mark pass).

## Must Not Copy (Correct)

- Exit code `2` from processors was mapped to `detected` → `pass`. Missing or bad data **must never** become pass.
- Silent conversion of missing, blank, or unparsable CSV values to `0.0`. The replacement distinguishes real zero from parse/missing failures.
- Duplicated hardcoded cell maps, column names, and step numbers across scripts. These belong in versioned config.

## Key Risks Carried Forward (AGENTS.md)

- Never treat processor exit code 2 as "detected then pass".
- Respect STEP71 (long) vs STEP72 (report capture).
- Use Data Automation Upgraded as stronger reference for accuracy logic.
- Missing/unparsable values must not silently become valid-looking zeroes.
- Always re-validate Excel template preservation (formatting, formulas, merged cells, no repair prompts) after any change to patching logic or templates.

## Current Status in Replacement (v0.2.9+)

- Strict CSV parsing with no silent zero fallback (tests cover blank/malformed/missing column cases).
- `validate_ready_for_print` gate before writing operator name or opening print dialog.
- Verification runs before any Excel patch for system/breaker tasks.
- CSV stability wait before processing.
- `unit_state.json` sidecar for restart resilience and idempotency.
- Transformer tasks are now driven from `config/report-layouts/pdu500.rev02.layout.json`.
- System, breaker, and burn-in tasks still use built-in processors (progressively moving to mappings).
