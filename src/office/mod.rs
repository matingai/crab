mod native;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::Path;

use crate::tools::ToolContext;

pub fn inspect_document(path: &Path) -> Result<Value> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext == "xlsx" {
        return native::inspect_xlsx(path);
    }
    if ext == "docx" {
        return native::inspect_docx(path);
    }
    if ext == "pptx" {
        return native::inspect_pptx(path);
    }
    Ok(json!({
        "path": path.display().to_string(),
        "format": if ext.is_empty() { "unknown" } else { &ext },
        "kind": "binary",
        "supported": false,
        "capabilities": {
            "preview": false,
            "extract_ir": false,
            "create": false,
            "apply_ops": false,
        },
        "reason": "only .xlsx, .docx, and .pptx are supported in v1"
    }))
}

pub fn extract_ir(path: &Path) -> Result<Value> {
    match extension(path).as_str() {
        "xlsx" => native::extract_xlsx_ir(path),
        "docx" => native::extract_docx_ir(path),
        "pptx" => native::extract_pptx_ir(path),
        _ => bail!("only .xlsx, .docx, and .pptx are supported in v1"),
    }
}

pub fn preview_xlsx(path: &Path, max_rows: usize, max_cols: usize) -> Result<Value> {
    ensure_xlsx(path)?;
    native::preview_xlsx(path, max_rows, max_cols)
}

pub fn create_xlsx(output_path: &Path, spec: &Value) -> Result<Value> {
    ensure_output_xlsx(output_path)?;
    native::create_xlsx(output_path, spec)
}

pub fn create_docx(output_path: &Path, spec: &Value) -> Result<Value> {
    ensure_output_docx(output_path)?;
    native::create_docx(output_path, spec)
}

pub fn apply_xlsx_ops(path: &Path, output_path: Option<&Path>, ops: &Value) -> Result<Value> {
    ensure_xlsx(path)?;
    if let Some(output_path) = output_path {
        ensure_output_xlsx(output_path)?;
    }
    native::apply_xlsx_ops(path, output_path, ops)
}

pub fn preview_docx(path: &Path, max_paragraphs: usize) -> Result<Value> {
    ensure_docx(path)?;
    native::preview_docx(path, max_paragraphs)
}

pub fn apply_docx_ops(path: &Path, output_path: Option<&Path>, ops: &Value) -> Result<Value> {
    ensure_docx(path)?;
    if let Some(output_path) = output_path {
        ensure_output_docx(output_path)?;
    }
    native::apply_docx_ops(path, output_path, ops)
}

pub fn preview_pptx(path: &Path, max_slides: usize) -> Result<Value> {
    ensure_pptx(path)?;
    native::preview_pptx(path, max_slides)
}

pub async fn inspect_document_via_runtime(ctx: &ToolContext, path: &Path) -> Result<Value> {
    let _ = ctx;
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext == "xlsx" {
        return native::inspect_xlsx(path);
    }
    if ext == "docx" {
        return native::inspect_docx(path);
    }
    if ext == "pptx" {
        return native::inspect_pptx(path);
    }
    inspect_document(path)
}

pub async fn extract_ir_via_runtime(ctx: &ToolContext, path: &Path) -> Result<Value> {
    let _ = ctx;
    match extension(path).as_str() {
        "xlsx" => native::extract_xlsx_ir(path),
        "docx" => native::extract_docx_ir(path),
        "pptx" => native::extract_pptx_ir(path),
        _ => bail!("only .xlsx, .docx, and .pptx are supported in v1"),
    }
}

pub async fn preview_xlsx_via_runtime(
    ctx: &ToolContext,
    path: &Path,
    max_rows: usize,
    max_cols: usize,
) -> Result<Value> {
    ensure_xlsx(path)?;
    let _ = ctx;
    native::preview_xlsx(path, max_rows, max_cols)
}

pub async fn create_xlsx_via_runtime(
    ctx: &ToolContext,
    output_path: &Path,
    spec: &Value,
) -> Result<Value> {
    ensure_output_xlsx(output_path)?;
    let _ = ctx;
    native::create_xlsx(output_path, spec)
}

pub async fn create_docx_via_runtime(
    ctx: &ToolContext,
    output_path: &Path,
    spec: &Value,
) -> Result<Value> {
    ensure_output_docx(output_path)?;
    let _ = ctx;
    native::create_docx(output_path, spec)
}

pub async fn apply_xlsx_ops_via_runtime(
    ctx: &ToolContext,
    path: &Path,
    output_path: Option<&Path>,
    ops: &Value,
) -> Result<Value> {
    ensure_xlsx(path)?;
    if let Some(output_path) = output_path {
        ensure_output_xlsx(output_path)?;
    }
    let _ = ctx;
    native::apply_xlsx_ops(path, output_path, ops)
}

