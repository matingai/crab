use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::office;
use crate::tools::{Tool, ToolContext, resolve_existing_path};
use crate::types::{ToolDefinition, object_schema};

pub struct OfficeExtractIrTool;

#[derive(Debug, Deserialize)]
struct OfficeExtractIrArgs {
    path: String,
}

#[async_trait]
impl Tool for OfficeExtractIrTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "office_extract_ir",
            "Extract a structured intermediate representation (IR) from an Office document so a model can inspect or modify it. V1 supports .xlsx, .docx, and .pptx.",
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
        let args: OfficeExtractIrArgs =
            serde_json::from_value(args).context("invalid office_extract_ir arguments")?;
        let path = resolve_existing_path(&ctx.workspace_root, &args.path)?;
        let result = office::extract_ir_via_runtime(ctx, &path).await?;
        Ok(serde_json::to_string_pretty(&result)?)
    }
}
