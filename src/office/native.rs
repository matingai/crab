use anyhow::{Context, Result, anyhow, bail};
use calamine::{Data, Reader, open_workbook_auto};
use docx_rs::{Docx, Paragraph, Run, read_docx};
use regex::Regex;
use serde_json::{Number, Value, json};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use zip::{ZipArchive, ZipWriter, write::FileOptions};

#[derive(Debug, Clone)]
struct XlsxSheet {
    name: String,
    sheet_id: usize,
    cells: BTreeMap<String, Value>,
    max_row: usize,
    max_col: usize,
}

#[derive(Debug, Clone)]
struct DocxParagraph {
    id: String,
    text: String,
}

#[derive(Debug, Clone)]
struct PptxSlide {
    id: String,
    title: String,
    bullets: Vec<String>,
}

pub fn inspect_xlsx(path: &Path) -> Result<Value> {
    let workbook = load_workbook(path)?;
    Ok(json!({
        "path": path.display().to_string(),
        "format": "xlsx",
        "kind": "spreadsheet",
        "supported": true,
        "capabilities": {
            "preview": true,
            "extract_ir": true,
            "create": true,
            "apply_ops": true,
        },
        "sheetCount": workbook.len(),
        "sheetNames": workbook.iter().map(|sheet| sheet.name.clone()).collect::<Vec<_>>(),
        "nonEmptyCellCount": workbook.iter().map(|sheet| sheet.cells.len()).sum::<usize>(),
    }))
}

pub fn extract_xlsx_ir(path: &Path) -> Result<Value> {
    Ok(workbook_to_ir(&load_workbook(path)?))
}

pub fn preview_xlsx(path: &Path, max_rows: usize, max_cols: usize) -> Result<Value> {
    Ok(workbook_to_preview(
        &load_workbook(path)?,
        max_rows,
        max_cols,
    ))
}

pub fn create_xlsx(output_path: &Path, spec: &Value) -> Result<Value> {
    let workbook = normalize_workbook_spec(spec)?;
    write_workbook(output_path, &workbook)?;
    Ok(json!({
        "outputPath": output_path.display().to_string(),
        "sheetCount": workbook.len(),
        "sheetNames": workbook.iter().map(|sheet| sheet.name.clone()).collect::<Vec<_>>(),
    }))
}

pub fn apply_xlsx_ops(path: &Path, output_path: Option<&Path>, ops: &Value) -> Result<Value> {
    let workbook = load_workbook(path)?;
    let updated = apply_workbook_ops(workbook, ops)?;
    let output_path_buf = output_path
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| make_edited_path(path, "xlsx"));
    write_workbook(&output_path_buf, &updated)?;
    Ok(json!({
        "outputPath": output_path_buf.display().to_string(),
        "sheetCount": updated.len(),
        "sheetNames": updated.iter().map(|sheet| sheet.name.clone()).collect::<Vec<_>>(),
        "nonEmptyCellCount": updated.iter().map(|sheet| sheet.cells.len()).sum::<usize>(),
    }))
}

pub fn inspect_docx(path: &Path) -> Result<Value> {
    let paragraphs = load_docx_paragraphs(path)?;
    Ok(json!({
        "path": path.display().to_string(),
        "format": "docx",
        "kind": "document",
        "supported": true,
        "capabilities": {
            "preview": true,
            "extract_ir": true,
            "create": true,
            "apply_ops": true,
        },
        "paragraphCount": paragraphs.len(),
    }))
}

pub fn extract_docx_ir(path: &Path) -> Result<Value> {
    let paragraphs = load_docx_paragraphs(path)?;
    Ok(json!({
        "type": "docx",
        "paragraphCount": paragraphs.len(),
        "blocks": paragraphs
            .iter()
            .map(|paragraph| json!({
                "id": paragraph.id,
                "kind": "paragraph",
                "text": paragraph.text,
            }))
            .collect::<Vec<_>>(),
    }))
}

pub fn preview_docx(path: &Path, max_paragraphs: usize) -> Result<Value> {
    let paragraphs = load_docx_paragraphs(path)?;
    Ok(json!({
        "type": "docx_preview",
        "paragraphCount": paragraphs.len(),
        "truncated": paragraphs.len() > max_paragraphs,
        "paragraphs": paragraphs
            .iter()
            .take(max_paragraphs)
            .map(|paragraph| paragraph.text.clone())
            .collect::<Vec<_>>(),
    }))
}

