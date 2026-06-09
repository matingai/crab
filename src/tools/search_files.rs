use anyhow::{Context, Result};
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;

use walkdir::WalkDir;

use crate::tools::{Tool, ToolContext, relative_display, resolve_existing_path, truncated};
use crate::types::{ToolDefinition, object_schema};

pub struct SearchFilesTool;

#[derive(Debug, Deserialize)]
struct SearchFilesArgs {
    pattern: String,
    path: Option<String>,
    #[serde(default = "default_max_results")]
    max_results: usize,
}

fn default_max_results() -> usize {
    25
}

#[async_trait]
impl Tool for SearchFilesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "search_files",
            "Search text files in the workspace using a Rust regular expression.",
            object_schema(
                json!({
                    "pattern": {
                        "type": "string",
                        "description": "Rust regex pattern to search for."
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional file or directory path relative to the workspace root."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of matches to return.",
                        "minimum": 1,
                        "maximum": 200
                    }
                }),
                &["pattern"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: SearchFilesArgs =
            serde_json::from_value(args).context("invalid search_files arguments")?;
        let base = args.path.unwrap_or_else(|| ".".to_string());
        let path = resolve_existing_path(&ctx.workspace_root, &base)?;
        let regex = Regex::new(&args.pattern)
            .with_context(|| format!("invalid regex pattern `{}`", args.pattern))?;

        let mut results = Vec::new();
        if path.is_file() {
            search_one_file(
                &path,
                &ctx.workspace_root,
                &regex,
                args.max_results,
                &mut results,
            )?;
        } else {
            for entry in WalkDir::new(&path).into_iter().filter_map(Result::ok) {
                if !entry.file_type().is_file() {
                    continue;
                }
                search_one_file(
                    entry.path(),
                    &ctx.workspace_root,
                    &regex,
                    args.max_results,
                    &mut results,
                )?;
                if results.len() >= args.max_results {
                    break;
                }
            }
        }

        if results.is_empty() {
            return Ok(format!(
                "no matches for `{}` under {}",
                args.pattern,
                relative_display(&ctx.workspace_root, &path)
            ));
        }

        Ok(results.join("\n"))
    }
}

fn search_one_file(
    path: &std::path::Path,
    workspace_root: &std::path::Path,
    regex: &Regex,
    max_results: usize,
    results: &mut Vec<String>,
) -> Result<()> {
    if results.len() >= max_results {
        return Ok(());
    }

    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    for (index, line) in text.lines().enumerate() {
        if regex.is_match(line) {
            results.push(format!(
                "{}:{}: {}",
                relative_display(workspace_root, path),
                index + 1,
                truncated(line, 240)
            ));
            if results.len() >= max_results {
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::SearchFilesTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    #[tokio::test]
    async fn search_returns_matching_lines() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("demo.txt"), "alpha\nbeta\nalpha beta\n").expect("write");

        let tool = SearchFilesTool;
        let output = tool
            .execute(
                json!({
                    "pattern": "alpha",
                    "max_results": 10
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
            .expect("search should succeed");

        assert!(output.contains("demo.txt:1"));
        assert!(output.contains("demo.txt:3"));
    }
}
