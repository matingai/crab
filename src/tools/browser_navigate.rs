use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::browser_backend::{
    ActiveBrowserBackend, agent_browser_navigate, electron_devtools_navigate,
    resolve_active_backend,
};
use crate::browser_state::{BrowserPageState, BrowserSessionState, BrowserStateStore};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};
use crate::web_content::{indent_block, validate_url};

pub struct BrowserNavigateTool;

#[derive(Debug, Deserialize)]
struct BrowserNavigateArgs {
    url: String,
    max_chars: Option<usize>,
    timeout_seconds: Option<u64>,
}

const DEFAULT_MAX_CHARS: usize = 6_000;
const DEFAULT_TIMEOUT_SECONDS: u64 = 20;
#[async_trait]
impl Tool for BrowserNavigateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_navigate",
            "Open a web page for the current session and return a compact, readable snapshot with interactive element refs. Use browser tools for pages that require clicking, typing, dynamic content, visual inspection, or login/form flows.",
            object_schema(
                json!({
                    "url": {
                        "type": "string",
                        "description": "HTTP or HTTPS URL to open."
                    },
                    "max_chars": {
                        "type": "integer",
                        "minimum": 200,
                        "maximum": 16000,
                        "description": "Maximum extracted content characters to store and return."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60,
                        "description": "Request timeout in seconds."
                    }
                }),
                &["url"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserNavigateArgs =
            serde_json::from_value(args).context("invalid browser_navigate arguments")?;
        let max_chars = args
            .max_chars
            .unwrap_or(DEFAULT_MAX_CHARS)
            .clamp(200, 16_000);
        let timeout_seconds = args
            .timeout_seconds
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .clamp(1, 60);

        let state = navigate_and_store(ctx, &args.url, max_chars, timeout_seconds).await?;

        Ok(render_page(&state))
    }
}

pub(crate) async fn navigate_and_store(
    ctx: &ToolContext,
    url: &str,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    validate_url(url)?;
    let backend = resolve_active_backend(ctx).await?;
    let state = match backend {
        ActiveBrowserBackend::AgentBrowser => {
            agent_browser_navigate(ctx, url, max_chars, timeout_seconds).await?
        }
        ActiveBrowserBackend::ElectronDevtools => {
            electron_devtools_navigate(ctx, url, max_chars, timeout_seconds).await?
        }
    };
    let store = BrowserStateStore::new(&ctx.data_dir)?;
    let mut session = store
        .load(&ctx.current_session_id)?
        .unwrap_or_else(|| BrowserSessionState::new(state.clone()));
    if session.current.url == state.url && session.current.final_url == state.final_url {
        session.current = state.clone();
    } else {
        session.push_navigation(state.clone());
    }
    store.save(&ctx.current_session_id, &session)?;
    Ok(state)
}

pub(crate) fn render_page(page: &BrowserPageState) -> String {
    let elements_block = if page.elements.is_empty() {
        "    (none)".to_string()
    } else {
        page.elements
            .iter()
            .take(64)
            .map(format_browser_element)
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "browser page loaded\nurl: {}\nfinal_url: {}\ncontent_type: {}\ntitle: {}\ntruncated_body: {}\nfetched_at_unix: {}\nelements:\n{}\ncontent:\n{}",
        page.url,
        page.final_url,
        if page.content_type.is_empty() {
            "unknown"
        } else {
            &page.content_type
        },
        page.title.as_deref().unwrap_or("(none)"),
        page.truncated_body,
        page.fetched_at_unix,
        elements_block,
        indent_block(&page.content)
    )
}

pub(crate) fn format_browser_element(element: &crate::browser_state::BrowserElement) -> String {
    let mut details = Vec::new();
    if let Some(role) = element
        .role
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        details.push(format!("role={role}"));
    }
    if let Some(selector) = element
        .selector
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        details.push(format!("selector={selector}"));
    }
    if let Some(bbox) = &element.bbox {
        details.push(format!(
            "bbox={},{} {}x{}",
            bbox.x, bbox.y, bbox.width, bbox.height
        ));
    }
    if element.disabled == Some(true) {
        details.push("disabled=true".to_string());
    }
    if let Some(checked) = element.checked {
        details.push(format!("checked={checked}"));
    }
    if let Some(selected) = element.selected {
        details.push(format!("selected={selected}"));
    }
    if element.required == Some(true) {
        details.push("required=true".to_string());
    }
    if let Some(name) = element
        .field_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        details.push(format!("name={name}"));
    }
    if let Some(value) = element.value.as_deref().filter(|value| !value.is_empty()) {
        details.push(format!("value={value}"));
    }

    let suffix = if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join(", "))
    };
    match &element.target {
        Some(target) => format!(
            "    {} [{}] {} -> {}{}",
            element.ref_id, element.kind, element.label, target, suffix
        ),
        None => format!(
            "    {} [{}] {}{}",
            element.ref_id, element.kind, element.label, suffix
        ),
    }
}

pub(crate) fn render_session_page(session: &BrowserSessionState, max_chars: usize) -> String {
    let mut page = session.current.clone();
    let total_chars = page.content.chars().count();
    let offset = usize::min(session.scroll_offset, total_chars);
    let visible = page
        .content
        .chars()
        .skip(offset)
        .take(max_chars)
        .collect::<String>();
    page.content = visible;
    let focus = session
        .focused_ref
        .as_deref()
        .map(|value| format!("focused_ref: {value}\n"))
        .unwrap_or_default();
    format!(
        "{}scroll_offset_chars: {}\n{}",
        focus,
        offset,
        render_page(&page)
    )
}

#[cfg(test)]
mod tests {
    use super::{BrowserNavigateTool, render_page};
    use crate::browser_state::{BrowserElement, BrowserPageState};
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

    #[test]
    fn render_page_formats_snapshot() {
        let page = BrowserPageState::new(
            "https://example.com",
            "https://example.com/docs",
            "text/html",
            Some("Example Docs".to_string()),
            "Hello browser",
            vec![BrowserElement {
                ref_id: "@e1".to_string(),
                kind: "link".to_string(),
                label: "Docs".to_string(),
                target: Some("https://example.com/docs".to_string()),
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
        );
        let output = render_page(&page);
        assert!(output.contains("browser page loaded"));
        assert!(output.contains("Example Docs"));
        assert!(output.contains("Hello browser"));
        assert!(output.contains("@e1 [link] Docs"));
    }

    #[tokio::test]
    async fn navigate_rejects_unsupported_scheme() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = BrowserNavigateTool;
        let error = tool
            .execute(json!({ "url": "file:///tmp/test.html" }), &ctx(tmp.path()))
            .await
            .expect_err("should fail");
        assert!(error.to_string().contains("unsupported URL scheme"));
    }
}
