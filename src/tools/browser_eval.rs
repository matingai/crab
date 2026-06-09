use anyhow::{Context, Result};
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

pub struct BrowserEvalTool;

#[derive(Debug, Deserialize)]
struct BrowserEvalArgs {
    expression: String,
    timeout_seconds: Option<u64>,
    capture_snapshot: Option<bool>,
}

#[async_trait]
impl Tool for BrowserEvalTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_eval",
            "Evaluate JavaScript in the current browser page. Optionally refresh the stored snapshot after evaluation.",
            object_schema(
                json!({
                    "expression": {
                        "type": "string",
                        "description": "JavaScript expression to evaluate in the current page."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60
                    },
                    "capture_snapshot": {
                        "type": "boolean",
                        "description": "When true, refresh the stored browser snapshot after evaluation."
                    }
                }),
                &["expression"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserEvalArgs =
            serde_json::from_value(args).context("invalid browser_eval arguments")?;
        let timeout_seconds = args.timeout_seconds.unwrap_or(20).clamp(1, 60);
        let capture_snapshot = args.capture_snapshot.unwrap_or(true);

        let backend = resolve_active_backend(ctx).await?;
        let result = match backend {
            ActiveBrowserBackend::AgentBrowser => {
                agent_browser_eval(ctx, &args.expression, timeout_seconds).await?
            }
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_eval(ctx, &args.expression, timeout_seconds).await?
            }
        };

        if capture_snapshot {
            let refreshed = match backend {
                ActiveBrowserBackend::AgentBrowser => {
                    agent_browser_snapshot(ctx, 6_000, timeout_seconds).await?
                }
                ActiveBrowserBackend::ElectronDevtools => {
                    electron_devtools_snapshot(ctx, 6_000).await?
                }
            };
            let store = BrowserStateStore::new(&ctx.data_dir)?;
            if let Some(mut session) = store.load(&ctx.current_session_id)? {
                session.current = refreshed;
                session.log_console("evaluated browser expression");
                store.save(&ctx.current_session_id, &session)?;
            }
        }

        let rendered = match result {
            Value::String(text) => text,
            other => serde_json::to_string_pretty(&other).unwrap_or_else(|_| other.to_string()),
        };
        Ok(format!("browser eval result\n{}", rendered))
    }
}
