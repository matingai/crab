use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::office;
use crate::tools::{Tool, ToolContext, resolve_existing_path};
use crate::types::{ToolDefinition, object_schema};

pub struct OfficeInspectTool;

#[derive(Debug, Deserialize)]
struct OfficeInspectArgs {
    path: String,
}

#[async_trait]
impl Tool for OfficeInspectTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "office_inspect",
            "Inspect an Office-like document and report what preview/extract/edit capabilities are available. V1 supports .xlsx, .docx, and .pptx.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Path relative to the workspace root."
                    }
                }),
                &["path"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: OfficeInspectArgs =
            serde_json::from_value(args).context("invalid office_inspect arguments")?;
        let path = resolve_existing_path(&ctx.workspace_root, &args.path)?;
        let result = office::inspect_document_via_runtime(ctx, &path).await?;
        Ok(serde_json::to_string_pretty(&result)?)
    }
}
