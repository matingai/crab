use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use crate::browser_backend::{
    ActiveBrowserBackend, electron_devtools_upload, resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::render_session_page;

pub struct BrowserUploadFileTool;

#[derive(Debug, Deserialize)]
struct BrowserUploadFileArgs {
    reference: Option<String>,
    #[serde(rename = "ref")]
    ref_alias: Option<String>,
    file_path: Option<String>,
    file_paths: Option<Vec<String>>,
    timeout_seconds: Option<u64>,
    max_chars: Option<usize>,
}

impl BrowserUploadFileArgs {
    fn reference(&self) -> Result<&str> {
        match (self.reference.as_deref(), self.ref_alias.as_deref()) {
            (Some(reference), Some(ref_alias)) if reference != ref_alias => {
                bail!("browser_upload_file received conflicting `reference` and `ref` values")
            }
            (Some(reference), _) => Ok(reference),
            (_, Some(ref_alias)) => Ok(ref_alias),
            (None, None) => bail!("browser_upload_file requires a non-empty `reference` or `ref`"),
        }
    }
}

#[async_trait]
impl Tool for BrowserUploadFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_upload_file",
            "Attach one or more local files to a file input element in the current browser page.",
            object_schema(
                json!({
                    "reference": { "type": "string" },
                    "ref": { "type": "string" },
                    "file_path": { "type": "string" },
                    "file_paths": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60
                    },
                    "max_chars": {
                        "type": "integer",
                        "minimum": 200,
                        "maximum": 16000
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserUploadFileArgs =
            serde_json::from_value(args).context("invalid browser_upload_file arguments")?;
        let reference = args.reference()?.trim();
        if reference.is_empty() {
            bail!("browser_upload_file requires a non-empty `reference` or `ref`");
        }
        let files = normalize_files(ctx, args.file_path.as_deref(), args.file_paths.as_deref())?;
        if files.is_empty() {
            bail!("browser_upload_file requires `file_path` or `file_paths`");
        }
        let timeout_seconds = args.timeout_seconds.unwrap_or(20).clamp(1, 60);
        let max_chars = args.max_chars.unwrap_or(2_000).clamp(200, 16_000);

        let backend = resolve_active_backend(ctx).await?;
        let refreshed = match backend {
            ActiveBrowserBackend::AgentBrowser => {
                bail!(
                    "browser_upload_file is currently supported only when `browser_backend` is `electron_devtools`"
                )
            }
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_upload(ctx, reference, &files, 6_000, timeout_seconds).await?
            }
        };

        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let mut session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        session.current = refreshed;
        session.set_focus(Some(reference.to_string()));
        session.log_console(format!(
            "uploaded {} file(s) into {}",
            files.len(),
            reference
        ));
        store.save(&ctx.current_session_id, &session)?;

        Ok(format!(
            "uploaded {} file(s) into {}\n{}",
            files.len(),
            reference,
            render_session_page(&session, max_chars)
        ))
    }
}

fn normalize_files(
    ctx: &ToolContext,
    file_path: Option<&str>,
    file_paths: Option<&[String]>,
) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    if let Some(path) = file_path {
        if !path.trim().is_empty() {
            paths.push(path.trim().to_string());
        }
    }
    if let Some(items) = file_paths {
        for item in items {
            if !item.trim().is_empty() {
                paths.push(item.trim().to_string());
            }
        }
    }

    let mut resolved = Vec::new();
    for item in paths {
        let path = resolve_file_path(&ctx.workspace_root, &item);
        if !path.is_file() {
            bail!(
                "browser_upload_file could not find file `{}`",
                path.display()
            );
        }
        resolved.push(path.to_string_lossy().to_string());
    }
    Ok(resolved)
}

fn resolve_file_path(workspace_root: &Path, value: &str) -> PathBuf {
    let candidate = PathBuf::from(value);
    if candidate.is_absolute() {
        return candidate;
    }
    workspace_root.join(candidate)
}