pub fn create_docx(output_path: &Path, spec: &Value) -> Result<Value> {
    let paragraphs = normalize_paragraph_spec(spec);
    write_docx(output_path, &paragraphs)?;
    Ok(json!({
        "outputPath": output_path.display().to_string(),
        "paragraphCount": paragraphs.len(),
    }))
}

pub fn apply_docx_ops(path: &Path, output_path: Option<&Path>, ops: &Value) -> Result<Value> {
    let paragraphs = load_docx_paragraphs(path)?;
    let updated = apply_paragraph_ops(paragraphs, ops)?;
    let output_path_buf = output_path
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| make_edited_path(path, "docx"));
    write_docx(&output_path_buf, &updated)?;
    Ok(json!({
        "outputPath": output_path_buf.display().to_string(),
        "paragraphCount": updated.len(),
    }))
}

pub fn inspect_pptx(path: &Path) -> Result<Value> {
    let slides = load_presentation(path)?;
    Ok(json!({
        "path": path.display().to_string(),
        "format": "pptx",
        "kind": "presentation",
        "supported": true,
        "capabilities": {
            "preview": true,
            "extract_ir": true,
            "create": false,
            "apply_ops": false,
        },
        "slideCount": slides.len(),
    }))
}

pub fn extract_pptx_ir(path: &Path) -> Result<Value> {
    let slides = load_presentation(path)?;
    Ok(json!({
        "type": "pptx",
        "slideCount": slides.len(),
        "slides": slides
            .iter()
            .map(|slide| json!({
                "id": slide.id,
                "title": slide.title,
                "bullets": slide.bullets,
            }))
            .collect::<Vec<_>>(),
    }))
}

pub fn preview_pptx(path: &Path, max_slides: usize) -> Result<Value> {
    let slides = load_presentation(path)?;
    Ok(json!({
        "type": "pptx_preview",
        "slideCount": slides.len(),
        "truncated": slides.len() > max_slides,
        "slides": slides
            .iter()
            .take(max_slides)
            .map(|slide| json!({
                "id": slide.id,
                "title": slide.title,
                "bullets": slide.bullets,
            }))
            .collect::<Vec<_>>(),
    }))
}

fn load_docx_paragraphs(path: &Path) -> Result<Vec<DocxParagraph>> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let docx = read_docx(&bytes).with_context(|| format!("failed to parse {}", path.display()))?;
    let parsed: Value = serde_json::from_str(&docx.json()).context("failed to parse docx JSON")?;
    let children = parsed
        .pointer("/document/children")
        .and_then(Value::as_array)
        .context("docx JSON is missing document children")?;
    let mut paragraphs = Vec::new();
    for child in children {
        if child.get("type").and_then(Value::as_str) != Some("paragraph") {
            continue;
        }
        let data = child.get("data").unwrap_or(&Value::Null);
        let text = paragraph_text_from_docx_json(data);
        paragraphs.push(DocxParagraph {
            id: format!("p{}", paragraphs.len() + 1),
            text,
        });
    }
    Ok(paragraphs)
}

fn paragraph_text_from_docx_json(paragraph: &Value) -> String {
    let mut output = String::new();
    if let Some(children) = paragraph.get("children").and_then(Value::as_array) {
        for child in children {
            collect_docx_text(child, &mut output);
        }
    }
    output
}

fn collect_docx_text(node: &Value, output: &mut String) {
    if node.get("type").and_then(Value::as_str) == Some("text") {
        if let Some(text) = node.pointer("/data/text").and_then(Value::as_str) {
            output.push_str(text);
        }
    }
    if let Some(children) = node.pointer("/data/children").and_then(Value::as_array) {
        for child in children {
            collect_docx_text(child, output);
        }
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            collect_docx_text(child, output);
        }
    }
}

fn normalize_paragraph_spec(spec: &Value) -> Vec<DocxParagraph> {
    let mut raw_blocks = Vec::new();
    if let Some(blocks) = spec.get("blocks").and_then(Value::as_array) {
        for block in blocks {
            let kind = block
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("paragraph")
                .to_ascii_lowercase();
            let text = value_to_string(block.get("text").unwrap_or(&Value::Null));
            if kind == "paragraph" {
                raw_blocks.extend(split_paragraph_text(&text));
            } else {
                raw_blocks.push(text);
            }
        }
    } else if spec.get("text").is_some() {
        raw_blocks.extend(split_paragraph_text(&value_to_string(
            spec.get("text").unwrap_or(&Value::Null),
        )));
    } else if let Some(paragraphs) = spec.get("paragraphs").and_then(Value::as_array) {
        raw_blocks.extend(paragraphs.iter().map(value_to_string));
    }
    if raw_blocks.is_empty() {
        raw_blocks.push(String::new());
    }
    raw_blocks
        .into_iter()
        .enumerate()
        .map(|(idx, text)| DocxParagraph {
            id: format!("p{}", idx + 1),
            text,
        })
        .collect()
}

