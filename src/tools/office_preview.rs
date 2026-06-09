use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::office;
use crate::tools::{Tool, ToolContext, resolve_existing_path};
use crate::types::{ToolDefinition, object_schema};

pub struct OfficePreviewTool;

#[derive(Debug, Deserialize)]
struct OfficePreviewArgs {
    path: String,
    #[serde(default = "default_max_rows")]
    max_rows: usize,
    #[serde(default = "default_max_cols")]
    max_cols: usize,
    #[serde(default = "default_max_paragraphs")]
    max_paragraphs: usize,
    #[serde(default = "default_max_slides")]
    max_slides: usize,
}

fn default_max_rows() -> usize {
    40
}

fn default_max_cols() -> usize {
    12
}

fn default_max_paragraphs() -> usize {
    20
}

fn default_max_slides() -> usize {
    20
}

#[async_trait]
impl Tool for OfficePreviewTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "office_preview",
            "Return a lightweight preview payload for an Office document. V1 supports .xlsx grid previews, .docx paragraph previews, and .pptx slide text previews.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Path relative to the workspace root."
                    },
                    "max_rows": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 500,
                        "description": "Maximum preview rows per sheet."
                    },
                    "max_cols": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "description": "Maximum preview columns per sheet."
                    },
                    "max_paragraphs": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 200,
                        "description": "Maximum preview paragraphs for .docx files."
                    },
                    "max_slides": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 200,
                        "description": "Maximum preview slides for .pptx files."
                    }
                }),
                &["path"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: OfficePreviewArgs =
            serde_json::from_value(args).context("invalid office_preview arguments")?;
        let path = resolve_existing_path(&ctx.workspace_root, &args.path)?;
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let result = match ext.as_str() {
            "xlsx" => {
                office::preview_xlsx_via_runtime(ctx, &path, args.max_rows, args.max_cols).await?
            }
            "docx" => office::preview_docx_via_runtime(ctx, &path, args.max_paragraphs).await?,
            "pptx" => office::preview_pptx_via_runtime(ctx, &path, args.max_slides).await?,
            _ => bail!("office_preview v1 supports only .xlsx, .docx, and .pptx"),
        };
        Ok(serde_json::to_string_pretty(&result)?)
    }
}
