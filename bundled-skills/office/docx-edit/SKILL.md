---
name: docx-edit
description: Edit or generate docx files using paragraph-oriented V1 office tools first, then escalate to python-docx scripts for richer document structure.
keywords:
  - office
  - word
  - docx
  - document
  - paragraphs
  - report
  - memo
  - letter
  - 文档
  - 段落
requires_tools:
  - office_inspect
  - office_preview
  - office_extract_ir
  - office_apply_ops
  - office_create
updated_at_unix: 1776513600
---

# DOCX Edit

Use this skill when the user wants to inspect, draft, or modify `.docx` content.

## Default Workflow

1. Call `office_inspect` first to confirm the file is `.docx`.
2. Use `office_preview` for a quick paragraph snapshot.
3. Use `office_extract_ir` before planning edits so you can target paragraph ids explicitly.
4. Use `office_apply_ops` for paragraph-oriented changes:
   `replace_paragraph`, `insert_paragraph_after`, `append_paragraph`, `remove_paragraph`.
5. Use `office_create` for simple new documents.
6. Save to a new output path unless the user explicitly asks for overwrite semantics.

## Escalate To A Python Script

Prefer `write_file` + `execute_code` with `python-docx` when the user expects richer document structure:

- headings, sections, spacing, or page layout
- tables, images, headers, footers, or lists
- polished reports, memos, letters, or templates
- consistent style application across many paragraphs

When using a script:

- generate a new output file path
- keep the document structure explicit and deterministic
- print the output path and a compact summary of sections, tables, or paragraphs changed
- state clearly when tracked changes, comments, or exact style fidelity are out of scope

See `DOCX.md` for a script pattern.
