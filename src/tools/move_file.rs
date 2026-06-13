use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;

use crate::tools::{
    Tool, ToolContext, ensure_clean_worktree_path, relative_display, resolve_existing_path,
    resolve_workspace_path,
};
use crate::types::{ToolDefinition, object_schema};

pub struct MoveFileTool;

#[derive(Debug, Deserialize)]
struct MoveFileArgs {
    source_path: String,
    destination_path: String,
    #[serde(default)]
    overwrite: bool,
    #[serde(default)]
    allow_dirty: bool,
}

#[async_trait]
impl Tool for MoveFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "move_file",
            "Move or rename a file or directory inside the workspace.",
            object_schema(
                json!({
                    "source_path": { "type": "string", "description": "Existing relative source path." },
                    "destination_path": { "type": "string", "description": "Destination relative path." },
                    "overwrite": { "type": "boolean", "description": "Overwrite destination when true." },
                    "allow_dirty": { "type": "boolean", "description": "Allow moving from or overwriting paths that have uncommitted Git worktree changes." }
                }),
                &["source_path", "destination_path"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: MoveFileArgs =
            serde_json::from_value(args).context("invalid move_file arguments")?;
        let source = resolve_existing_path(&ctx.workspace_root, &args.source_path)?;
        let destination = resolve_workspace_path(&ctx.workspace_root, &args.destination_path)?;
        ensure_clean_worktree_path(
            &ctx.workspace_root,
            &source,
            "move_file source",
            args.allow_dirty,
        )?;
        if destination.exists() {
            ensure_clean_worktree_path(
                &ctx.workspace_root,
                &destination,
                "move_file destination overwrite",
                args.allow_dirty,
            )?;
        }

        if destination.exists() {
            if !args.overwrite {
                bail!(
                    "destination already exists: {}",
                    relative_display(&ctx.workspace_root, &destination)
                );
            }
            if destination.is_dir() {
                fs::remove_dir_all(&destination).with_context(|| {
                    format!(
                        "failed to remove existing directory {}",
                        destination.display()
                    )
                })?;
            } else {
                fs::remove_file(&destination).with_context(|| {
                    format!("failed to remove existing file {}", destination.display())
                })?;
            }
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        fs::rename(&source, &destination).with_context(|| {
            format!(
                "failed to move {} to {}",
                source.display(),
                destination.display()
            )
        })?;

        Ok(format!(
            "moved {} -> {}",
            relative_display(&ctx.workspace_root, &source),
            relative_display(&ctx.workspace_root, &destination)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::MoveFileTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    #[tokio::test]
    async fn moves_file_inside_workspace() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("demo.txt");
        let destination = tmp.path().join("nested/out.txt");
        std::fs::write(&source, "hello").expect("write");
        let tool = MoveFileTool;

        let output = tool
            .execute(
                json!({
                    "source_path": "demo.txt",
                    "destination_path": "nested/out.txt"
                }),
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
            .expect("move should succeed");

        assert!(output.contains("moved demo.txt -> nested/out.txt"));
        assert!(!source.exists());
        assert_eq!(std::fs::read_to_string(destination).expect("read"), "hello");
    }
}
