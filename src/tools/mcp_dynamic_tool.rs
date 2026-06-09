use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::mcp::{McpCachedInspection, McpToolDescriptor, call_server_tool, local_tool_name};
use crate::tools::{Tool, ToolContext, truncated};
use crate::types::ToolDefinition;

pub struct McpDynamicTool {
    server_name: String,
    remote_tool_name: String,
    local_tool_name: String,
    description: String,
    input_schema: Value,
}

impl McpDynamicTool {
    pub fn from_cached_tool(server_name: &str, tool: McpToolDescriptor) -> Self {
        let remote_tool_name = tool.name;
        let local_tool_name = local_tool_name(server_name, &remote_tool_name);
        let description = if tool.description.trim().is_empty() {
            format!("Proxy for MCP server `{server_name}` remote tool `{remote_tool_name}`.")
        } else {
            format!(
                "{}\n\nProxy for MCP server `{server_name}` remote tool `{remote_tool_name}`.",
                tool.description
            )
        };

        Self {
            server_name: server_name.to_string(),
            remote_tool_name,
            local_tool_name,
            description,
            input_schema: tool.input_schema,
        }
    }

    pub fn from_cached_inspection(inspection: &McpCachedInspection) -> Vec<Self> {
        inspection
            .tools
            .iter()
            .cloned()
            .map(|tool| Self::from_cached_tool(&inspection.server_name, tool))
            .collect()
    }
}

#[async_trait]
impl Tool for McpDynamicTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            self.local_tool_name.clone(),
            self.description.clone(),
            self.input_schema.clone(),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let output = call_server_tool(
            &ctx.data_dir,
            &self.server_name,
            &self.remote_tool_name,
            args,
        )
        .await?;
        Ok(format!(
            "server: {}\ntool: {}\nproxy: {}\nresult:\n{}",
            self.server_name,
            self.remote_tool_name,
            self.local_tool_name,
            truncated(output, 12_000)
        ))
    }
}
