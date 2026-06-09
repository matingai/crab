# XLSX Script Notes

Use `office_*` tools for lightweight structural edits. Use a Python script when workbook fidelity matters.

## Preferred Script Pattern

1. Use `write_file` to save a workspace script such as `scripts/update_workbook.py`.
2. Use `execute_code` with `path` to run it.
3. Read the workbook with `openpyxl.load_workbook(...)`.
4. Save to a new `.xlsx` path unless the user explicitly wants overwrite behavior.
5. Print the output file path and a small summary of changed sheets and cells.

## Example Skeleton

```python
from openpyxl import load_workbook

src = "input.xlsx"
dst = "output.xlsx"

wb = load_workbook(src)
ws = wb["Sheet1"]
ws["A1"] = "Updated"
ws["B2"] = "=SUM(B3:B10)"
wb.save(dst)
print({"output": dst, "sheet": ws.title, "changed": ["A1", "B2"]})
```

## Guidance

- Prefer formulas over hardcoded calculated values.
- For rectangular data updates, fill ranges consistently.
- For template work, preserve widths, merges, and styles unless the user asks for redesign.
- If the task mentions charts, macros, pivots, or exact Excel fidelity, say clearly what is and is not preserved.