fn split_paragraph_text(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(ToString::to_string)
        .collect()
}

fn apply_paragraph_ops(paragraphs: Vec<DocxParagraph>, ops: &Value) -> Result<Vec<DocxParagraph>> {
    let mut result = paragraphs;
    let ops = ops.as_array().context("docx ops must be an array")?;
    for op in ops {
        let kind = op.get("op").and_then(Value::as_str).unwrap_or_default();
        if kind == "append_paragraph" {
            result.push(DocxParagraph {
                id: String::new(),
                text: value_to_string(op.get("text").unwrap_or(&Value::Null)),
            });
            continue;
        }

        let target = op.get("target").and_then(Value::as_str).unwrap_or_default();
        let index = result
            .iter()
            .position(|paragraph| paragraph.id == target)
            .ok_or_else(|| anyhow!("paragraph not found: {target}"))?;
        match kind {
            "replace_paragraph" => {
                result[index].text = value_to_string(op.get("text").unwrap_or(&Value::Null));
            }
            "insert_paragraph_after" => {
                result.insert(
                    index + 1,
                    DocxParagraph {
                        id: String::new(),
                        text: value_to_string(op.get("text").unwrap_or(&Value::Null)),
                    },
                );
            }
            "remove_paragraph" => {
                result.remove(index);
                if result.is_empty() {
                    result.push(DocxParagraph {
                        id: String::new(),
                        text: String::new(),
                    });
                }
            }
            _ => bail!("unsupported op: {kind}"),
        }
    }
    for (idx, paragraph) in result.iter_mut().enumerate() {
        paragraph.id = format!("p{}", idx + 1);
    }
    Ok(result)
}

fn write_docx(path: &Path, paragraphs: &[DocxParagraph]) -> Result<()> {
    let mut docx = Docx::new();
    for paragraph in paragraphs {
        let p = if paragraph.text.is_empty() {
            Paragraph::new()
        } else {
            Paragraph::new().add_run(Run::new().add_text(&paragraph.text))
        };
        docx = docx.add_paragraph(p);
    }
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    docx.build()
        .pack(file)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn load_workbook(path: &Path) -> Result<Vec<XlsxSheet>> {
    let mut reader = open_workbook_auto(path)
        .with_context(|| format!("failed to open spreadsheet {}", path.display()))?;
    let mut workbook = Vec::new();
    let sheet_names = reader.sheet_names().to_owned();
    for (idx, name) in sheet_names.iter().enumerate() {
        let range = reader
            .worksheet_range(name)
            .with_context(|| format!("failed to read worksheet {name}"))?;
        let formula_range = reader.worksheet_formula(name).ok();
        let start = range.start().unwrap_or((0, 0));
        let mut cells = BTreeMap::new();
        for (row, col, data) in range.used_cells() {
            let abs_row = start.0 as usize + row + 1;
            let abs_col = start.1 as usize + col + 1;
            let addr = format!("{}{}", index_to_col(abs_col), abs_row);
            let formula = formula_range
                .as_ref()
                .and_then(|range| range.get_value(((abs_row - 1) as u32, (abs_col - 1) as u32)))
                .filter(|value| !value.is_empty());
            let value = if let Some(formula) = formula {
                Value::String(format!("={formula}"))
            } else {
                calamine_data_to_json(data)
            };
            if !value_is_blank(&value) {
                cells.insert(addr, value);
            }
        }
        workbook.push(sheet_from_cells(name.clone(), idx + 1, cells)?);
    }
    Ok(workbook)
}

fn calamine_data_to_json(data: &Data) -> Value {
    match data {
        Data::Empty => Value::Null,
        Data::String(value) => Value::String(value.clone()),
        Data::Float(value) if value.fract() == 0.0 => Value::Number(Number::from(*value as i64)),
        Data::Float(value) => Number::from_f64(*value)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(value.to_string())),
        Data::Int(value) => Value::Number(Number::from(*value)),
        Data::Bool(value) => Value::Bool(*value),
        Data::DateTime(value) => Value::String(value.to_string()),
        Data::DateTimeIso(value) | Data::DurationIso(value) => Value::String(value.clone()),
        Data::Error(value) => Value::String(value.to_string()),
    }
}

