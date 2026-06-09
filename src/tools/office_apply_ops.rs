use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::office;
use crate::tools::{Tool, ToolContext, resolve_existing_path, resolve_workspace_path};
use crate::types::{ToolDefinition, object_schema};

pub struct OfficeApplyOpsTool;

#[derive(Debug, Deserialize)]
struct OfficeApplyOpsArgs {
    path: String,
    ops: Value,
    #[serde(default)]
    save_as: Option<String>,
}

#[async_trait]
impl Tool for OfficeApplyOpsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "office_apply_ops",
            "Apply deterministic edit operations to an Office document and save the result to a new file. V1 supports .xlsx and .docx.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Path relative to the workspace root."
                    },
                    "save_as": {
                        "type": "string",
                        "description": "Optional output path relative to the workspace root. Defaults to `<name>.edited.<ext>`."
                    },
                    "ops": {
                        "type": "array",
                        "description": "Edit operations. For xlsx, V1 supports `add_sheet`, `remove_sheet`, `rename_sheet`, `set_cell`, `clear_cell`, `set_range`, `append_rows`, and `clear_range`. For docx, V1 supports `replace_paragraph`, `insert_paragraph_after`, `append_paragraph`, and `remove_paragraph`.",
                        "items": {
                            "type": "object"
                        }
                    }
                }),
                &["path", "ops"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: OfficeApplyOpsArgs =
            serde_json::from_value(args).context("invalid office_apply_ops arguments")?;
        let path = resolve_existing_path(&ctx.workspace_root, &args.path)?;
        let output_path = args
            .save_as
            .as_deref()
            .map(|value| resolve_workspace_path(&ctx.workspace_root, value))
            .transpose()?;
        let result = match path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "xlsx" => {
                office::apply_xlsx_ops_via_runtime(ctx, &path, output_path.as_deref(), &args.ops)
                    .await?
            }
            "docx" => {
                office::apply_docx_ops_via_runtime(ctx, &path, output_path.as_deref(), &args.ops)
                    .await?
            }
            _ => bail!("office_apply_ops v1 supports only .xlsx and .docx"),
        };
        Ok(serde_json::to_string_pretty(&result)?)
    }
}