pub async fn preview_docx_via_runtime(
    ctx: &ToolContext,
    path: &Path,
    max_paragraphs: usize,
) -> Result<Value> {
    ensure_docx(path)?;
    let _ = ctx;
    native::preview_docx(path, max_paragraphs)
}

pub async fn apply_docx_ops_via_runtime(
    ctx: &ToolContext,
    path: &Path,
    output_path: Option<&Path>,
    ops: &Value,
) -> Result<Value> {
    ensure_docx(path)?;
    if let Some(output_path) = output_path {
        ensure_output_docx(output_path)?;
    }
    let _ = ctx;
    native::apply_docx_ops(path, output_path, ops)
}

pub async fn preview_pptx_via_runtime(
    ctx: &ToolContext,
    path: &Path,
    max_slides: usize,
) -> Result<Value> {
    ensure_pptx(path)?;
    let _ = ctx;
    native::preview_pptx(path, max_slides)
}

fn ensure_xlsx(path: &Path) -> Result<()> {
    match extension(path).as_str() {
        "xlsx" => Ok(()),
        _ => bail!("only .xlsx is supported in v1"),
    }
}

fn ensure_output_xlsx(path: &Path) -> Result<()> {
    ensure_xlsx(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

fn ensure_docx(path: &Path) -> Result<()> {
    match extension(path).as_str() {
        "docx" => Ok(()),
        _ => bail!("only .docx is supported in v1"),
    }
}

fn ensure_output_docx(path: &Path) -> Result<()> {
    ensure_docx(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn ensure_pptx(path: &Path) -> Result<()> {
    match extension(path).as_str() {
        "pptx" => Ok(()),
        _ => bail!("only .pptx is supported in v1"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_docx_ops, apply_xlsx_ops, create_docx, create_xlsx, extract_ir, inspect_document,
        preview_docx, preview_pptx, preview_xlsx,
    };
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn create_and_inspect_xlsx_document() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("report.xlsx");
        let result = create_xlsx(
            &path,
            &json!({
                "sheets": [
                    {
                        "name": "Summary",
                        "cells": [
                            { "addr": "A1", "value": "Task" },
                            { "addr": "B1", "value": "Status" },
                            { "addr": "A2", "value": "Office V1" },
                            { "addr": "B2", "value": "In Progress" }
                        ]
                    }
                ]
            }),
        )
        .expect("create xlsx");
        assert_eq!(
            result.get("sheetCount").and_then(|value| value.as_u64()),
            Some(1)
        );

        let inspect = inspect_document(&path).expect("inspect xlsx");
        assert_eq!(
            inspect.get("supported").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            inspect.get("sheetCount").and_then(|value| value.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn extract_apply_and_preview_xlsx_document() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("workbook.xlsx");
        create_xlsx(
            &path,
            &json!({
                "sheets": [
                    {
                        "name": "Sheet1",
                        "cells": [
                            { "addr": "A1", "value": "Name" },
                            { "addr": "B1", "value": "Score" },
                            { "addr": "A2", "value": "Ada" },
                            { "addr": "B2", "value": 42 }
                        ]
                    }
                ]
            }),
        )
        .expect("seed workbook");

        let ir = extract_ir(&path).expect("extract ir");
        let cells = ir["sheets"][0]["cells"].as_array().expect("cells array");
        assert!(
            cells
                .iter()
                .any(|cell| cell["addr"] == "A2" && cell["value"] == "Ada")
        );

        let output_path = tmp.path().join("workbook.edited.xlsx");
        let apply = apply_xlsx_ops(
            &path,
            Some(&output_path),
            &json!([
                { "op": "set_cell", "sheet": "Sheet1", "addr": "B2", "value": 99 },
                { "op": "set_range", "sheet": "Sheet1", "addr": "A4", "values": [["Name", "Score"], ["Grace", 88]] },
                { "op": "add_sheet", "name": "Notes" },
                { "op": "append_rows", "sheet": "Notes", "rows": [["Ready"], ["Review"]] },
                { "op": "clear_range", "sheet": "Sheet1", "addr": "A2", "end_addr": "A2" }
            ]),
        )
        .expect("apply ops");
        assert_eq!(
            apply.get("sheetCount").and_then(|value| value.as_u64()),
            Some(2)
        );

        let preview = preview_xlsx(&output_path, 10, 5).expect("preview workbook");
        assert_eq!(
            preview.get("sheetCount").and_then(|value| value.as_u64()),
            Some(2)
        );
        assert_eq!(preview["sheets"][0]["rows"][1]["values"][1], json!(99));
        assert_eq!(preview["sheets"][0]["rows"][1]["values"][0], json!(""));
        assert_eq!(preview["sheets"][0]["rows"][2]["values"][0], json!("Name"));
        assert_eq!(preview["sheets"][0]["rows"][3]["values"][0], json!("Grace"));
        assert_eq!(preview["sheets"][1]["rows"][0]["values"][0], json!("Ready"));
    }

    #[test]
    fn create_extract_and_apply_docx_document() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("notes.docx");
        create_docx(
            &path,
            &json!({
                "paragraphs": ["Intro", "Details"]
            }),
        )
        .expect("create docx");

        let inspect = inspect_document(&path).expect("inspect docx");
        assert_eq!(
            inspect.get("supported").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            inspect
                .get("paragraphCount")
                .and_then(|value| value.as_u64()),
            Some(2)
        );

        let ir = extract_ir(&path).expect("extract docx ir");
        assert_eq!(ir["blocks"][0]["text"], json!("Intro"));

        let output_path = tmp.path().join("notes.edited.docx");
        let apply = apply_docx_ops(
            &path,
            Some(&output_path),
            &json!([
                { "op": "replace_paragraph", "target": "p1", "text": "Overview" },
                { "op": "insert_paragraph_after", "target": "p1", "text": "Middle" },
                { "op": "append_paragraph", "text": "Tail" }
            ]),
        )
        .expect("apply docx ops");
        assert_eq!(
            apply.get("paragraphCount").and_then(|value| value.as_u64()),
            Some(4)
        );

        let preview = preview_docx(&output_path, 10).expect("preview docx");
        assert_eq!(preview["paragraphs"][0], json!("Overview"));
        assert_eq!(preview["paragraphs"][1], json!("Middle"));
        assert_eq!(preview["paragraphs"][3], json!("Tail"));
    }

    #[test]
    fn create_docx_splits_multiline_text_into_paragraphs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("multiline.docx");
        let create = create_docx(
            &path,
            &json!({
                "text": "Intro\n\nDetails\nTail"
            }),
        )
        .expect("create multiline docx");

        assert_eq!(
            create
                .get("paragraphCount")
                .and_then(|value| value.as_u64()),
            Some(4)
        );

        let preview = preview_docx(&path, 10).expect("preview multiline docx");
        assert_eq!(preview["paragraphs"][0], json!("Intro"));
        assert_eq!(preview["paragraphs"][1], json!(""));
        assert_eq!(preview["paragraphs"][2], json!("Details"));
        assert_eq!(preview["paragraphs"][3], json!("Tail"));
    }

    #[test]
    fn inspect_extract_and_preview_pptx_document() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("slides.pptx");
        let file = std::fs::File::create(&path).expect("create pptx");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<()>::default();
        zip.start_file("ppt/presentation.xml", options)
            .expect("presentation");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <p:sldIdLst>
    <p:sldId id="256" r:id="rId1"/>
    <p:sldId id="257" r:id="rId2"/>
  </p:sldIdLst>
</p:presentation>"#,
        )
        .expect("write presentation");
        zip.start_file("ppt/_rels/presentation.xml.rels", options)
            .expect("rels");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide2.xml"/>
</Relationships>"#,
        )
        .expect("write rels");
        zip.start_file("ppt/slides/slide1.xml", options)
            .expect("slide1");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp>
        <p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
        <p:txBody><a:p><a:r><a:t>Overview</a:t></a:r></a:p></p:txBody>
      </p:sp>
      <p:sp>
        <p:txBody><a:p><a:r><a:t>Bullet A</a:t></a:r></a:p></p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#,
        )
        .expect("write slide1");
        zip.start_file("ppt/slides/slide2.xml", options)
            .expect("slide2");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp>
        <p:txBody><a:p><a:r><a:t>Second Slide</a:t></a:r></a:p></p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#,
        )
        .expect("write slide2");
        zip.finish().expect("finish zip");

        let inspect = inspect_document(&path).expect("inspect pptx");
        assert_eq!(inspect["slideCount"], json!(2));
        assert_eq!(inspect["capabilities"]["extract_ir"], json!(true));
        assert_eq!(inspect["capabilities"]["create"], json!(false));

        let ir = extract_ir(&path).expect("extract pptx");
        assert_eq!(ir["slides"][0]["title"], json!("Overview"));
        assert_eq!(ir["slides"][0]["bullets"][0], json!("Bullet A"));
        assert_eq!(ir["slides"][1]["title"], json!("Second Slide"));

        let preview = preview_pptx(&path, 10).expect("preview pptx");
        assert_eq!(preview["slides"][0]["title"], json!("Overview"));
        assert_eq!(preview["slides"][1]["title"], json!("Second Slide"));
    }
}
