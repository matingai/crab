use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

use crate::tools::{Tool, ToolContext, relative_display, resolve_workspace_path, truncated};
use crate::types::{ToolDefinition, object_schema};

pub struct GitDiffTool;

#[derive(Debug, Deserialize)]
struct GitDiffArgs {
    path: Option<String>,
    #[serde(default)]
    staged: bool,
}

#[async_trait]
impl Tool for GitDiffTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "git_diff",
            "Show git diff for the workspace repository.",
            object_schema(
                json!({
                    "path": { "type": "string", "description": "Optional relative path filter." },
                    "staged": { "type": "boolean", "description": "Show staged diff when true." }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: GitDiffArgs =
            serde_json::from_value(args).context("invalid git_diff arguments")?;
        ensure_git_repo(&ctx.workspace_root)?;

        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&ctx.workspace_root)
            .arg("diff")
            .arg("--no-ext-diff")
            .arg("--no-color");

        if args.staged {
            command.arg("--cached");
        }

        let display_path = if let Some(path) = args.path.as_deref() {
            let resolved = resolve_workspace_path(&ctx.workspace_root, path)?;
            let relative = relative_display(&ctx.workspace_root, &resolved);
            command.arg("--").arg(&relative);
            Some(relative)
        } else {
            None
        };

        let output = command.output().context("failed to run git diff")?;
        if !output.status.success() {
            bail!(
                "git diff failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return Ok(match display_path {
                Some(path) => {
                    if args.staged {
                        format!("no staged diff for {path}")
                    } else {
                        format!("no diff for {path}")
                    }
                }
                None => {
                    if args.staged {
                        "no staged diff".to_string()
                    } else {
                        "no diff".to_string()
                    }
                }
            });
        }

        Ok(truncated(stdout, 20_000))
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
