use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;

use crate::tools::{Tool, ToolContext, relative_display, resolve_existing_path};
use crate::types::{ToolDefinition, object_schema};

pub struct ReadFileTool;

#[derive(Debug, Deserialize)]
struct ReadFileArgs {
    path: String,
    #[serde(default = "default_max_bytes")]
    max_bytes: usize,
}

fn default_max_bytes() -> usize {
    32 * 1024
}

#[async_trait]
impl Tool for ReadFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "read_file",
            "Read a UTF-8 text file inside the workspace.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Path relative to the workspace root."
                    },
                    "max_bytes": {
                        "type": "integer",
                        "description": "Maximum number of bytes to return.",
                        "minimum": 256,
                        "maximum": 262144
                    }
                }),
                &["path"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: ReadFileArgs =
            serde_json::from_value(args).context("invalid read_file arguments")?;
        let path = resolve_existing_path(&ctx.workspace_root, &args.path)?;
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let truncated = bytes.len() > args.max_bytes;
        let slice = if truncated {
            &bytes[..args.max_bytes]
        } else {
            &bytes[..]
        };
        let content = String::from_utf8_lossy(slice);

        Ok(format!(
            "path: {}\nbytes: {}\ntruncated: {}\n\n{}",
            relative_display(&ctx.workspace_root, &path),
            bytes.len(),
            truncated,
            content
        ))
    }
}
