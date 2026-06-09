use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PDF_HELPER: &str = include_str!("../scripts/pdf_extract.swift");

pub fn inspect_document(path: &Path) -> Result<Value> {
    ensure_pdf(path)?;
    run_helper("inspect", &json!({ "path": path.display().to_string() }))
}

pub fn preview_pdf(path: &Path, max_pages: usize, max_chars_per_page: usize) -> Result<Value> {
    ensure_pdf(path)?;
    run_helper(
        "preview",
        &json!({
            "path": path.display().to_string(),
            "max_pages": max_pages,
            "max_chars_per_page": max_chars_per_page,
        }),
    )
}

pub fn extract_ir(path: &Path, max_pages: usize, max_chars_per_page: usize) -> Result<Value> {
    ensure_pdf(path)?;
    run_helper(
        "extract_ir",
        &json!({
            "path": path.display().to_string(),
            "max_pages": max_pages,
            "max_chars_per_page": max_chars_per_page,
        }),
    )
}

fn run_helper(action: &str, payload: &Value) -> Result<Value> {
    let helper_root = helper_root();
    let script_path = ensure_helper_script(&helper_root)?;
    let module_cache_path = helper_root.join("swift-module-cache");
    fs::create_dir_all(&module_cache_path)
        .with_context(|| format!("failed to create {}", module_cache_path.display()))?;

    let mut child = match Command::new("swift")
        .arg("-module-cache-path")
        .arg(&module_cache_path)
        .arg(&script_path)
        .arg(action)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            bail!("PDF features require Swift to be installed and available on PATH");
        }
        Err(error) => return Err(error).context("failed to launch swift for PDF helper"),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let body = serde_json::to_vec(payload)?;
        use std::io::Write;
        stdin
            .write_all(&body)
            .context("failed to send PDF helper payload")?;
    }

    let output = child
        .wait_with_output()
        .context("failed to wait for PDF helper process")?;
    let stdout = String::from_utf8(output.stdout).context("PDF helper returned non-UTF8 stdout")?;
    if output.status.success() {
        return serde_json::from_str(&stdout).context("failed to parse PDF helper JSON output");
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.contains("no such module 'PDFKit'") {
        bail!(
            "PDF features require Apple's PDFKit framework; this helper currently works on macOS with Swift"
        );
    }
    let helper_output =
        serde_json::from_str::<Value>(&stdout).unwrap_or_else(|_| json!({ "error": stdout }));
    let helper_error = helper_output
        .get("error")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("PDF helper failed");
    if stderr.is_empty() {
        bail!("{helper_error}");
    }
    Err(anyhow!("{helper_error}: {stderr}"))
}

fn helper_root() -> PathBuf {
    std::env::temp_dir()
        .join("hermes-agent-rs")
        .join("pdf-helper")
}

fn ensure_helper_script(helper_root: &Path) -> Result<PathBuf> {
    fs::create_dir_all(helper_root)
        .with_context(|| format!("failed to create {}", helper_root.display()))?;
    let script_path = helper_root.join("pdf_extract.swift");
    let needs_write = fs::read_to_string(&script_path)
        .map(|existing| existing != PDF_HELPER)
        .unwrap_or(true);
    if needs_write {
        fs::write(&script_path, PDF_HELPER)
            .with_context(|| format!("failed to write {}", script_path.display()))?;
    }
    Ok(script_path)
}

fn ensure_pdf(path: &Path) -> Result<()> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "pdf" => Ok(()),
        _ => bail!("only .pdf is supported"),
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_ir, inspect_document, preview_pdf};
    use serde_json::json;
    use std::fs;
    use std::process::Command;

    fn pdfkit_available() -> bool {
        Command::new("swift")
            .arg("-e")
            .arg("import PDFKit")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn build_test_pdf(text: &str) -> Vec<u8> {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        let stream = format!("BT\n/F1 24 Tf\n72 100 Td\n({escaped}) Tj\nET\n");
        let objects = [
            "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_string(),
            "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_string(),
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n".to_string(),
            format!(
                "4 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\n",
                stream.len(),
                stream
            ),
            "5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n"
                .to_string(),
        ];

        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0usize];
        for object in &objects {
            offsets.push(pdf.len());
            pdf.extend_from_slice(object.as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Root 1 0 R /Size {} >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref_offset
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn inspects_and_extracts_pdf_text() {
        if !pdfkit_available() {
            eprintln!("skipping PDF helper test because Swift PDFKit is not available");
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("sample.pdf");
        fs::write(&path, build_test_pdf("Hello PDF from test")).expect("write pdf");

        let inspect = inspect_document(&path).expect("inspect");
        assert_eq!(inspect["format"], json!("pdf"));
        assert_eq!(inspect["pageCount"], json!(1));
        assert_eq!(inspect["capabilities"]["extract_ir"], json!(true));

        let preview = preview_pdf(&path, 4, 400).expect("preview");
        assert_eq!(preview["type"], json!("pdf_preview"));
        assert_eq!(preview["pageCount"], json!(1));
        let preview_text = preview["pages"][0]["text"].as_str().expect("preview text");
        assert!(preview_text.contains("Hello PDF from test"));

        let ir = extract_ir(&path, 4, 4000).expect("extract");
        assert_eq!(ir["type"], json!("pdf"));
        let ir_text = ir["pages"][0]["text"].as_str().expect("ir text");
        assert!(ir_text.contains("Hello PDF from test"));
    }
}
