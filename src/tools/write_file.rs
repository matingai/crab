use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::io::Write;

use crate::tools::{Tool, ToolContext, relative_display, resolve_workspace_path};
use crate::types::{ToolDefinition, object_schema};

pub struct WriteFileTool;

#[derive(Debug, Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
    #[serde(default)]
    append: bool,
}

#[async_trait]
impl Tool for WriteFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "write_file",
            "Write or append UTF-8 text content to a file inside the workspace.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Path relative to the workspace root."
                    },
                    "content": {
                        "type": "string",
                        "description": "Text content to write."
                    },
                    "append": {
                        "type": "boolean",
                        "description": "Append instead of overwriting when true."
                    }
                }),
                &["path", "content"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: WriteFileArgs =
            serde_json::from_value(args).context("invalid write_file arguments")?;
        let path = resolve_workspace_path(&ctx.workspace_root, &args.path)?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        if args.append {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("failed to open {}", path.display()))?;
            file.write_all(args.content.as_bytes())
                .with_context(|| format!("failed to append {}", path.display()))?;
        } else {
            fs::write(&path, args.content.as_bytes())
                .with_context(|| format!("failed to write {}", path.display()))?;
        }

        Ok(format!(
            "path: {}\nmode: {}\nbytes_written: {}",
            relative_display(&ctx.workspace_root, &path),
            if args.append { "append" } else { "overwrite" },
            args.content.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::WriteFileTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    #[tokio::test]
    async fn writes_file_inside_workspace() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = WriteFileTool;
        tool.execute(
            json!({
                "path": "notes/demo.txt",
                "content": "hello"
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
                max_iterations: 4,
                current_session_id: "test-session".to_string(),
                current_delegate_run_id: None,
                delegate_depth: 0,
            },
        )
        .await
        .expect("write should succeed");

        let saved = std::fs::read_to_string(tmp.path().join("notes/demo.txt")).expect("read");
        assert_eq!(saved, "hello");
    }
}
