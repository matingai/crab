use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::process::Command;

use crate::tools::{Tool, ToolContext, relative_display, resolve_workspace_path, truncated};
use crate::types::{ToolDefinition, object_schema};

pub struct GitLogTool;

#[derive(Debug, Deserialize)]
struct GitLogArgs {
    path: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    10
}

#[async_trait]
impl Tool for GitLogTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "git_log",
            "Show recent git commits for the workspace repository.",
            object_schema(
                json!({
                    "path": { "type": "string", "description": "Optional relative path filter." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Number of commits to show." }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: GitLogArgs = serde_json::from_value(args).context("invalid git_log arguments")?;
        ensure_git_repo(&ctx.workspace_root)?;

        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&ctx.workspace_root)
            .arg("log")
            .arg(format!("-n{}", args.limit))
            .arg("--date=short")
            .arg("--pretty=format:%h %ad %s");

        if let Some(path) = args.path.as_deref() {
            let resolved = resolve_workspace_path(&ctx.workspace_root, path)?;
            let relative = relative_display(&ctx.workspace_root, &resolved);
            command.arg("--").arg(relative);
        }

        let output = command.output().context("failed to run git log")?;
        if !output.status.success() {
            bail!(
                "git log failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return Ok("no commits found".to_string());
        }
        Ok(truncated(stdout, 12_000))
    }
}

fn ensure_git_repo(root: &std::path::Path) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::GitLogTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;
    use std::process::Command;

    #[tokio::test]
    async fn shows_git_log() {
        let tmp = tempfile::tempdir().expect("tempdir");
        Command::new("git")
            .arg("init")
            .arg(tmp.path())
            .output()
            .expect("git init");
        Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(["config", "user.email", "test@example.com"])
            .output()
            .expect("git config");
        Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(["config", "user.name", "Test User"])
            .output()
            .expect("git config");
        std::fs::write(tmp.path().join("README.md"), "hi").expect("write");
        Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(["add", "."])
            .output()
            .expect("git add");
        Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(["commit", "-m", "init"])
            .output()
            .expect("git commit");

        let tool = GitLogTool;
        let output = tool
            .execute(
                json!({ "limit": 1 }),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: tmp.path().join(".data"),
                    shell_enabled: false,
                    skill_platform: "cli".to_string(),
                    provider_id: "openai".to_string(),
                    model: "test-model".to_string(),
                    base_url: "https://example.invalid/v1".to_string(),
                    api_key: None,
                    api_mode: crate::llm::ApiMode::ChatCompletions,
                    max_iterations: 4,
                    current_session_id: "test-session".to_string(),
                    current_delegate_run_id: None,
                    delegate_depth: 0,
                },
            )
            .await
            .expect("git log should succeed");

        assert!(output.contains("init"));
    }
}
