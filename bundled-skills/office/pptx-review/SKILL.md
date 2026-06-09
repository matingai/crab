---
name: pptx-review
description: Review existing pptx decks with V1 slide extraction, and only use direct pptx generation paths when Slidev is not appropriate or `.pptx` is explicitly required.
keywords:
  - office
  - powerpoint
  - pptx
  - presentation
  - slides
  - deck
  - bullets
  - pitch
  - 演示
  - 幻灯片
requires_tools:
  - office_inspect
  - office_preview
  - office_extract_ir
updated_at_unix: 1776513600
---

# PPTX Review

Use this skill when the user wants to inspect, summarize, plan, or modify an existing `.pptx` presentation.

## Default Review Workflow

1. Call `office_inspect` to confirm the file is `.pptx` and read capability boundaries.
2. Call `office_preview` for slide titles and extracted bullets.
3. Call `office_extract_ir` when you need stable slide ids and a fuller structured view.
4. Summarize the deck, identify weak slides, or propose revisions from the extracted structure.

## Current V1 Limitation

Built-in `.pptx` support is read-only. Do not pretend that `office_apply_ops` exists for presentations.

## Write Workflow

If the user needs a new deck, prefer `slides/slidev-deck` first unless the `.pptx` file itself is the required authoring format.

If the user still needs direct `.pptx` creation or modification, prefer `write_file` + `execute_code` with `python-pptx`.

Use a script when the task involves:

- creating a new deck from notes or markdown
- replacing titles or bullet lists
- generating status, report, or pitch slides
- producing a deliverable `.pptx` instead of only an edit plan

When using a script:

- save to a new `.pptx` path
- keep the slide structure explicit: title plus bullet content or simple content blocks
- print the output path and slide count
- be clear that exact themes, animations, notes, and advanced layout geometry may still need manual polish

See `PPTX.md` for a script pattern.