fn normalize_workbook_spec(spec: &Value) -> Result<Vec<XlsxSheet>> {
    let sheets = spec
        .get("sheets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let sheets = if sheets.is_empty() {
        vec![json!({ "name": "Sheet1", "cells": [] })]
    } else {
        sheets
    };
    let mut workbook = Vec::new();
    for (idx, sheet) in sheets.iter().enumerate() {
        let name = sheet
            .get("name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("Sheet{}", idx + 1));
        let mut cells = BTreeMap::new();
        if let Some(items) = sheet.get("cells").and_then(Value::as_array) {
            for cell in items {
                let addr = cell
                    .get("addr")
                    .and_then(Value::as_str)
                    .context("cell is missing addr")?
                    .to_ascii_uppercase();
                split_addr(&addr)?;
                cells.insert(addr, cell.get("value").cloned().unwrap_or(Value::Null));
            }
        }
        workbook.push(sheet_from_cells(name, idx + 1, cells)?);
    }
    ensure_sheet_names_unique(&workbook)?;
    Ok(workbook)
}

fn apply_workbook_ops(workbook: Vec<XlsxSheet>, ops: &Value) -> Result<Vec<XlsxSheet>> {
    let mut sheets: Vec<XlsxSheet> = workbook;
    let ops = ops.as_array().context("xlsx ops must be an array")?;
    for op in ops {
        let kind = op.get("op").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "add_sheet" => {
                let name = op
                    .get("name")
                    .and_then(Value::as_str)
                    .context("add_sheet requires name")?;
                if sheets.iter().any(|sheet| sheet.name == name) {
                    bail!("sheet already exists: {name}");
                }
                sheets.push(sheet_from_cells(
                    name.to_string(),
                    sheets.len() + 1,
                    BTreeMap::new(),
                )?);
            }
            "remove_sheet" => {
                let name = op
                    .get("name")
                    .and_then(Value::as_str)
                    .context("remove_sheet requires name")?;
                if sheets.len() == 1 {
                    bail!("cannot remove the last sheet");
                }
                let index = sheet_index(&sheets, name)?;
                sheets.remove(index);
            }
            "rename_sheet" => {
                let name = op
                    .get("name")
                    .and_then(Value::as_str)
                    .context("rename_sheet requires name")?;
                let new_name = op
                    .get("new_name")
                    .and_then(Value::as_str)
                    .context("rename_sheet requires new_name")?;
                if sheets.iter().any(|sheet| sheet.name == new_name) {
                    bail!("sheet already exists: {new_name}");
                }
                let index = sheet_index(&sheets, name)?;
                sheets[index].name = new_name.to_string();
            }
            "set_cell" => {
                let sheet = target_sheet_mut(&mut sheets, op)?;
                let addr = op
                    .get("addr")
                    .and_then(Value::as_str)
                    .context("set_cell requires addr")?
                    .to_ascii_uppercase();
                split_addr(&addr)?;
                let value = op.get("value").cloned().unwrap_or(Value::Null);
                set_cell(sheet, &addr, value)?;
            }
            "clear_cell" => {
                let sheet = target_sheet_mut(&mut sheets, op)?;
                let addr = op
                    .get("addr")
                    .and_then(Value::as_str)
                    .context("clear_cell requires addr")?
                    .to_ascii_uppercase();
                sheet.cells.remove(&addr);
            }
            "set_range" => {
                let sheet = target_sheet_mut(&mut sheets, op)?;
                let addr = op
                    .get("addr")
                    .and_then(Value::as_str)
                    .context("set_range requires addr")?
                    .to_ascii_uppercase();
                let (start_col, start_row) = split_addr(&addr)?;
                let values = op
                    .get("values")
                    .and_then(Value::as_array)
                    .context("set_range requires values")?;
                for (row_offset, row_values) in values.iter().enumerate() {
                    let Some(row_values) = row_values.as_array() else {
                        continue;
                    };
                    for (col_offset, value) in row_values.iter().enumerate() {
                        let current_addr = format!(
                            "{}{}",
                            index_to_col(start_col + col_offset),
                            start_row + row_offset
                        );
                        set_cell(sheet, &current_addr, value.clone())?;
                    }
                }
            }
            "append_rows" => {
                let sheet = target_sheet_mut(&mut sheets, op)?;
                let rows = op
                    .get("rows")
                    .and_then(Value::as_array)
                    .context("append_rows requires rows")?;
                let start_row = if sheet.max_row > 0 {
                    sheet.max_row + 1
                } else {
                    1
                };
                for (row_offset, row_values) in rows.iter().enumerate() {
                    let Some(row_values) = row_values.as_array() else {
                        continue;
                    };
                    for (col_offset, value) in row_values.iter().enumerate() {
                        let current_addr =
                            format!("{}{}", index_to_col(col_offset + 1), start_row + row_offset);
                        set_cell(sheet, &current_addr, value.clone())?;
                    }
                }
            }
            "clear_range" => {
                let sheet = target_sheet_mut(&mut sheets, op)?;
                let addr = op
                    .get("addr")
                    .and_then(Value::as_str)
                    .context("clear_range requires addr")?
                    .to_ascii_uppercase();
                let end_addr = op
                    .get("end_addr")
                    .and_then(Value::as_str)
                    .context("clear_range requires end_addr")?
                    .to_ascii_uppercase();
                let (start_col, start_row) = split_addr(&addr)?;
                let (end_col, end_row) = split_addr(&end_addr)?;
                for row in start_row..=end_row {
                    for col in start_col..=end_col {
                        sheet.cells.remove(&format!("{}{}", index_to_col(col), row));
                    }
                }
            }
            _ => bail!("unsupported op: {kind}"),
        }
    }

    let mut normalized = Vec::new();
    for (idx, sheet) in sheets.into_iter().enumerate() {
        normalized.push(sheet_from_cells(sheet.name, idx + 1, sheet.cells)?);
    }
    ensure_sheet_names_unique(&normalized)?;
    Ok(normalized)
}

