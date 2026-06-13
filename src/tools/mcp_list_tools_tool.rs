use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::list_server_tools;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct McpListToolsTool;

#[derive(Debug, Deserialize)]
struct McpListToolsArgs {
    server_name: String,
}

#[async_trait]
impl Tool for McpListToolsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "mcp_list_tools",
            "List tools exposed by a configured MCP server.",
            object_schema(
                json!({
                    "server_name": {
                        "type": "string",
                        "description": "Name of the configured MCP server."
                    }
                }),
                &["server_name"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: McpListToolsArgs =
            serde_json::from_value(args).context("invalid mcp_list_tools arguments")?;
        let server_name = args.server_name.trim();
        if server_name.is_empty() {
            bail!("server_name cannot be empty");
        }

        let tools = list_server_tools(&ctx.data_dir, server_name).await?;
        if tools.is_empty() {
            return Ok(format!("server: {server_name}\ntools: 0"));
        }

        let lines = tools
            .into_iter()
            .map(|tool| {
                format!(
                    "- {}: {}\n  schema: {}",
                    tool.name, tool.description, tool.input_schema
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(format!(
            "server: {server_name}\ntools: {}\n{}",
            lines.lines().filter(|line| line.starts_with("- ")).count(),
            lines
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::McpListToolsTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    #[tokio::test]
    async fn lists_mock_mcp_tools() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            r#"mcp:
  servers:
    - name: docs
      command: __mock_mcp_server__
"#,
        )
        .expect("write config");

        let output = McpListToolsTool
            .execute(
                json!({ "server_name": "docs" }),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: tmp.path().to_path_buf(),
                    shell_enabled: false,
                    skill_platform: "cli".to_string(),
                    provider_id: "openai".to_string(),
                    model: "test-model".to_string(),
                    base_url: "https://example.invalid/v1".to_string(),
                    api_key: None,
                    api_mode: crate::llm::ApiMode::ChatCompletions,
                    worker_model: None,
                    max_iterations: 4,
                    current_session_id: "test-session".to_string(),
                    current_delegate_run_id: None,
                    delegate_depth: 0,
                },
            )
            .await
            .expect("list tools");

        assert!(output.contains("search_docs"));
        assert!(output.contains("read_doc"));
    }
}
