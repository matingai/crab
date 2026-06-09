---
name: slidev-deck
description: Create and iterate on Slidev presentation decks, defaulting to local preview immediately after deck generation.
keywords:
  - slidev
  - slides
  - presentation
  - slide deck
  - pitch deck
  - keynote
  - demo deck
  - 幻灯片
  - 演示
  - 路演
requires_tools:
  - slidev_create
  - slidev_preview
updated_at_unix: 1776513600
---

# Slidev Deck

Use this skill when the user wants a new presentation, slide deck, or talk outline and does not explicitly require editing an existing `.pptx` file first.

## Default Workflow

1. Prefer `slidev_create` to scaffold the deck instead of creating `.pptx` directly.
2. Let `slidev_create` start preview by default so the deck is immediately reviewable in the browser.
3. Use a built-in template when the user is describing a common deck shape:
   - `pitch`
   - `product-launch`
   - `research`
   - `weekly-review`
4. If the user provides a desired structure, pass explicit `slides` so the initial draft already reflects that outline.
5. Use `slidev_preview` only when preview needs to be restarted for an existing `.md/.mdx` deck.

## Routing Rules

- If the user says "make slides", "create a deck", "prepare a presentation", or similar, start with Slidev.
- If the user explicitly needs a `.pptx` output or is modifying an existing `.pptx`, switch to the Office/PPTX workflow.
- If the user wants a polished browser-first deck that can later be exported, stay in Slidev as long as possible.

## Notes

- Slidev is the preferred authoring format for new presentation work.
- Exact PowerPoint fidelity is not the goal of this workflow.
- If the user later needs `.pptx`, generate the Slidev deck first and only then convert/export as a separate step.