fn set_cell(sheet: &mut XlsxSheet, addr: &str, value: Value) -> Result<()> {
    let (col, row) = split_addr(addr)?;
    if value_is_blank(&value) {
        sheet.cells.remove(addr);
    } else {
        sheet.cells.insert(addr.to_string(), value);
        sheet.max_row = sheet.max_row.max(row);
        sheet.max_col = sheet.max_col.max(col);
    }
    Ok(())
}

fn target_sheet_mut<'a>(sheets: &'a mut [XlsxSheet], op: &Value) -> Result<&'a mut XlsxSheet> {
    let name = op
        .get("sheet")
        .and_then(Value::as_str)
        .context("operation requires sheet")?;
    let index = sheet_index(sheets, name)?;
    Ok(&mut sheets[index])
}

fn sheet_index(sheets: &[XlsxSheet], name: &str) -> Result<usize> {
    sheets
        .iter()
        .position(|sheet| sheet.name == name)
        .ok_or_else(|| anyhow!("sheet not found: {name}"))
}

fn sheet_from_cells(
    name: String,
    sheet_id: usize,
    cells: BTreeMap<String, Value>,
) -> Result<XlsxSheet> {
    let mut max_row = 0;
    let mut max_col = 0;
    for addr in cells.keys() {
        let (col, row) = split_addr(addr)?;
        max_row = max_row.max(row);
        max_col = max_col.max(col);
    }
    Ok(XlsxSheet {
        name,
        sheet_id,
        cells,
        max_row,
        max_col,
    })
}

fn ensure_sheet_names_unique(workbook: &[XlsxSheet]) -> Result<()> {
    let mut names = HashMap::new();
    for sheet in workbook {
        if names.insert(sheet.name.clone(), ()).is_some() {
            bail!("sheet names must be unique");
        }
    }
    Ok(())
}

