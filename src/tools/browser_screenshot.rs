use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use uuid::Uuid;

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_screenshot, electron_devtools_screenshot,
    resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct BrowserScreenshotTool;

#[derive(Debug, Deserialize)]
struct BrowserScreenshotArgs {
    full_page: Option<bool>,
    annotate: Option<bool>,
    timeout_seconds: Option<u64>,
}

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_screenshot",
            "Capture a PNG screenshot of the current browser page. Use this when visual layout, canvas content, charts, CAPTCHA, or styling matters beyond the text snapshot.",
            object_schema(
                json!({
                    "full_page": {
                        "type": "boolean",
                        "description": "When true, capture the full scrollable page when the backend supports it. Defaults to true."
                    },
                    "annotate": {
                        "type": "boolean",
                        "description": "When true, overlay element reference labels before capture when supported."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserScreenshotArgs =
            serde_json::from_value(args).context("invalid browser_screenshot arguments")?;
        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        let full_page = args.full_page.unwrap_or(true);
        let annotate = args.annotate.unwrap_or(false);
        let timeout_seconds = args.timeout_seconds.unwrap_or(20).clamp(1, 60);
        let screenshots_dir = ctx.data_dir.join("browser").join("screenshots");
        fs::create_dir_all(&screenshots_dir).with_context(|| {
            format!(
                "failed to create browser screenshot directory {}",
                screenshots_dir.display()
            )
        })?;
        let output_path =
            screenshots_dir.join(format!("browser_screenshot_{}.png", Uuid::new_v4()));

        match resolve_active_backend(ctx).await? {
            ActiveBrowserBackend::AgentBrowser => {
                agent_browser_screenshot(ctx, &output_path, full_page, annotate, timeout_seconds)
                    .await?;
            }
            ActiveBrowserBackend::ElectronDevtools => {
                let bytes =
                    electron_devtools_screenshot(ctx, full_page, annotate, timeout_seconds).await?;
                if bytes.is_empty() {
                    bail!("electron devtools screenshot returned no image bytes");
                }
                fs::write(&output_path, bytes).with_context(|| {
                    format!("failed to write screenshot {}", output_path.display())
                })?;
            }
        }

        Ok(format!(
            "browser screenshot captured\nurl: {}\ntitle: {}\npath: {}\nfull_page: {}\nannotated: {}",
            session.current.final_url,
            session.current.title.as_deref().unwrap_or("(none)"),
            output_path.display(),
            full_page,
            annotate
        ))
    }
}
