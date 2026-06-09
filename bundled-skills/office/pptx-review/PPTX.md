# PPTX Script Notes

Built-in `office_*` tools currently review `.pptx` content only. For deck generation or edits, use a Python script with `python-pptx`.

## Preferred Script Pattern

1. Use `write_file` to create a script such as `scripts/build_deck.py`.
2. Run it with `execute_code` using `path`.
3. Build slides with a simple, explicit structure.
4. Save to a new `.pptx` file and print the path plus slide count.

## Example Skeleton

```python
from pptx import Presentation

prs = Presentation()
layout = prs.slide_layouts[1]
slide = prs.slides.add_slide(layout)
slide.shapes.title.text = "Project Update"
body = slide.placeholders[1].text_frame
body.text = "Milestone 1 complete"
for bullet in ["Backend stable", "UI in progress", "Next: testing"]:
    p = body.add_paragraph()
    p.text = bullet
prs.save("project-update.pptx")
print({"output": "project-update.pptx", "slides": len(prs.slides)})
```

## Guidance

- Prefer simple title-and-content layouts unless the user provided a template.
- Keep text explicit and readable before chasing visual polish.
- If the user wants exact theme matching or complex charts, say that generated decks may need a final manual design pass.
