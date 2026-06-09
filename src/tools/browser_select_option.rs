use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_eval, agent_browser_snapshot, electron_devtools_eval,
    electron_devtools_snapshot, resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::render_session_page;

pub struct BrowserSelectOptionTool;

#[derive(Debug, Deserialize)]
struct BrowserSelectOptionArgs {
    reference: Option<String>,
    #[serde(rename = "ref")]
    ref_alias: Option<String>,
    value: Option<String>,
    label: Option<String>,
    index: Option<usize>,
}

impl BrowserSelectOptionArgs {
    fn reference(&self) -> Result<&str> {
        match (self.reference.as_deref(), self.ref_alias.as_deref()) {
            (Some(reference), Some(ref_alias)) if reference != ref_alias => {
                bail!("browser_select_option received conflicting `reference` and `ref` values")
            }
            (Some(reference), _) => Ok(reference),
            (_, Some(ref_alias)) => Ok(ref_alias),
            (None, None) => {
                bail!("browser_select_option requires a non-empty `reference` or `ref`")
            }
        }
    }
}

#[async_trait]
impl Tool for BrowserSelectOptionTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_select_option",
            "Choose an option in a select element by value, visible label, or index.",
            object_schema(
                json!({
                    "reference": { "type": "string" },
                    "ref": { "type": "string" },
                    "value": { "type": "string" },
                    "label": { "type": "string" },
                    "index": { "type": "integer", "minimum": 0 }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserSelectOptionArgs =
            serde_json::from_value(args).context("invalid browser_select_option arguments")?;
        let reference = args.reference()?.trim();
        if reference.is_empty() {
            bail!("browser_select_option requires a non-empty `reference` or `ref`");
        }
        if args.value.is_none() && args.label.is_none() && args.index.is_none() {
            bail!("browser_select_option requires one of `value`, `label`, or `index`");
        }

        let select_script = build_select_script(
            reference,
            args.value.as_deref(),
            args.label.as_deref(),
            args.index,
        );
        let backend = resolve_active_backend(ctx).await?;
        let selected = match backend {
            ActiveBrowserBackend::AgentBrowser => {
                agent_browser_eval(ctx, &select_script, 20).await?
            }
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_eval(ctx, &select_script, 20).await?
            }
        };
        if !selected.as_bool().unwrap_or(false) {
            bail!("browser_select_option could not match any option");
        }

        let refreshed = match backend {
            ActiveBrowserBackend::AgentBrowser => agent_browser_snapshot(ctx, 6_000, 20).await?,
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_snapshot(ctx, 6_000).await?
            }
        };

        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let mut session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        session.current = refreshed;
        session.set_focus(Some(reference.to_string()));
        session.log_console(format!("selected option on {reference}"));
        store.save(&ctx.current_session_id, &session)?;

        Ok(format!(
            "selected option on {}\n{}",
            reference,
            render_session_page(&session, 2_000)
        ))
    }
}

fn build_select_script(
    reference: &str,
    value: Option<&str>,
    label: Option<&str>,
    index: Option<usize>,
) -> String {
    format!(
        "(function() {{
            const refToken = {reference};
            const nextValue = {value};
            const nextLabel = {label};
            const nextIndex = {index};
            const element = document.querySelector('[data-hermes-ref=\"' + refToken.replace(/^@/, '') + '\"]');
            if (!(element instanceof HTMLSelectElement)) {{
                throw new Error('target element is not a <select>');
            }}
            element.scrollIntoView({{ block: 'center', inline: 'center' }});
            let matched = false;
            if (typeof nextValue === 'string' && nextValue.length) {{
                matched = Array.from(element.options).some((option) => {{
                    if (option.value !== nextValue) return false;
                    element.value = option.value;
                    return true;
                }});
            }}
            if (!matched && typeof nextLabel === 'string' && nextLabel.length) {{
                matched = Array.from(element.options).some((option) => {{
                    if ((option.label || option.textContent || '').trim() !== nextLabel) return false;
                    element.value = option.value;
                    return true;
                }});
            }}
            if (!matched && Number.isInteger(nextIndex) && nextIndex >= 0 && nextIndex < element.options.length) {{
                element.selectedIndex = nextIndex;
                matched = true;
            }}
            if (!matched) {{
                return false;
            }}
            element.dispatchEvent(new Event('input', {{ bubbles: true }}));
            element.dispatchEvent(new Event('change', {{ bubbles: true }}));
            return true;
        }})()",
        reference = json!(reference),
        value = json!(value),
        label = json!(label),
        index = match index {
            Some(value) => value.to_string(),
            None => "null".to_string(),
        },
    )
}
