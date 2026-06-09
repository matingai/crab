use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_back, electron_devtools_back, resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::render_page;

pub struct BrowserBackTool;

#[async_trait]
impl Tool for BrowserBackTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_back",
            "Navigate back to the previous stored browser page in the current session.",
            object_schema(json!({}), &[]),
        )
    }

    async fn execute(&self, _args: Value, ctx: &ToolContext) -> Result<String> {
        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let mut session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        if !session.go_back() {
            bail!("browser_back has no previous page in history");
        }
        let backend = resolve_active_backend(ctx).await?;
        let refreshed = match backend {
            ActiveBrowserBackend::AgentBrowser => agent_browser_back(ctx, 6_000, 20).await?,
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_back(ctx, 6_000, 20).await?
            }
        };
        session.current = refreshed;
        store.save(&ctx.current_session_id, &session)?;
        Ok(format!(
            "navigated back\n{}",
            render_page(session.current_page())
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::BrowserBackTool;
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
            max_iterations: 4,
            current_session_id: "browser-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[tokio::test]
    async fn browser_back_requires_agent_browser_runtime() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = BrowserStateStore::new(tmp.path().join(".data")).expect("store");
        let first = BrowserPageState::new(
            "https://example.com",
            "https://example.com",
            "text/html",
            Some("First".to_string()),
            "First page",
            Vec::new(),
            Vec::new(),
            false,
        );
        let second = BrowserPageState::new(
            "https://example.com/docs",
            "https://example.com/docs",
            "text/html",
            Some("Second".to_string()),
            "Second page",
            Vec::new(),
            Vec::new(),
            false,
        );
        let mut session = BrowserSessionState::new(first);
        session.push_navigation(second);
        store.save("browser-session", &session).expect("save");

        let tool = BrowserBackTool;
        let error = tool
            .execute(json!({}), &ctx(tmp.path()))
            .await
            .expect_err("should fail");
        assert!(
            error
                .to_string()
                .contains("browser fallback has been removed")
        );
    }
}
