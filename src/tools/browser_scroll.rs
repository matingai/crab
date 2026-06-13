use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_scroll, electron_devtools_scroll, resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::render_session_page;

pub struct BrowserScrollTool;

#[derive(Debug, Deserialize)]
struct BrowserScrollArgs {
    direction: String,
}

const SCROLL_CHARS: isize = 1_200;

#[async_trait]
impl Tool for BrowserScrollTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_scroll",
            "Scroll the stored browser page content up or down.",
            object_schema(
                json!({
                    "direction": {
                        "type": "string",
                        "enum": ["up", "down"],
                        "description": "Scroll direction."
                    }
                }),
                &["direction"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserScrollArgs =
            serde_json::from_value(args).context("invalid browser_scroll arguments")?;
        let delta = match args.direction.as_str() {
            "up" => -SCROLL_CHARS,
            "down" => SCROLL_CHARS,
            other => bail!("unsupported scroll direction `{other}`"),
        };

        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let mut session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        let backend = resolve_active_backend(ctx).await?;
        let _ = match backend {
            ActiveBrowserBackend::AgentBrowser => {
                agent_browser_scroll(ctx, &args.direction, SCROLL_CHARS as usize, 6_000, 20).await?
            }
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_scroll(ctx, &args.direction, SCROLL_CHARS as usize, 6_000, 20)
                    .await?
            }
        };
        session.scroll_by(delta, 2_000);
        session.log_console(format!("scrolled {}", args.direction));
        store.save(&ctx.current_session_id, &session)?;

        Ok(format!(
            "scrolled {}\n{}",
            args.direction,
            render_session_page(&session, 2_000)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::BrowserScrollTool;
    use crate::browser_state::{BrowserPageState, BrowserSessionState, BrowserStateStore};
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    fn ctx(root: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace_root: root.to_path_buf(),
            data_dir: root.join(".data"),
            shell_enabled: false,
            skill_platform: "desktop".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            api_mode: crate::llm::ApiMode::ChatCompletions,
            worker_model: None,
            max_iterations: 4,
            current_session_id: "browser-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[tokio::test]
    async fn browser_scroll_requires_agent_browser_runtime() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = BrowserStateStore::new(tmp.path().join(".data")).expect("store");
        let content = "0123456789".repeat(800);
        store
            .save(
                "browser-session",
                &BrowserSessionState::new(BrowserPageState::new(
                    "https://example.com",
                    "https://example.com",
                    "text/html",
                    Some("Example".to_string()),
                    content,
                    Vec::new(),
                    Vec::new(),
                    false,
                )),
            )
            .expect("save");

        let tool = BrowserScrollTool;
        let error = tool
            .execute(json!({"direction": "down"}), &ctx(tmp.path()))
            .await
            .expect_err("should fail");
        assert!(
            error
                .to_string()
                .contains("browser fallback has been removed")
        );
    }
}
