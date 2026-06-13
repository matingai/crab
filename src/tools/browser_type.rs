use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_fill, electron_devtools_fill, resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::render_session_page;

pub struct BrowserTypeTool;

#[derive(Debug, Deserialize)]
struct BrowserTypeArgs {
    reference: Option<String>,
    #[serde(rename = "ref")]
    ref_alias: Option<String>,
    text: String,
}

impl BrowserTypeArgs {
    fn reference(&self) -> Result<&str> {
        match (self.reference.as_deref(), self.ref_alias.as_deref()) {
            (Some(reference), Some(ref_alias)) if reference != ref_alias => {
                bail!("browser_type received conflicting `reference` and `ref` values")
            }
            (Some(reference), _) => Ok(reference),
            (_, Some(ref_alias)) => Ok(ref_alias),
            (None, None) => bail!("browser_type requires a non-empty `reference` or `ref`"),
        }
    }
}

#[async_trait]
impl Tool for BrowserTypeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_type",
            "Type text into an input element stored in the current browser session.",
            object_schema(
                json!({
                    "reference": {
                        "type": "string",
                        "description": "Element reference from browser_snapshot, such as @e3."
                    },
                    "ref": {
                        "type": "string",
                        "description": "Alias for `reference`, matching the Python tool schema."
                    },
                    "text": {
                        "type": "string",
                        "description": "Text to place into the target input element."
                    }
                }),
                &["text"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserTypeArgs =
            serde_json::from_value(args).context("invalid browser_type arguments")?;
        let reference = args.reference()?.trim();
        if reference.is_empty() {
            bail!("browser_type requires a non-empty `reference` or `ref`");
        }

        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let mut session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        let element = session
            .current_page_mut()
            .elements
            .iter_mut()
            .find(|element| element.ref_id == reference)
            .ok_or_else(|| {
                anyhow!("browser element `{reference}` not found in current snapshot")
            })?;
        if !element.kind.starts_with("input:") {
            bail!(
                "browser_type currently supports input elements only; `{}` is `{}`",
                reference,
                element.kind
            );
        }

        let backend = resolve_active_backend(ctx).await?;
        let refreshed = match backend {
            ActiveBrowserBackend::AgentBrowser => {
                agent_browser_fill(ctx, reference, &args.text, 6_000, 20).await?
            }
            ActiveBrowserBackend::ElectronDevtools => {
                electron_devtools_fill(ctx, reference, &args.text, 6_000, 20).await?
            }
        };
        let field_label = element
            .field_name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&element.label);
        let field_label = field_label.to_string();
        session.current = refreshed;
        if let Some(updated) = session
            .current_page_mut()
            .elements
            .iter_mut()
            .find(|item| item.ref_id == reference)
        {
            updated.value = Some(args.text.clone());
        }
        session.set_focus(Some(reference.to_string()));
        session.log_console(format!("typed into {} ({})", reference, field_label));
        store.save(&ctx.current_session_id, &session)?;

        Ok(format!(
            "typed into {} ({})\n{}",
            reference,
            field_label,
            render_session_page(&session, 2_000)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{BrowserTypeArgs, BrowserTypeTool};
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
            max_iterations: 4,
            current_session_id: "browser-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[tokio::test]
    async fn browser_type_requires_agent_browser_runtime() {
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
                    "Search docs",
                    vec![BrowserElement {
                        ref_id: "@e1".to_string(),
                        kind: "input:search".to_string(),
                        label: "Search".to_string(),
                        target: None,
                        role: Some("searchbox".to_string()),
                        selector: Some("input[name=\"q\"]".to_string()),
                        bbox: None,
                        disabled: None,
                        checked: None,
                        selected: None,
                        required: None,
                        field_name: Some("q".to_string()),
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

        let tool = BrowserTypeTool;
        let error = tool
            .execute(
                json!({"reference": "@e1", "text": "rust agent"}),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("should fail");
        assert!(
            error
                .to_string()
                .contains("browser fallback has been removed")
        );
    }

    #[test]
    fn type_allows_matching_reference_and_ref() {
        let args: BrowserTypeArgs = serde_json::from_value(json!({
            "reference": "@e1",
            "ref": "@e1",
            "text": "hello"
        }))
        .expect("args");
        assert_eq!(args.reference().expect("reference"), "@e1");
    }

    #[test]
    fn type_rejects_conflicting_reference_and_ref() {
        let args: BrowserTypeArgs = serde_json::from_value(json!({
            "reference": "@e1",
            "ref": "@e2",
            "text": "hello"
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
