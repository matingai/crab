# DOCX Script Notes

Use `office_*` tools for paragraph-level edits. Use `python-docx` when you need richer Word output.

## Preferred Script Pattern

1. Use `write_file` to create a script such as `scripts/build_report.py`.
2. Run it with `execute_code` using `path`.
3. Build a new document or load an existing one.
4. Save to a new output path and print a short summary.

## Example Skeleton

```python
from docx import Document

doc = Document()
doc.add_heading("Weekly Report", level=1)
doc.add_paragraph("Summary paragraph.")
table = doc.add_table(rows=2, cols=2)
table.cell(0, 0).text = "Metric"
table.cell(0, 1).text = "Value"
table.cell(1, 0).text = "Status"
table.cell(1, 1).text = "Ready"
doc.save("weekly-report.docx")
print({"output": "weekly-report.docx", "tables": 1})
```

## Guidance

- Prefer explicit headings, paragraphs, and tables over raw text dumps.
- If the user wants a polished deliverable, structure the document instead of just appending paragraphs.
- Be clear about limitations around tracked changes, comments, and exact import-preserving edits.