fn workbook_to_ir(workbook: &[XlsxSheet]) -> Value {
    json!({
        "type": "xlsx",
        "sheetCount": workbook.len(),
        "sheets": workbook
            .iter()
            .map(|sheet| json!({
                "name": sheet.name,
                "sheetId": sheet.sheet_id,
                "dimensions": {
                    "rows": sheet.max_row,
                    "cols": sheet.max_col,
                },
                "cells": sorted_cells(sheet)
                    .into_iter()
                    .map(|(addr, value)| json!({ "addr": addr, "value": value }))
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    })
}

fn workbook_to_preview(workbook: &[XlsxSheet], max_rows: usize, max_cols: usize) -> Value {
    json!({
        "type": "xlsx_preview",
        "sheetCount": workbook.len(),
        "sheets": workbook
            .iter()
            .map(|sheet| {
                let row_limit = sheet.max_row.min(max_rows);
                let col_limit = sheet.max_col.min(max_cols);
                let mut rows = Vec::new();
                for row_num in 1..=row_limit {
                    let mut values = Vec::new();
                    let mut has_any = false;
                    for col_num in 1..=col_limit {
                        let addr = format!("{}{}", index_to_col(col_num), row_num);
                        let value = sheet.cells.get(&addr).cloned().unwrap_or_else(|| json!(""));
                        if !value_is_blank(&value) {
                            has_any = true;
                        }
                        values.push(value);
                    }
                    if has_any {
                        rows.push(json!({ "row": row_num, "values": values }));
                    }
                }
                json!({
                    "name": sheet.name,
                    "rowCount": sheet.max_row,
                    "colCount": sheet.max_col,
                    "truncatedRows": sheet.max_row > max_rows,
                    "truncatedCols": sheet.max_col > max_cols,
                    "columns": (1..=col_limit).map(index_to_col).collect::<Vec<_>>(),
                    "rows": rows,
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn sorted_cells(sheet: &XlsxSheet) -> Vec<(String, Value)> {
    let mut cells = sheet
        .cells
        .iter()
        .map(|(addr, value)| (addr.clone(), value.clone()))
        .collect::<Vec<_>>();
    cells.sort_by_key(|(addr, _)| {
        split_addr(addr)
            .map(|(col, row)| (row, col))
            .unwrap_or((usize::MAX, usize::MAX))
    });
    cells
}

fn write_workbook(path: &Path, workbook: &[XlsxSheet]) -> Result<()> {
    ensure_sheet_names_unique(workbook)?;
    let shared_strings = build_shared_strings(workbook);
    let file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::<()>::default();
    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(content_types_xml(workbook, !shared_strings.is_empty()).as_bytes())?;
    zip.start_file("_rels/.rels", options)?;
    zip.write_all(root_rels_xml().as_bytes())?;
    zip.start_file("xl/workbook.xml", options)?;
    zip.write_all(workbook_xml(workbook).as_bytes())?;
    zip.start_file("xl/_rels/workbook.xml.rels", options)?;
    zip.write_all(workbook_rels_xml(workbook, !shared_strings.is_empty()).as_bytes())?;
    for (idx, sheet) in workbook.iter().enumerate() {
        zip.start_file(format!("xl/worksheets/sheet{}.xml", idx + 1), options)?;
        zip.write_all(sheet_xml(sheet, &shared_strings).as_bytes())?;
    }
    if !shared_strings.is_empty() {
        zip.start_file("xl/sharedStrings.xml", options)?;
        zip.write_all(shared_strings_xml(&shared_strings).as_bytes())?;
    }
    zip.finish()?;
    Ok(())
}

fn build_shared_strings(workbook: &[XlsxSheet]) -> BTreeMap<String, usize> {
    let mut shared_strings = BTreeMap::new();
    for sheet in workbook {
        for value in sheet.cells.values() {
            if let ClassifiedCellValue::String(text) = classify_cell_value(value) {
                if !shared_strings.contains_key(&text) {
                    shared_strings.insert(text, shared_strings.len());
                }
            }
        }
    }
    shared_strings
}

enum ClassifiedCellValue {
    Bool(String),
    Number(String),
    Formula(String),
    String(String),
}

fn classify_cell_value(value: &Value) -> ClassifiedCellValue {
    match value {
        Value::Bool(value) => ClassifiedCellValue::Bool(if *value { "1" } else { "0" }.into()),
        Value::Number(value) => ClassifiedCellValue::Number(value.to_string()),
        Value::String(value) if value.starts_with('=') && value.len() > 1 => {
            ClassifiedCellValue::Formula(value[1..].to_string())
        }
        Value::Null => ClassifiedCellValue::String(String::new()),
        Value::String(value) => ClassifiedCellValue::String(value.clone()),
        other => ClassifiedCellValue::String(other.to_string()),
    }
}

fn sheet_xml(sheet: &XlsxSheet, shared_strings: &BTreeMap<String, usize>) -> String {
    let dim_ref = if sheet.max_row > 0 && sheet.max_col > 0 {
        format!("A1:{}{}", index_to_col(sheet.max_col), sheet.max_row)
    } else {
        "A1".to_string()
    };
    let mut rows: BTreeMap<usize, Vec<(usize, String, Value)>> = BTreeMap::new();
    for (addr, value) in &sheet.cells {
        if let Ok((col, row)) = split_addr(addr) {
            rows.entry(row)
                .or_default()
                .push((col, addr.clone(), value.clone()));
        }
    }

    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dimension ref="{dim_ref}"/><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/><sheetData>"#
    );
    for (row, mut cells) in rows {
        cells.sort_by_key(|(col, _, _)| *col);
        xml.push_str(&format!(r#"<row r="{row}">"#));
        for (_, addr, value) in cells {
            match classify_cell_value(&value) {
                ClassifiedCellValue::String(text) => {
                    let idx = shared_strings.get(&text).copied().unwrap_or(0);
                    xml.push_str(&format!(r#"<c r="{addr}" t="s"><v>{idx}</v></c>"#));
                }
                ClassifiedCellValue::Bool(value) => {
                    xml.push_str(&format!(r#"<c r="{addr}" t="b"><v>{value}</v></c>"#));
                }
                ClassifiedCellValue::Formula(value) => {
                    xml.push_str(&format!(
                        r#"<c r="{addr}"><f>{}</f></c>"#,
                        xml_escape(&value)
                    ));
                }
                ClassifiedCellValue::Number(value) => {
                    xml.push_str(&format!(
                        r#"<c r="{addr}"><v>{}</v></c>"#,
                        xml_escape(&value)
                    ));
                }
            }
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData></worksheet>");
    xml
}

fn shared_strings_xml(shared_strings: &BTreeMap<String, usize>) -> String {
    let mut items = shared_strings
        .iter()
        .map(|(text, idx)| (*idx, text.as_str()))
        .collect::<Vec<_>>();
    items.sort_by_key(|(idx, _)| *idx);
    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{}" uniqueCount="{}">"#,
        items.len(),
        items.len()
    );
    for (_, item) in items {
        xml.push_str(&format!("<si><t>{}</t></si>", xml_escape(item)));
    }
    xml.push_str("</sst>");
    xml
}

fn workbook_xml(workbook: &[XlsxSheet]) -> String {
    let mut xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr/><bookViews><workbookView xWindow="0" yWindow="0" windowWidth="28800" windowHeight="17100"/></bookViews><sheets>"#.to_string();
    for (idx, sheet) in workbook.iter().enumerate() {
        xml.push_str(&format!(
            r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#,
            xml_escape(&sheet.name),
            idx + 1,
            idx + 1
        ));
    }
    xml.push_str(r#"</sheets><calcPr calcId="171027"/></workbook>"#);
    xml
}

fn workbook_rels_xml(workbook: &[XlsxSheet], has_shared_strings: bool) -> String {
    let mut xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#.to_string();
    let mut rel_id = 1;
    for idx in 1..=workbook.len() {
        xml.push_str(&format!(r#"<Relationship Id="rId{rel_id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{idx}.xml"/>"#));
        rel_id += 1;
    }
    if has_shared_strings {
        xml.push_str(&format!(r#"<Relationship Id="rId{rel_id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>"#));
    }
    xml.push_str("</Relationships>");
    xml
}

fn content_types_xml(workbook: &[XlsxSheet], has_shared_strings: bool) -> String {
    let mut xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#.to_string();
    for idx in 1..=workbook.len() {
        xml.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{idx}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
    }
    if has_shared_strings {
        xml.push_str(r#"<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>"#);
    }
    xml.push_str("</Types>");
    xml
}

fn root_rels_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
}

fn load_presentation(path: &Path) -> Result<Vec<PptxSlide>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut zip =
        ZipArchive::new(file).with_context(|| format!("failed to read {}", path.display()))?;
    let presentation_xml = read_zip_string(&mut zip, "ppt/presentation.xml")?;
    let rels_xml = read_zip_string(&mut zip, "ppt/_rels/presentation.xml.rels")?;
    let rels = parse_relationships(&rels_xml, "ppt");
    let mut slides = Vec::new();
    for slide_attrs in tag_attrs(&presentation_xml, "sldId") {
        let rel_id = attr_exact(&slide_attrs, "r:id")
            .or_else(|| attr(&slide_attrs, "id"))
            .unwrap_or_default();
        let Some(slide_path) = rels.get(&rel_id) else {
            continue;
        };
        let Ok(slide_xml) = read_zip_string(&mut zip, slide_path) else {
            continue;
        };
        let (title, bullets) = parse_slide_text(&slide_xml);
        slides.push(PptxSlide {
            id: format!("s{}", slides.len() + 1),
            title,
            bullets,
        });
    }
    Ok(slides)
}

fn parse_slide_text(xml: &str) -> (String, Vec<String>) {
    let shape_re =
        Regex::new(r#"(?is)<(?:\w+:)?sp\b[^>]*>(.*?)</(?:\w+:)?sp>"#).expect("shape regex");
    let mut title = String::new();
    let mut bullets = Vec::new();
    for captures in shape_re.captures_iter(xml) {
        let shape = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
        let text = all_tag_text(shape, "t").join("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        let is_title = shape.contains(r#"type="title""#) || shape.contains(r#"type="ctrTitle""#);
        if (is_title || title.is_empty()) && title.is_empty() {
            title = text;
        } else {
            bullets.push(text);
        }
    }
    (title, bullets)
}

fn parse_relationships(xml: &str, base_dir: &str) -> HashMap<String, String> {
    let mut rels = HashMap::new();
    for attrs in tag_attrs(xml, "Relationship") {
        let Some(id) = attr(&attrs, "Id") else {
            continue;
        };
        let Some(target) = attr(&attrs, "Target") else {
            continue;
        };
        rels.insert(id, normalize_target(base_dir, &target));
    }
    rels
}

fn normalize_target(base_dir: &str, target: &str) -> String {
    if target.starts_with('/') {
        return target.trim_start_matches('/').to_string();
    }
    let joined = format!("{base_dir}/{target}");
    let mut parts = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn tag_attrs(xml: &str, tag: &str) -> Vec<String> {
    let pattern = format!(r#"(?is)<(?:\w+:)?{}\b([^>]*)/?>"#, regex::escape(tag));
    Regex::new(&pattern)
        .expect("tag attrs regex")
        .captures_iter(xml)
        .filter_map(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .collect()
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let escaped = regex::escape(name);
    let pattern = format!(r#"(?i)(?:^|\s)(?:\w+:)?{escaped}="([^"]*)""#);
    Regex::new(&pattern)
        .expect("attr regex")
        .captures(attrs)
        .and_then(|captures| captures.get(1))
        .map(|m| xml_unescape(m.as_str()))
}

fn attr_exact(attrs: &str, name: &str) -> Option<String> {
    let escaped = regex::escape(name);
    let pattern = format!(r#"(?i)(?:^|\s){escaped}="([^"]*)""#);
    Regex::new(&pattern)
        .expect("exact attr regex")
        .captures(attrs)
        .and_then(|captures| captures.get(1))
        .map(|m| xml_unescape(m.as_str()))
}

fn all_tag_text(xml: &str, tag: &str) -> Vec<String> {
    let pattern = format!(
        r#"(?is)<(?:\w+:)?{}\b[^>]*>(.*?)</(?:\w+:)?{}>"#,
        regex::escape(tag),
        regex::escape(tag)
    );
    Regex::new(&pattern)
        .expect("text tag regex")
        .captures_iter(xml)
        .map(|captures| xml_unescape(captures.get(1).map(|m| m.as_str()).unwrap_or_default()))
        .collect()
}

fn read_zip_string(zip: &mut ZipArchive<File>, name: &str) -> Result<String> {
    let mut file = zip
        .by_name(name)
        .with_context(|| format!("zip entry not found: {name}"))?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .with_context(|| format!("failed to read zip entry: {name}"))?;
    Ok(content)
}

fn split_addr(addr: &str) -> Result<(usize, usize)> {
    let mut letters = String::new();
    let mut digits = String::new();
    for ch in addr.chars() {
        if ch.is_ascii_alphabetic() {
            letters.push(ch.to_ascii_uppercase());
        } else if ch.is_ascii_digit() {
            digits.push(ch);
        }
    }
    if letters.is_empty() || digits.is_empty() {
        bail!("invalid cell address: {addr}");
    }
    Ok((col_to_index(&letters), digits.parse()?))
}

fn col_to_index(col: &str) -> usize {
    col.chars().fold(0, |acc, ch| {
        acc * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1)
    })
}

fn index_to_col(mut index: usize) -> String {
    let mut chars = Vec::new();
    while index > 0 {
        index -= 1;
        chars.push((b'A' + (index % 26) as u8) as char);
        index /= 26;
    }
    chars.iter().rev().collect()
}

fn value_is_blank(value: &Value) -> bool {
    value.is_null() || value.as_str() == Some("")
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn make_edited_path(path: &Path, fallback_ext: &str) -> std::path::PathBuf {
    let mut file_name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document")
        .to_string();
    file_name.push_str(".edited.");
    file_name.push_str(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or(fallback_ext),
    );
    path.with_file_name(file_name)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
