use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::pdf;
use crate::tools::{Tool, ToolContext, resolve_existing_path};
use crate::types::{ToolDefinition, object_schema};

pub struct PdfInspectTool;

#[derive(Debug, Deserialize)]
struct PdfInspectArgs {
    path: String,
}

#[async_trait]
impl Tool for PdfInspectTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "pdf_inspect",
            "Inspect a PDF document and report available preview and extract capabilities plus page metadata.",
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
        let args: PdfInspectArgs =
            serde_json::from_value(args).context("invalid pdf_inspect arguments")?;
        let path = resolve_existing_path(&ctx.workspace_root, &args.path)?;
        let result = pdf::inspect_document(&path)?;
        Ok(serde_json::to_string_pretty(&result)?)
    }
}
