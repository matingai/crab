use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::office;
use crate::tools::{Tool, ToolContext, relative_display, resolve_workspace_path};
use crate::types::{ToolDefinition, object_schema};

pub struct OfficeCreateTool;

#[derive(Debug, Deserialize)]
struct OfficeCreateArgs {
    #[serde(default)]
    kind: Option<String>,
    output_path: String,
    #[serde(default)]
    spec: Option<Value>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[async_trait]
impl Tool for OfficeCreateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "office_create",
            "Create a new Office document using the built-in Rust Office implementation. V1 supports kind=`xlsx` and kind=`docx`. `spec` is optional; for docx, `content` or `text` is accepted as plain paragraph text.",
            object_schema(
                json!({
                    "kind": {
                        "type": "string",
                        "description": "Document kind to create. V1 supports `xlsx` and `docx`. If omitted, it is inferred from output_path."
                    },
                    "output_path": {
                        "type": "string",
                        "description": "Path relative to the workspace root."
                    },
                    "spec": {
                        "type": "object",
                        "description": "Document spec. For xlsx, pass `{ \"sheets\": [{ \"name\": \"Sheet1\", \"cells\": [{ \"addr\": \"A1\", \"value\": \"hello\" }] }] }`. For docx, pass `{ \"paragraphs\": [\"Intro\", \"Body\"] }`, `{ \"blocks\": [{ \"kind\": \"paragraph\", \"text\": \"Intro\" }] }`, or `{ \"text\": \"Intro\\nBody\" }` to split newline-delimited text into paragraphs."
                    },
                    "content": {
                        "type": "string",
                        "description": "Plain text content for docx documents. Newlines are split into paragraphs."
                    },
                    "text": {
                        "type": "string",
                        "description": "Alias for content for docx documents."
                    }
                }),
                &["output_path"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: OfficeCreateArgs =
            serde_json::from_value(args).context("invalid office_create arguments")?;
        let output_path = resolve_workspace_path(&ctx.workspace_root, &args.output_path)?;
        let kind = args
            .kind
            .as_deref()
            .map(str::to_ascii_lowercase)
            .or_else(|| {
                output_path
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(str::to_ascii_lowercase)
            })
            .context("office_create requires kind or an output_path extension")?;
        let spec = normalized_spec(&kind, args.spec, args.content.or(args.text))?;
        let result = match kind.as_str() {
            "xlsx" => office::create_xlsx_via_runtime(ctx, &output_path, &spec).await?,
            "docx" => office::create_docx_via_runtime(ctx, &output_path, &spec).await?,
            _ => bail!("office_create v1 supports only kind=`xlsx` or kind=`docx`"),
        };
        Ok(format!(
            "created {}\n{}",
            relative_display(&ctx.workspace_root, &output_path),
            serde_json::to_string_pretty(&result)?
        ))
    }
}

fn normalized_spec(kind: &str, spec: Option<Value>, content: Option<String>) -> Result<Value> {
    if let Some(spec) = spec {
        return Ok(spec);
    }
    match kind {
        "docx" => Ok(match content {
            Some(text) => json!({ "text": text }),
            None => json!({ "paragraphs": [""] }),
        }),
        "xlsx" => Ok(json!({ "sheets": [{ "name": "Sheet1", "cells": [] }] })),
        _ => bail!("office_create v1 supports only kind=`xlsx` or kind=`docx`"),
    }
}

#[cfg(test)]
mod tests {
    use super::OfficeCreateTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    fn ctx(root: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace_root: root.to_path_buf(),
            data_dir: root.join(".data"),
            shell_enabled: false,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            api_mode: crate::llm::ApiMode::ChatCompletions,
            worker_model: None,
            max_iterations: 4,
            current_session_id: "session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[tokio::test]
    async fn creates_docx_without_explicit_spec() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = OfficeCreateTool;
        let output = tool
            .execute(
                json!({
                    "kind": "docx",
                    "output_path": "blank.docx"
                }),
                &ctx(tmp.path()),
            )
            .await
            .expect("create");

        assert!(output.contains("blank.docx"));
        assert!(tmp.path().join("blank.docx").is_file());
    }

    #[tokio::test]
    async fn creates_docx_from_plain_content() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = OfficeCreateTool;
        tool.execute(
            json!({
                "output_path": "notes.docx",
                "content": "Intro\nDetails"
            }),
            &ctx(tmp.path()),
        )
        .await
        .expect("create");

        let preview =
            crate::office::preview_docx(&tmp.path().join("notes.docx"), 10).expect("preview");
        assert_eq!(preview["paragraphs"][0], json!("Intro"));
        assert_eq!(preview["paragraphs"][1], json!("Details"));
    }
}
