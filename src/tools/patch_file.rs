use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;

use crate::tools::{
    Tool, ToolContext, ensure_clean_worktree_path, relative_display, resolve_existing_path,
};
use crate::types::{ToolDefinition, object_schema};

pub struct PatchFileTool;

#[derive(Debug, Deserialize)]
struct PatchFileArgs {
    path: String,
    old_text: String,
    new_text: String,
    #[serde(default)]
    replace_all: bool,
    #[serde(default)]
    allow_dirty: bool,
}

#[async_trait]
impl Tool for PatchFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "patch_file",
            "Patch a workspace file by replacing exact text.",
            object_schema(
                json!({
                    "path": { "type": "string", "description": "Relative workspace path." },
                    "old_text": { "type": "string", "description": "Exact text to replace." },
                    "new_text": { "type": "string", "description": "Replacement text." },
                    "replace_all": { "type": "boolean", "description": "Replace all matches when true." },
                    "allow_dirty": { "type": "boolean", "description": "Allow patching a file that has uncommitted Git worktree changes." }
                }),
                &["path", "old_text", "new_text"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: PatchFileArgs =
            serde_json::from_value(args).context("invalid patch_file arguments")?;
        if args.old_text.is_empty() {
            bail!("patch_file requires non-empty `old_text`");
        }

        let path = resolve_existing_path(&ctx.workspace_root, &args.path)?;
        ensure_clean_worktree_path(&ctx.workspace_root, &path, "patch_file", args.allow_dirty)?;
        let existing = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        let count = existing.matches(&args.old_text).count();
        if count == 0 {
            bail!(
                "old_text was not found in {}",
                relative_display(&ctx.workspace_root, &path)
            );
        }
        if count > 1 && !args.replace_all {
            bail!("old_text matched {count} times; set `replace_all=true` to replace all matches");
        }

        let updated = if args.replace_all {
            existing.replace(&args.old_text, &args.new_text)
        } else {
            existing.replacen(&args.old_text, &args.new_text, 1)
        };

        fs::write(&path, updated.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;

        Ok(format!(
            "patched {}\nreplacements: {}",
            relative_display(&ctx.workspace_root, &path),
            if args.replace_all { count } else { 1 }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::PatchFileTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    #[tokio::test]
    async fn patches_file_content() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("demo.txt"), "alpha beta").expect("write");
        let tool = PatchFileTool;
        let output = tool
            .execute(
                json!({
                    "path": "demo.txt",
                    "old_text": "beta",
                    "new_text": "gamma"
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
            .expect("patch should succeed");

        assert!(output.contains("patched demo.txt"));
        let content = std::fs::read_to_string(tmp.path().join("demo.txt")).expect("read");
        assert_eq!(content, "alpha gamma");
    }
}
