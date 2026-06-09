use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::render_session_page;

pub struct BrowserSnapshotTool;

#[derive(Debug, Deserialize)]
struct BrowserSnapshotArgs {
    full: Option<bool>,
    max_chars: Option<usize>,
}

#[async_trait]
impl Tool for BrowserSnapshotTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_snapshot",
            "Return the current stored browser snapshot: page text plus interactive elements with refs like @e1. Refresh this after clicks, typing, scrolling, waiting, or DOM-changing evals.",
            object_schema(
                json!({
                    "full": {
                        "type": "boolean",
                        "description": "When true, return the fullest available stored page content."
                    },
                    "max_chars": {
                        "type": "integer",
                        "minimum": 200,
                        "maximum": 16000,
                        "description": "Optional maximum content characters to return from the stored snapshot."
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserSnapshotArgs =
            serde_json::from_value(args).context("invalid browser_snapshot arguments")?;
        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let page = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow::anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        let max_chars = match (args.full.unwrap_or(false), args.max_chars) {
            (_, Some(max_chars)) => max_chars,
            (true, None) => 16_000,
            (false, None) => 6_000,
        };
        if max_chars == 0 {
            bail!("browser_snapshot requires `max_chars` to be at least 1");
        }
        Ok(render_session_page(&page, max_chars.clamp(200, 16_000)))
    }
}

#[cfg(test)]
mod tests {
    use super::BrowserSnapshotTool;
    use crate::browser_state::{
        BrowserElement, BrowserPageState, BrowserSessionState, BrowserStateStore,
    };
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
    async fn snapshot_returns_saved_page() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = BrowserStateStore::new(tmp.path().join(".data")).expect("store");
        store
            .save(
                "browser-session",
                &BrowserSessionState::new(BrowserPageState::new(
                    "https://example.com",
                    "https://example.com",
                    "text/html",
                    Some("Example".to_string()),
                    "Hello from browser snapshot",
                    vec![BrowserElement {
                        ref_id: "@e1".to_string(),
                        kind: "link".to_string(),
                        label: "Home".to_string(),
                        target: Some("https://example.com".to_string()),
                        role: Some("link".to_string()),
                        selector: Some("a".to_string()),
                        bbox: None,
                        disabled: None,
                        checked: None,
                        selected: None,
                        required: None,
                        field_name: None,
                        value: None,
                        form_id: None,
                        form_action: None,
                        form_method: None,
                    }],
                    Vec::new(),
                    false,
                )),
            )
            .expect("save");

        let tool = BrowserSnapshotTool;
        let output = tool
            .execute(json!({ "max_chars": 200 }), &ctx(tmp.path()))
            .await
            .expect("snapshot");
        assert!(output.contains("Example"));
        assert!(output.contains("Hello from browser snapshot"));
        assert!(output.contains("@e1 [link] Home"));
    }

    #[tokio::test]
    async fn snapshot_requires_navigate_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = BrowserSnapshotTool;
        let error = tool
            .execute(json!({}), &ctx(tmp.path()))
            .await
            .expect_err("should fail");
        assert!(
            error
                .to_string()
                .contains("no browser page stored for this session")
        );
    }
}
