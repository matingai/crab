use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_eval, agent_browser_snapshot, electron_devtools_hover,
    resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::render_session_page;

pub struct BrowserHoverTool;

#[derive(Debug, Deserialize)]
struct BrowserHoverArgs {
    reference: Option<String>,
    #[serde(rename = "ref")]
    ref_alias: Option<String>,
    max_chars: Option<usize>,
    timeout_seconds: Option<u64>,
}

impl BrowserHoverArgs {
    fn reference(&self) -> Result<&str> {
        match (self.reference.as_deref(), self.ref_alias.as_deref()) {
            (Some(reference), Some(ref_alias)) if reference != ref_alias => {
                bail!("browser_hover received conflicting `reference` and `ref` values")
            }
            (Some(reference), _) => Ok(reference),
            (_, Some(ref_alias)) => Ok(ref_alias),
            (None, None) => bail!("browser_hover requires a non-empty `reference` or `ref`"),
        }
    }
}

#[async_trait]
impl Tool for BrowserHoverTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_hover",
            "Move the pointer over a stored browser element reference.",
            object_schema(
                json!({
                    "reference": { "type": "string" },
                    "ref": { "type": "string" },
                    "max_chars": { "type": "integer", "minimum": 200, "maximum": 16000 },
                    "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": 60 }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserHoverArgs =
            serde_json::from_value(args).context("invalid browser_hover arguments")?;
        let reference = args.reference()?.trim();
        if reference.is_empty() {
            bail!("browser_hover requires a non-empty `reference` or `ref`");
        }
        let max_chars = args.max_chars.unwrap_or(2_000).clamp(200, 16_000);
        let timeout_seconds = args.timeout_seconds.unwrap_or(20).clamp(1, 60);

        let backend = resolve_active_backend(ctx).await?;
        let refreshed = match backend {
            ActiveBrowserBackend::AgentBrowser => {
                let script = format!(
                    "(function() {{
                        const refToken = {reference};
                        const element = document.querySelector('[data-hermes-ref=\"' + refToken.replace(/^@/, '') + '\"]');
                        if (!(element instanceof Element)) {{
                            return false;
                        }}
                        element.scrollIntoView({{ block: 'center', inline: 'center' }});
                        for (const eventName of ['mouseover', 'mouseenter', 'mousemove']) {{
                            element.dispatchEvent(new MouseEvent(eventName, {{ bubbles: true, cancelable: true, view: window }}));
                        }}
                        return true;
                    }})()",
                    reference = json!(reference),
                );
                let hovered = agent_browser_eval(ctx, &script, timeout_seconds).await?;
                if !hovered.as_bool().unwrap_or(false) {
                    bail!("browser_hover could not find the target element");
                }
                agent_browser_snapshot(ctx, 6_000, timeout_seconds).await?
            }
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_hover(ctx, reference, 6_000, timeout_seconds).await?
            }
        };

        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let mut session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        session.current = refreshed;
        session.set_focus(Some(reference.to_string()));
        session.log_console(format!("hovered {}", reference));
        store.save(&ctx.current_session_id, &session)?;

        Ok(format!(
            "hovered {}\n{}",
            reference,
            render_session_page(&session, max_chars)
        ))
    }
}
