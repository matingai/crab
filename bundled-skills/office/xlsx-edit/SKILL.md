---
name: xlsx-edit
description: Edit or generate xlsx workbooks using either deterministic V1 office tools or a Python workbook script when higher fidelity is needed.
keywords:
  - office
  - excel
  - xlsx
  - spreadsheet
  - workbook
  - cells
  - sheets
  - formulas
  - csv
  - tsv
  - 表格
  - 工作簿
requires_tools:
  - office_inspect
  - office_preview
  - office_extract_ir
  - office_apply_ops
  - office_create
updated_at_unix: 1776513600
---

# XLSX Edit

Use this skill when the user wants to inspect, create, clean, or modify spreadsheet-style data in `.xlsx` files.

## Default Workflow

1. Call `office_inspect` to confirm the file is `.xlsx`.
2. Call `office_preview` for a compact grid snapshot.
3. Call `office_extract_ir` if you need explicit sheet names, dimensions, and cell values.
4. Prefer `office_apply_ops` for deterministic data edits:
   `add_sheet`, `remove_sheet`, `rename_sheet`, `set_cell`, `clear_cell`, `set_range`, `append_rows`, `clear_range`.
5. Use `office_create` for simple new workbooks.
6. Save to a new output path unless overwrite is explicitly intended.

## Escalate To A Python Script

Prefer `write_file` + `execute_code` with `openpyxl` when the task depends on workbook fidelity rather than plain data:

- preserving an existing template
- styles, fonts, fills, borders, widths, freezes, merges
- formulas, references, and multi-sheet models
- CSV or TSV ingestion before export
- repeated transformations across many sheets or files

When using a script:

- work on a copy of the workbook, not the only original
- use Excel formulas instead of hardcoding computed values
- preserve sheet names and structure unless the user asked to change them
- print a concise summary of created files, changed sheets, and key ranges

See `XLSX.md` for a script pattern.
