use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::pdf;
use crate::tools::{Tool, ToolContext, resolve_existing_path};
use crate::types::{ToolDefinition, object_schema};

pub struct PdfExtractIrTool;

#[derive(Debug, Deserialize)]
struct PdfExtractIrArgs {
    path: String,
    #[serde(default = "default_max_pages")]
    max_pages: usize,
    #[serde(default = "default_max_chars_per_page")]
    max_chars_per_page: usize,
}

fn default_max_pages() -> usize {
    20
}

fn default_max_chars_per_page() -> usize {
    4_000
}

#[async_trait]
impl Tool for PdfExtractIrTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "pdf_extract_ir",
            "Extract a structured page-oriented representation from a PDF document so a model can summarize, explain, and reference specific pages.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Path relative to the workspace root."
                    },
                    "max_pages": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 500,
                        "description": "Maximum number of pages to extract into the IR."
                    },
                    "max_chars_per_page": {
                        "type": "integer",
                        "minimum": 200,
                        "maximum": 20000,
                        "description": "Maximum extracted characters per page."
                    }
                }),
                &["path"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: PdfExtractIrArgs =
            serde_json::from_value(args).context("invalid pdf_extract_ir arguments")?;
        let path = resolve_existing_path(&ctx.workspace_root, &args.path)?;
        let result = pdf::extract_ir(&path, args.max_pages, args.max_chars_per_page)?;
        Ok(serde_json::to_string_pretty(&result)?)
    }
}
