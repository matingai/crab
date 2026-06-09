use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::pdf;
use crate::tools::{Tool, ToolContext, resolve_existing_path};
use crate::types::{ToolDefinition, object_schema};

pub struct PdfPreviewTool;

#[derive(Debug, Deserialize)]
struct PdfPreviewArgs {
    path: String,
    #[serde(default = "default_max_pages")]
    max_pages: usize,
    #[serde(default = "default_max_chars_per_page")]
    max_chars_per_page: usize,
}

fn default_max_pages() -> usize {
    8
}

fn default_max_chars_per_page() -> usize {
    1_200
}

#[async_trait]
impl Tool for PdfPreviewTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "pdf_preview",
            "Return a lightweight preview payload for a PDF document with page excerpts suitable for summarization and study workflows.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Path relative to the workspace root."
                    },
                    "max_pages": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 200,
                        "description": "Maximum number of pages to extract into the preview."
                    },
                    "max_chars_per_page": {
                        "type": "integer",
                        "minimum": 200,
                        "maximum": 12000,
                        "description": "Maximum extracted characters per page."
                    }
                }),
                &["path"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: PdfPreviewArgs =
            serde_json::from_value(args).context("invalid pdf_preview arguments")?;
        let path = resolve_existing_path(&ctx.workspace_root, &args.path)?;
        let result = pdf::preview_pdf(&path, args.max_pages, args.max_chars_per_page)?;
        Ok(serde_json::to_string_pretty(&result)?)
    }
}
