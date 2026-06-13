use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_press, electron_devtools_press, resolve_active_backend,
};
use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::{render_page, render_session_page};

pub struct BrowserPressTool;

#[derive(Debug, Deserialize)]
struct BrowserPressArgs {
    key: String,
}

const DEFAULT_MAX_CHARS: usize = 6_000;
const DEFAULT_TIMEOUT_SECONDS: u64 = 20;
const SCROLL_CHARS: isize = 1_200;

#[async_trait]
impl Tool for BrowserPressTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_press",
            "Press a keyboard key for the current browser session. Useful for focus navigation and simple keyboard activation.",
            object_schema(
                json!({
                    "key": {
                        "type": "string",
                        "description": "Key to press, such as Tab, Shift+Tab, Enter, Space, ArrowDown, ArrowUp, or Backspace."
                    }
                }),
                &["key"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserPressArgs =
            serde_json::from_value(args).context("invalid browser_press arguments")?;
        let key = args.key.trim();
        if key.is_empty() {
            bail!("browser_press requires a non-empty `key`");
        }

        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let mut session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        let normalized = key.to_ascii_lowercase();
        let backend = resolve_active_backend(ctx).await?;

        let press_backend = async || -> Result<_> {
            match backend {
                ActiveBrowserBackend::AgentBrowser => {
                    agent_browser_press(ctx, key, DEFAULT_MAX_CHARS, DEFAULT_TIMEOUT_SECONDS).await
                }
                ActiveBrowserBackend::ElectronDevtools => {
                    electron_devtools_press(ctx, key, DEFAULT_MAX_CHARS, DEFAULT_TIMEOUT_SECONDS)
                        .await
                }
            }
        };

        match normalized.as_str() {
            "tab" => {
                let _ = press_backend().await?;
                let focused = session.focus_next(false);
                session.log_console(format!(
                    "pressed {}{}",
                    key,
                    focused
                        .as_deref()
                        .map(|value| format!(" -> focus {value}"))
                        .unwrap_or_default()
                ));
                store.save(&ctx.current_session_id, &session)?;
                Ok(format!(
                    "pressed {}\n{}",
                    key,
                    render_session_page(&session, 2_000)
                ))
            }
            "shift+tab" | "shift-tab" => {
                let _ = press_backend().await?;
                let focused = session.focus_next(true);
                session.log_console(format!(
                    "pressed {}{}",
                    key,
                    focused
                        .as_deref()
                        .map(|value| format!(" -> focus {value}"))
                        .unwrap_or_default()
                ));
                store.save(&ctx.current_session_id, &session)?;
                Ok(format!(
                    "pressed {}\n{}",
                    key,
                    render_session_page(&session, 2_000)
                ))
            }
            "arrowdown" | "pagedown" => {
                let _ = press_backend().await?;
                session.scroll_by(SCROLL_CHARS, 2_000);
                session.log_console(format!("pressed {key}"));
                store.save(&ctx.current_session_id, &session)?;
                Ok(format!(
                    "pressed {}\n{}",
                    key,
                    render_session_page(&session, 2_000)
                ))
            }
            "arrowup" | "pageup" => {
                let _ = press_backend().await?;
                session.scroll_by(-SCROLL_CHARS, 2_000);
                session.log_console(format!("pressed {key}"));
                store.save(&ctx.current_session_id, &session)?;
                Ok(format!(
                    "pressed {}\n{}",
                    key,
                    render_session_page(&session, 2_000)
                ))
            }
            "backspace" => {
                let focused_ref = session
                    .focused_ref
                    .clone()
                    .ok_or_else(|| anyhow!("browser_press Backspace requires a focused element"))?;
                let element = session
                    .current_page_mut()
                    .elements
                    .iter_mut()
                    .find(|element| element.ref_id == focused_ref)
                    .ok_or_else(|| anyhow!("focused browser element `{focused_ref}` not found"))?;
                if !element.kind.starts_with("input:") {
                    bail!(
                        "browser_press Backspace currently supports focused input elements only; `{}` is `{}`",
                        focused_ref,
                        element.kind
                    );
                }
                let _ = press_backend().await?;
                let mut value = element.value.clone().unwrap_or_default();
                value.pop();
                element.value = Some(value);
                session.log_console(format!("pressed {key}"));
                store.save(&ctx.current_session_id, &session)?;
                Ok(format!(
                    "pressed {}\n{}",
                    key,
                    render_session_page(&session, 2_000)
                ))
            }
            "enter" | "space" => {
                let focused_ref = session
                    .focused_ref
                    .clone()
                    .ok_or_else(|| anyhow!("browser_press {key} requires a focused element"))?;
                let element = session
                    .current
                    .elements
                    .iter()
                    .find(|element| element.ref_id == focused_ref)
                    .cloned()
                    .ok_or_else(|| anyhow!("focused browser element `{focused_ref}` not found"))?;
                let next_page = press_backend().await?;
                if element.kind == "link" {
                    session.push_navigation(next_page.clone());
                    store.save(&ctx.current_session_id, &session)?;
                    return Ok(format!(
                        "pressed {} on {}\n{}",
                        key,
                        focused_ref,
                        render_page(&next_page)
                    ));
                }

                session.current = next_page;
                session.log_console(format!("pressed {key} on {focused_ref}"));
                store.save(&ctx.current_session_id, &session)?;
                Ok(format!(
                    "pressed {}\n{}",
                    key,
                    render_session_page(&session, 2_000)
                ))
            }
            _ => bail!(
                "browser_press does not support key `{}` yet; supported keys: Tab, Shift+Tab, Enter, Space, Backspace, ArrowDown, ArrowUp, PageDown, PageUp",
                key
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BrowserPressTool;
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
    async fn browser_press_requires_agent_browser_runtime_for_tab() {
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
                    vec![
                        BrowserElement {
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
                        },
                        BrowserElement {
                            ref_id: "@e2".to_string(),
                            kind: "input:text".to_string(),
                            label: "Search".to_string(),
                            target: None,
                            role: Some("textbox".to_string()),
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
                        },
                    ],
                    Vec::new(),
                    false,
                )),
            )
            .expect("save");

        let tool = BrowserPressTool;
        let error = tool
            .execute(json!({"key": "Tab"}), &ctx(tmp.path()))
            .await
            .expect_err("should fail");
        assert!(
            error
                .to_string()
                .contains("browser fallback has been removed")
        );
    }

    #[tokio::test]
    async fn browser_press_requires_agent_browser_runtime_for_backspace() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = BrowserStateStore::new(tmp.path().join(".data")).expect("store");
        let mut session = BrowserSessionState::new(BrowserPageState::new(
            "https://example.com",
            "https://example.com",
            "text/html",
            Some("Example".to_string()),
            "Body",
            vec![BrowserElement {
                ref_id: "@e1".to_string(),
                kind: "input:text".to_string(),
                label: "Search".to_string(),
                target: None,
                role: Some("textbox".to_string()),
                selector: Some("input[name=\"q\"]".to_string()),
                bbox: None,
                disabled: None,
                checked: None,
                selected: None,
                required: None,
                field_name: Some("q".to_string()),
                value: Some("rust".to_string()),
                form_id: None,
                form_action: None,
                form_method: None,
            }],
            Vec::new(),
            false,
        ));
        session.set_focus(Some("@e1".to_string()));
        store.save("browser-session", &session).expect("save");

        let tool = BrowserPressTool;
        let error = tool
            .execute(json!({"key": "Backspace"}), &ctx(tmp.path()))
            .await
            .expect_err("should fail");
        assert!(
            error
                .to_string()
                .contains("browser fallback has been removed")
        );
    }
}
