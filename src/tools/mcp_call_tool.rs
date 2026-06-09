use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::call_server_tool;
use crate::tools::{Tool, ToolContext, truncated};
use crate::types::{ToolDefinition, object_schema};

pub struct McpCallTool;

#[derive(Debug, Deserialize)]
struct McpCallArgs {
    server_name: String,
    tool_name: String,
    #[serde(default)]
    arguments: Value,
}

#[async_trait]
impl Tool for McpCallTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "mcp_call",
            "Call a tool exposed by a configured MCP server.",
            object_schema(
                json!({
                    "server_name": {
                        "type": "string",
                        "description": "Name of the configured MCP server."
                    },
                    "tool_name": {
                        "type": "string",
                        "description": "Remote MCP tool name."
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Arguments passed to the remote MCP tool."
                    }
                }),
                &["server_name", "tool_name"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: McpCallArgs =
            serde_json::from_value(args).context("invalid mcp_call arguments")?;
        let server_name = args.server_name.trim();
        if server_name.is_empty() {
            bail!("server_name cannot be empty");
        }
        let tool_name = args.tool_name.trim();
        if tool_name.is_empty() {
            bail!("tool_name cannot be empty");
        }

        let output =
            call_server_tool(&ctx.data_dir, server_name, tool_name, args.arguments).await?;
        Ok(format!(
            "server: {}\ntool: {}\nresult:\n{}",
            server_name,
            tool_name,
            truncated(output, 12_000)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::McpCallTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    #[tokio::test]
    async fn calls_mock_mcp_tool() {
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

        let output = McpCallTool
            .execute(
                json!({
                    "server_name": "docs",
                    "tool_name": "search_docs",
                    "arguments": { "query": "workflow" }
                }),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: tmp.path().to_path_buf(),
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
            .expect("call tool");

        assert!(output.contains("search_docs"));
        assert!(output.contains("workflow"));
    }
}
