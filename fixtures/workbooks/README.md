# Workbook Fixtures

Synthetic or sanitized Excel workbook fixtures used by backend integration tests.

Rules:

- Do not store production workbooks here.
- Do not store customer data, private operator data, or real test records.
- Keep workbook fixtures small and safe for source control.
- Do not commit Excel temporary lock files such as `~$*.xlsx`.
- Tests must copy fixtures to a temp directory before any operation that could mutate them.

Current fixtures are minimal `.xlsx` shells used to exercise report discovery and report-write
smoke behavior. They are not full production templates.

Report-write smoke fixtures:

- `PDUD500442AM088_Test Report_0.2CT_Rev02_SN262343000072.xlsx`
  - Contains `Test Summary` for Transformer SN writes.
- `PDUD500442AA088_Write Smoke Test Report Print.xlsx`
  - Contains `Test Report #2` for final operator-name writes.
