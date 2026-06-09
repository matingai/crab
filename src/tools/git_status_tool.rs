use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

use crate::tools::{Tool, ToolContext, relative_display, resolve_workspace_path, truncated};
use crate::types::{ToolDefinition, object_schema};

pub struct GitStatusTool;

#[derive(Debug, Deserialize)]
struct GitStatusArgs {
    path: Option<String>,
}

#[async_trait]
impl Tool for GitStatusTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "git_status",
            "Show git status for the workspace repository.",
            object_schema(
                json!({
                    "path": { "type": "string", "description": "Optional relative path filter." }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: GitStatusArgs =
            serde_json::from_value(args).context("invalid git_status arguments")?;
        ensure_git_repo(&ctx.workspace_root)?;

        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&ctx.workspace_root)
            .arg("status")
            .arg("--short")
            .arg("--branch");

        let display_path = if let Some(path) = args.path.as_deref() {
            let resolved = resolve_workspace_path(&ctx.workspace_root, path)?;
            let relative = relative_display(&ctx.workspace_root, &resolved);
            command.arg("--").arg(&relative);
            Some(relative)
        } else {
            None
        };

        let output = command.output().context("failed to run git status")?;
        if !output.status.success() {
            bail!(
                "git status failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return Ok(match display_path {
                Some(path) => format!("git status clean for {path}"),
                None => "git status clean".to_string(),
            });
        }

        Ok(truncated(stdout, 12_000))
    }
}

fn ensure_git_repo(root: &PathBuf) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .context("failed to run git rev-parse")?;
    if output.status.success() {
        return Ok(());
    }
    bail!("workspace is not inside a git repository")
}
