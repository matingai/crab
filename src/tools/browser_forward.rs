use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_forward, electron_devtools_forward, resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::render_page;

pub struct BrowserForwardTool;

#[async_trait]
impl Tool for BrowserForwardTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_forward",
            "Navigate forward to the next stored browser page in the current session.",
            object_schema(json!({}), &[]),
        )
    }

    async fn execute(&self, _args: Value, ctx: &ToolContext) -> Result<String> {
        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let mut session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        if !session.go_forward() {
            bail!("browser_forward has no next page in history");
        }
        let backend = resolve_active_backend(ctx).await?;
        let refreshed = match backend {
            ActiveBrowserBackend::AgentBrowser => agent_browser_forward(ctx, 6_000, 20).await?,
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_forward(ctx, 6_000, 20).await?
            }
        };
        session.current = refreshed;
        store.save(&ctx.current_session_id, &session)?;
        Ok(format!(
            "navigated forward\n{}",
            render_page(session.current_page())
        ))
    }
}
