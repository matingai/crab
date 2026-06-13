use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::browser_state::{BrowserElement, BrowserImage, BrowserStateStore};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

use super::browser_navigate::format_browser_element;

pub struct BrowserFindTool;

#[derive(Debug, Deserialize)]
struct BrowserFindArgs {
    query: String,
    case_sensitive: Option<bool>,
    max_results: Option<usize>,
}

#[async_trait]
impl Tool for BrowserFindTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_find",
            "Search the current browser snapshot, interactive elements, links, form fields, and image metadata. Use this to quickly locate controls or text before clicking, typing, or scrolling.",
            object_schema(
                json!({
                    "query": {
                        "type": "string",
                        "description": "Text to find in page content, element labels, URLs, field names, values, or image alt/src."
                    },
                    "case_sensitive": {
                        "type": "boolean",
                        "description": "Defaults to false."
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 50,
                        "description": "Maximum matches to return. Defaults to 12."
                    }
                }),
                &["query"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: BrowserFindArgs =
            serde_json::from_value(args).context("invalid browser_find arguments")?;
        let query = args.query.trim();
        if query.is_empty() {
            bail!("browser_find requires a non-empty query");
        }
        let case_sensitive = args.case_sensitive.unwrap_or(false);
        let max_results = args.max_results.unwrap_or(12).clamp(1, 50);
        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;

        let mut matches = Vec::new();
        for element in &session.current.elements {
            if searchable_element_text(element).contains_match(query, case_sensitive) {
                matches.push(format!(
                    "element: {}",
                    format_browser_element(element).trim()
                ));
                if matches.len() >= max_results {
                    break;
                }
            }
        }

        if matches.len() < max_results {
            for image in &session.current.images {
                if searchable_image_text(image).contains_match(query, case_sensitive) {
                    matches.push(format!(
                        "image: {}{}",
                        image.src,
                        if image.alt.trim().is_empty() {
                            String::new()
                        } else {
                            format!(" | alt={}", image.alt)
                        }
                    ));
                    if matches.len() >= max_results {
                        break;
                    }
                }
            }
        }

        if matches.len() < max_results {
            for snippet in content_snippets(&session.current.content, query, case_sensitive) {
                matches.push(format!("content: {snippet}"));
                if matches.len() >= max_results {
                    break;
                }
            }
        }

        if matches.is_empty() {
            return Ok(format!(
                "browser_find found no matches for `{query}` on {}",
                session.current.final_url
            ));
        }
        Ok(format!(
            "browser_find found {} match(es) for `{query}` on {}\n{}",
            matches.len(),
            session.current.final_url,
            matches.join("\n")
        ))
    }
}

trait ContainsMatch {
    fn contains_match(&self, query: &str, case_sensitive: bool) -> bool;
}

impl ContainsMatch for str {
    fn contains_match(&self, query: &str, case_sensitive: bool) -> bool {
        if case_sensitive {
            self.contains(query)
        } else {
            self.to_lowercase().contains(&query.to_lowercase())
        }
    }
}

fn searchable_element_text(element: &BrowserElement) -> String {
    [
        element.ref_id.as_str(),
        element.kind.as_str(),
        element.label.as_str(),
        element.target.as_deref().unwrap_or_default(),
        element.role.as_deref().unwrap_or_default(),
        element.selector.as_deref().unwrap_or_default(),
        element.field_name.as_deref().unwrap_or_default(),
        element.value.as_deref().unwrap_or_default(),
        element.form_id.as_deref().unwrap_or_default(),
        element.form_action.as_deref().unwrap_or_default(),
    ]
    .join("\n")
}

fn searchable_image_text(image: &BrowserImage) -> String {
    format!("{}\n{}", image.src, image.alt)
}

fn content_snippets(content: &str, query: &str, case_sensitive: bool) -> Vec<String> {
    let mut snippets = Vec::new();
    for line in content.lines() {
        if !line.contains_match(query, case_sensitive) {
            continue;
        }
        let compact = line.trim().replace('\t', " ");
        if compact.is_empty() {
            continue;
        }
        snippets.push(truncate_chars(&compact, 180));
        if snippets.len() >= 20 {
            break;
        }
    }
    snippets
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut output = value.chars().take(max_chars).collect::<String>();
    output.push_str("...");
    output
}

#[cfg(test)]
mod tests {
    use super::BrowserFindTool;
    use crate::browser_state::{
        BrowserElement, BrowserImage, BrowserPageState, BrowserSessionState, BrowserStateStore,
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
    async fn finds_elements_images_and_content() {
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
                    "Welcome to the docs. Search the catalog.",
                    vec![BrowserElement {
                        ref_id: "@e1".to_string(),
                        kind: "input:search".to_string(),
                        label: "Search docs".to_string(),
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
                    vec![BrowserImage {
                        src: "https://example.com/logo.png".to_string(),
                        alt: "Docs logo".to_string(),
                    }],
                    false,
                )),
            )
            .expect("save");

        let output = BrowserFindTool
            .execute(json!({ "query": "docs" }), &ctx(tmp.path()))
            .await
            .expect("find");
        assert!(output.contains("@e1"));
        assert!(output.contains("logo.png"));
        assert!(output.contains("Welcome"));
    }
}
