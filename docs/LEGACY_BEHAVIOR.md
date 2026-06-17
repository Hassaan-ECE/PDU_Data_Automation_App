# Legacy Behavior To Preserve Or Correct

The original legacy app is the working Python/PyQt bundle in:

```text
C:\Projects\Active\PDU_Data_Automation
```

There is also a newer legacy/reference folder:

```text
C:\Projects\Active\Data Automation Upgraded
```

Use `Data Automation Upgraded` as the preferred reference for 208V/415V system and breaker behavior. Its newer scripts add Python-side accuracy calculations and pass/fail verification. The transformer and burn-in scripts are the same as the older folder. Ignore `NOTHING.py`; it is not valid Python and appears to be a scratch HTML/CSS/JS prototype.

This document records behavior the replacement must either preserve or intentionally correct.

## Preserve

### Operator Workflow

- User selects a unit test folder.
- App copies or locates report templates.
- App scans existing CSV files for completed STEP data.
- App prompts whether to process previous detected tests.
- App starts the sequence and waits for source files.
- App processes each step through the matching report writer.
- App logs per-step output.
- App allows manual rerun of individual tests when the runner is idle or paused.
- App can open the main report.

### Visual Layout

The replacement should keep the same mental model:

- large total time display at the top
- current step/status label
- unit folder selector
- scrollable test sections
- 208V transformer, system, and breaker sections
- 415V transformer, system, and breaker sections
- system burn-in section
- breaker burn-in section
- expandable breaker rows
- state colors for off, detected, running, pass, and fail
- start/pause/resume and reset controls at the bottom

The new UI can be cleaner, but it should not force operators to relearn the workflow.

### File Expectations

The current file conventions should remain supported:

- CSV filenames contain `_STEP##_`.
- Main report pattern: `PDUD500442AM088_Test Report_0.2CT_Rev02_SN*.xlsx`.
- Main report template: `PDUD500442AM088_Test Report_0.2CT_Rev02_SN##.xlsx`.
- Print report template: `PDUD500442AA088_0.2CT Test Report Print.xlsx`.
- Template directory default: `C:/PDU500/00_Template`.
- Unit folders may contain metadata files such as `SN.txt`, `serial_number.txt`, `info.txt`, or `metadata.txt`.

### Logical Test Coverage

The replacement must cover:

- 208V transformer check
- 208V system 100%, 50%, and 20% load tests
- 208V breaker 1-8 100%, 50%, and 20% load tests
- 415V transformer check
- 415V system 100%, 50%, and 20% load tests
- 415V breaker 1-8 100%, 50%, and 20% load tests
- system burn-in
- breaker burn-in 1-8

### Accuracy Verification

The upgraded 208V/415V system and breaker scripts calculate accuracy values in Python, write those accuracy cells, and return failure when thresholds are exceeded.

The replacement should preserve this behavior:

- voltage and current threshold: +/-0.3%
- active and apparent power threshold: +/-0.6%
- power factor threshold: +/-2.0%
- missing accuracy data fails verification
- verification failures stop the automated sequence and surface a clear operator warning

## Correct

### Exit Code 2 Handling

The legacy GUI maps processor exit code `2` to `detected`, then later converts `detected` to `pass`. The processor scripts use exit code `2` for missing data or no data written.

The replacement must not mark missing data as pass.

### STEP71 / STEP72 Burn-In Workflow

This is intentional:

- STEP71 is the long system burn-in/soak period.
- STEP72 is the quick burn-in data capture used for report values.

The replacement should model both concepts clearly. The UI may show the system burn-in workflow as one operator step, but the report writer should use STEP72 data for the system burn-in report values.

### Silent Zeroes

The legacy processors sometimes convert missing, invalid, or unparsable values to `0.0`.

The replacement must distinguish:

- real numeric zero
- missing source column
- missing source row
- blank source value
- nonnumeric source value
- optional skipped value

### Duplicated Mappings

The legacy app duplicates report filenames, sheet names, cell maps, source columns, and step numbers across several scripts.

The replacement should keep these in versioned layout config files and validate them at startup.

### Burn-In CLI Parsing

The legacy burn-in scripts parse `--unit=<path>` manually and do not consistently honor `--unit <path>`.

The replacement should have one backend path resolution flow.

## Open Questions

- What is the final production S-drive root for PDU releases?
- What should the app identifier be? Proposed: `com.te.pdu.data.automation`.
- Should the template directory remain hardcoded by default or be user-configurable in settings?
- Which Rust Excel writer safely preserves the current templates?
- Should the app keep local session history, or only write per-unit logs?
