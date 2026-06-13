use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_click, electron_devtools_click, resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::render_page;

pub struct BrowserClickTool;

#[derive(Debug, Deserialize)]
struct BrowserClickArgs {
    reference: Option<String>,
    #[serde(rename = "ref")]
    ref_alias: Option<String>,
    max_chars: Option<usize>,
    timeout_seconds: Option<u64>,
}

impl BrowserClickArgs {
    fn reference(&self) -> Result<&str> {
        match (self.reference.as_deref(), self.ref_alias.as_deref()) {
            (Some(reference), Some(ref_alias)) if reference != ref_alias => {
                bail!("browser_click received conflicting `reference` and `ref` values")
            }
            (Some(reference), _) => Ok(reference),
            (_, Some(ref_alias)) => Ok(ref_alias),
            (None, None) => bail!("browser_click requires a non-empty `reference` or `ref`"),
        }
    }
}

const DEFAULT_MAX_CHARS: usize = 6_000;
const DEFAULT_TIMEOUT_SECONDS: u64 = 20;

#[async_trait]
impl Tool for BrowserClickTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_click",
            "Activate a stored browser element reference from the current session. Link elements open their target page.",
            object_schema(
                json!({
                    "reference": {
                        "type": "string",
                        "description": "Element reference from browser_snapshot, such as @e1."
                    },
                    "ref": {
                        "type": "string",
                        "description": "Alias for `reference`, matching the Python tool schema."
                    },
                    "max_chars": {
                        "type": "integer",
                        "minimum": 200,
                        "maximum": 16000,
                        "description": "Maximum extracted content characters to store and return after following the click."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60,
                        "description": "Request timeout in seconds."
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserClickArgs =
            serde_json::from_value(args).context("invalid browser_click arguments")?;
        let reference = args.reference()?.trim();
        if reference.is_empty() {
            bail!("browser_click requires a non-empty `reference` or `ref`");
        }

        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow::anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        let backend = resolve_active_backend(ctx).await?;
        let element = session
            .current
            .elements
            .iter()
            .find(|element| element.ref_id == reference)
            .ok_or_else(|| {
                anyhow::anyhow!("browser element `{reference}` not found in current snapshot")
            })?;
        let element_target = element.target.clone();
        let max_chars = args
            .max_chars
            .unwrap_or(DEFAULT_MAX_CHARS)
            .clamp(200, 16_000);
        let timeout_seconds = args
            .timeout_seconds
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .clamp(1, 60);
        let next_page = match backend {
            ActiveBrowserBackend::AgentBrowser => {
                agent_browser_click(ctx, reference, max_chars, timeout_seconds).await?
            }
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_click(ctx, reference, max_chars, timeout_seconds).await?
            }
        };
        let mut next_session = session;
        if next_session.current.url == next_page.url
            && next_session.current.final_url == next_page.final_url
        {
            next_session.current = next_page.clone();
        } else {
            next_session.push_navigation(next_page.clone());
        }
        store.save(&ctx.current_session_id, &next_session)?;

        Ok(format!(
            "clicked {}{}\n{}",
            reference,
            element_target
                .as_deref()
                .map(|target| format!(" -> {target}"))
                .unwrap_or_default(),
            render_page(&next_page)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{BrowserClickArgs, BrowserClickTool};
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
            api_mode: crate::llm::ApiMode::ChatCompletions,
            worker_model: None,
            max_iterations: 4,
            current_session_id: "browser-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[tokio::test]
    async fn click_requires_existing_snapshot() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = BrowserClickTool;
        let error = tool
            .execute(json!({ "reference": "@e1" }), &ctx(tmp.path()))
            .await
            .expect_err("should fail");
        assert!(
            error
                .to_string()
                .contains("no browser page stored for this session")
        );
    }

    #[tokio::test]
    async fn click_requires_agent_browser_runtime() {
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
                    "Body",
                    vec![BrowserElement {
                        ref_id: "@e1".to_string(),
                        kind: "button".to_string(),
                        label: "Submit".to_string(),
                        target: None,
                        role: Some("button".to_string()),
                        selector: Some("button".to_string()),
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

        let tool = BrowserClickTool;
        let error = tool
            .execute(json!({ "reference": "@e1" }), &ctx(tmp.path()))
            .await
            .expect_err("should fail");
        assert!(
            error
                .to_string()
                .contains("browser fallback has been removed")
        );
    }

    #[test]
    fn click_allows_matching_reference_and_ref() {
        let args: BrowserClickArgs = serde_json::from_value(json!({
            "reference": "@e1",
            "ref": "@e1"
        }))
        .expect("args");
        assert_eq!(args.reference().expect("reference"), "@e1");
    }

    #[test]
    fn click_rejects_conflicting_reference_and_ref() {
        let args: BrowserClickArgs = serde_json::from_value(json!({
            "reference": "@e1",
            "ref": "@e2"
        }))
        .expect("args");
        assert!(
            args.reference()
                .expect_err("should reject")
                .to_string()
                .contains("conflicting `reference` and `ref`")
        );
    }
}
