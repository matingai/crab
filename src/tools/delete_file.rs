use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;

use crate::tools::{
    Tool, ToolContext, ensure_clean_worktree_path, relative_display, resolve_existing_path,
};
use crate::types::{ToolDefinition, object_schema};

pub struct DeleteFileTool;

#[derive(Debug, Deserialize)]
struct DeleteFileArgs {
    path: String,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    allow_dirty: bool,
}

#[async_trait]
impl Tool for DeleteFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "delete_file",
            "Delete a file or directory inside the workspace.",
            object_schema(
                json!({
                    "path": { "type": "string", "description": "Relative path to delete." },
                    "recursive": { "type": "boolean", "description": "Delete directories recursively when true." },
                    "allow_dirty": { "type": "boolean", "description": "Allow deleting a path that has uncommitted Git worktree changes." }
                }),
                &["path"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: DeleteFileArgs =
            serde_json::from_value(args).context("invalid delete_file arguments")?;
        let path = resolve_existing_path(&ctx.workspace_root, &args.path)?;
        ensure_clean_worktree_path(&ctx.workspace_root, &path, "delete_file", args.allow_dirty)?;
        let display = relative_display(&ctx.workspace_root, &path);

        if path.is_dir() {
            if !args.recursive {
                bail!("`{display}` is a directory; set `recursive=true` to remove it");
            }
            fs::remove_dir_all(&path)
                .with_context(|| format!("failed to delete directory {}", path.display()))?;
            return Ok(format!("deleted directory: {display}"));
        }

        fs::remove_file(&path).with_context(|| format!("failed to delete {}", path.display()))?;
        Ok(format!("deleted file: {display}"))
    }
}

#[cfg(test)]
mod tests {
    use super::DeleteFileTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    #[tokio::test]
    async fn deletes_file_inside_workspace() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("demo.txt");
        std::fs::write(&path, "hello").expect("write");
        let tool = DeleteFileTool;

        let output = tool
            .execute(
                json!({ "path": "demo.txt" }),
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
            .expect("delete should succeed");

        assert!(output.contains("deleted file: demo.txt"));
        assert!(!path.exists());
    }
}
