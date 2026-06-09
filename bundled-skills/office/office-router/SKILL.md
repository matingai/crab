---
name: office-router
description: Route Office requests into the correct xlsx, docx, or pptx workflow, while preferring Slidev for new presentation drafts unless `.pptx` is explicitly required.
keywords:
  - office
  - excel
  - xlsx
  - word
  - docx
  - powerpoint
  - pptx
  - spreadsheet
  - document
  - presentation
  - slides
  - 表格
  - 文档
  - 幻灯片
requires_tools:
  - office_inspect
  - office_preview
  - office_extract_ir
updated_at_unix: 1776513600
---

# Office Router

Use this skill first when the user mentions an Office file or asks for an Office-style deliverable and the exact workflow is not yet clear.

## Routing

1. Call `office_inspect` on the target path before planning edits.
2. Route by actual format and reported capabilities, not by the user's label alone.
3. If the file is `.xlsx`, switch to `office/xlsx-edit`.
4. If the file is `.docx`, switch to `office/docx-edit`.
5. If the file is `.pptx`, switch to `office/pptx-review`.
6. If the file is unsupported, stop and explain the limitation instead of guessing.
7. If the user wants a new presentation but has not required `.pptx`, prefer `slides/slidev-deck` instead of the `.pptx` workflow.

## Choose The Execution Path

Start with the built-in `office_*` tools when the task is mostly structural:

- inspect or summarize an Office file
- simple workbook cell or sheet edits
- paragraph-level `.docx` rewrites
- slide review or extraction from `.pptx`

Escalate to a workspace script via `write_file` + `execute_code` when the user likely expects high fidelity or complex document behavior:

- exact template preservation
- formatting, widths, merges, formulas, charts, comments, or images
- `.pptx` creation or direct modification
- large data cleanup or conversions
- deterministic repeatability across multiple files

Prefer Slidev over `.pptx` creation when the user is asking for:

- a new slide deck from notes
- a pitch deck draft
- a presentation that will be iterated in-browser
- a deck outline before final export

Prefer a short script file over giant one-off shell commands. Save outputs to a new path unless the user explicitly wants in-place replacement.
