use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use walkdir::WalkDir;

use crate::tools::{Tool, ToolContext, relative_display, resolve_existing_path};
use crate::types::{ToolDefinition, object_schema};

pub struct ListFilesTool;

#[derive(Debug, Deserialize)]
struct ListFilesArgs {
    path: Option<String>,
    #[serde(default)]
    recursive: bool,
    #[serde(default = "default_max_results")]
    max_results: usize,
}

fn default_max_results() -> usize {
    200
}

#[async_trait]
impl Tool for ListFilesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "list_files",
            "List files and directories inside the workspace.",
            object_schema(
                json!({
                    "path": { "type": "string", "description": "Optional relative path to list." },
                    "recursive": { "type": "boolean", "description": "List recursively when true." },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 500 }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: ListFilesArgs =
            serde_json::from_value(args).context("invalid list_files arguments")?;
        let base = args.path.unwrap_or_else(|| ".".to_string());
        let root = resolve_existing_path(&ctx.workspace_root, &base)?;

        let mut items = Vec::new();
        if args.recursive {
            for entry in WalkDir::new(&root).into_iter().filter_map(Result::ok) {
                if entry.path() == root {
                    continue;
                }
                let suffix = if entry.file_type().is_dir() { "/" } else { "" };
                items.push(format!(
                    "{}{}",
                    relative_display(&ctx.workspace_root, entry.path()),
                    suffix
                ));
                if items.len() >= args.max_results {
                    break;
                }
            }
        } else {
            let mut entries = std::fs::read_dir(&root)
                .with_context(|| format!("failed to read {}", root.display()))?
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.path());
            for entry in entries.into_iter().take(args.max_results) {
                let path = entry.path();
                let suffix = if path.is_dir() { "/" } else { "" };
                items.push(format!(
                    "{}{}",
                    relative_display(&ctx.workspace_root, &path),
                    suffix
                ));
            }
        }

        if items.is_empty() {
            return Ok(format!(
                "no files found under {}",
                relative_display(&ctx.workspace_root, &root)
            ));
        }
        Ok(items.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::ListFilesTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    #[tokio::test]
    async fn lists_workspace_entries() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("src")).expect("mkdir");
        std::fs::write(tmp.path().join("src/main.rs"), "fn main() {}").expect("write");

        let tool = ListFilesTool;
        let output = tool
            .execute(
                json!({ "recursive": true }),
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
            .expect("list should succeed");

        assert!(output.contains("src/"));
        assert!(output.contains("src/main.rs"));
    }
}
