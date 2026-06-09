use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_eval, agent_browser_snapshot, electron_devtools_eval,
    electron_devtools_snapshot, resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::render_session_page;

pub struct BrowserWaitTool;

#[derive(Debug, Deserialize)]
struct BrowserWaitArgs {
    selector: Option<String>,
    text: Option<String>,
    timeout_seconds: Option<u64>,
    poll_interval_ms: Option<u64>,
    max_chars: Option<usize>,
}

const DEFAULT_TIMEOUT_SECONDS: u64 = 20;
const DEFAULT_POLL_INTERVAL_MS: u64 = 200;
const DEFAULT_MAX_CHARS: usize = 2_000;

#[async_trait]
impl Tool for BrowserWaitTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_wait",
            "Wait until a selector appears or page text contains a target string, then refresh the stored browser snapshot.",
            object_schema(
                json!({
                    "selector": {
                        "type": "string",
                        "description": "CSS selector that must appear."
                    },
                    "text": {
                        "type": "string",
                        "description": "Substring that must appear in the visible page text."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60
                    },
                    "poll_interval_ms": {
                        "type": "integer",
                        "minimum": 50,
                        "maximum": 1000
                    },
                    "max_chars": {
                        "type": "integer",
                        "minimum": 200,
                        "maximum": 16000
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserWaitArgs =
            serde_json::from_value(args).context("invalid browser_wait arguments")?;
        let selector = args
            .selector
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let text = args
            .text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let timeout_seconds = args
            .timeout_seconds
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .clamp(1, 60);
        let poll_interval_ms = args
            .poll_interval_ms
            .unwrap_or(DEFAULT_POLL_INTERVAL_MS)
            .clamp(50, 1_000);
        let max_chars = args
            .max_chars
            .unwrap_or(DEFAULT_MAX_CHARS)
            .clamp(200, 16_000);

        if selector.is_none() && text.is_none() {
            sleep(Duration::from_millis(poll_interval_ms)).await;
        } else {
            wait_for_condition(ctx, selector, text, timeout_seconds, poll_interval_ms).await?;
        }

        let backend = resolve_active_backend(ctx).await?;
        let refreshed = match backend {
            ActiveBrowserBackend::AgentBrowser => {
                agent_browser_snapshot(ctx, 6_000, timeout_seconds).await?
            }
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_snapshot(ctx, 6_000).await?
            }
        };

        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let mut session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        session.current = refreshed;
        session.log_console("wait condition satisfied");
        store.save(&ctx.current_session_id, &session)?;

        Ok(format!(
            "wait condition satisfied\n{}",
            render_session_page(&session, max_chars)
        ))
    }
}

async fn wait_for_condition(
    ctx: &ToolContext,
    selector: Option<&str>,
    text: Option<&str>,
    timeout_seconds: u64,
    poll_interval_ms: u64,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_seconds);
    let selector_json = json!(selector);
    let text_json = json!(text);
    let expression = format!(
        "(function() {{
            const selector = {selector};
            const text = {text};
            if (selector && document.querySelector(selector)) {{
                return true;
            }}
            if (text) {{
                const bodyText = (document.body?.innerText || document.documentElement?.innerText || \"\");
                if (bodyText.includes(text)) {{
                    return true;
                }}
            }}
            return false;
        }})()",
        selector = selector_json,
        text = text_json,
    );

    loop {
        let matched = match resolve_active_backend(ctx).await? {
            ActiveBrowserBackend::AgentBrowser => {
                agent_browser_eval(ctx, &expression, timeout_seconds).await?
            }
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_eval(ctx, &expression, timeout_seconds).await?
            }
        };
        if matched.as_bool().unwrap_or(false) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            bail!("browser_wait timed out before the requested condition was met");
        }
        sleep(Duration::from_millis(poll_interval_ms)).await;
    }
}
