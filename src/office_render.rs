use anyhow::{Context, Result, bail};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use tokio::process::Command;
use tokio::time::timeout;

use crate::tools::ToolContext;

const OFFICE_RENDER_VERSION: &str = "v3-office2pdf-only";
const OFFICE2PDF_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct RenderedPdf {
    pub output_path: PathBuf,
    pub cached: bool,
}

pub async fn render_pdf_via_runtime(ctx: &ToolContext, path: &Path) -> Result<RenderedPdf> {
    office_format(path)?;
    let cache_root = cache_root(ctx);
    let cache_key = cache_key(path)?;
    let output_dir = cache_root.join(&cache_key);
    let output_path = output_dir.join(output_pdf_name(path)?);

    if output_path
        .metadata()
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
    {
        return Ok(RenderedPdf {
            output_path,
            cached: true,
        });
    }

    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;

    render_pdf_via_office2pdf(path, &output_path).await?;

    Ok(RenderedPdf {
        output_path,
        cached: false,
    })
}

async fn render_pdf_via_office2pdf(path: &Path, output_path: &Path) -> Result<()> {
    let executable = std::env::current_exe().context("failed to resolve current executable")?;
    let mut command = Command::new(executable);
    command
        .arg("office2-pdf-render")
        .arg(path)
        .arg(output_path)
        .kill_on_drop(true);

    let outcome = timeout(OFFICE2PDF_TIMEOUT, command.output())
        .await
        .context("office2pdf render timed out")?
        .context("failed to run office2pdf render helper")?;
    if !outcome.status.success() {
        let stdout = String::from_utf8_lossy(&outcome.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&outcome.stderr).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        if detail.is_empty() {
            bail!("office2pdf render failed with status {}", outcome.status);
        }
        bail!("office2pdf render failed: {detail}");
    }
    if !output_path.exists() {
        bail!(
            "office2pdf render completed but no pdf was produced at {}",
            output_path.display()
        );
    }
    Ok(())
}

fn cache_root(ctx: &ToolContext) -> PathBuf {
    let data_root = if ctx.data_dir.starts_with(&ctx.workspace_root) {
        ctx.data_dir.clone()
    } else {
        ctx.workspace_root.join(".hermes-agent-rs")
    };
    data_root.join("cache").join("office-pdf")
}

fn cache_key(path: &Path) -> Result<String> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_millis())
        .unwrap_or_default();
    let canonical = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string();
    let mut hasher = DefaultHasher::new();
    OFFICE_RENDER_VERSION.hash(&mut hasher);
    canonical.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    modified.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn output_pdf_name(path: &Path) -> Result<String> {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .context("file is missing a valid stem")?;
    Ok(format!("{stem}.pdf"))
}

fn office_format(path: &Path) -> Result<OfficeFormat> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "docx" => Ok(OfficeFormat::Docx),
        "xlsx" => Ok(OfficeFormat::Xlsx),
        "pptx" => Ok(OfficeFormat::Pptx),
        _ => bail!("only .docx, .xlsx, and .pptx are supported"),
    }
}

#[derive(Debug, Clone, Copy)]
enum OfficeFormat {
    Docx,
    Xlsx,
    Pptx,
}
