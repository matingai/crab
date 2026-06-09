use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::plugins::{PluginToolSpec, execute_plugin_tool};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct PluginTool {
    spec: PluginToolSpec,
}

impl PluginTool {
    pub fn new(spec: PluginToolSpec) -> Self {
        Self { spec }
    }
}

#[async_trait]
impl Tool for PluginTool {
    fn definition(&self) -> ToolDefinition {
        let mut schema = self.spec.schema.clone();
        if !schema.is_object() {
            schema = object_schema(json!({ "input": { "type": "string" } }), &[]);
        }
        ToolDefinition::function(
            self.spec.tool_name.clone(),
            if self.spec.description.trim().is_empty() {
                format!("Plugin tool provided by `{}`.", self.spec.plugin_name)
            } else {
                self.spec.description.clone()
            },
            schema,
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let payload = json!({
            "arguments": args,
            "session_id": ctx.current_session_id,
            "workspace_root": ctx.workspace_root.display().to_string(),
            "data_dir": ctx.data_dir.display().to_string(),
            "provider": ctx.provider_id,
            "model": ctx.model,
        });
        execute_plugin_tool(&self.spec, &payload).await
    }
}
