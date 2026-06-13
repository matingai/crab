use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::Duration;

use crate::network_policy::{NetworkPolicyPreflight, evaluate_network_policy};
use crate::tools::{Tool, ToolContext, truncated};
use crate::types::{ToolDefinition, object_schema};
use crate::web_content::{fetch_web_page, indent_block};

pub struct WebExtractTool;

#[derive(Debug, Deserialize)]
struct WebExtractArgs {
    urls: Vec<String>,
    max_chars_per_page: Option<usize>,
    timeout_seconds: Option<u64>,
}

const DEFAULT_MAX_CHARS_PER_PAGE: usize = 4_000;
const MAX_BODY_BYTES: usize = 512 * 1024;
const DEFAULT_TIMEOUT_SECONDS: u64 = 15;

#[async_trait]
impl Tool for WebExtractTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "web_extract",
            "Fetch one or more web pages and extract readable text content.",
            object_schema(
                json!({
                    "urls": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "maxItems": 10,
                        "description": "HTTP or HTTPS URLs to fetch."
                    },
                    "max_chars_per_page": {
                        "type": "integer",
                        "minimum": 200,
                        "maximum": 12000,
                        "description": "Maximum extracted characters to return per page."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60,
                        "description": "Per-request timeout in seconds."
                    }
                }),
                &["urls"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: WebExtractArgs =
            serde_json::from_value(args).context("invalid web_extract arguments")?;
        if args.urls.is_empty() {
            bail!("web_extract requires at least one URL");
        }

        let timeout_seconds = args
            .timeout_seconds
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .clamp(1, 60);
        let max_chars = args
            .max_chars_per_page
            .unwrap_or(DEFAULT_MAX_CHARS_PER_PAGE)
            .clamp(200, 12_000);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .context("failed to build HTTP client")?;

        let mut results = Vec::new();
        for url in args.urls {
            results.push(fetch_one(&client, ctx, &url, max_chars).await);
        }

        Ok(results.join("\n\n"))
    }
}

async fn fetch_one(
    client: &reqwest::Client,
    ctx: &ToolContext,
    url: &str,
    max_chars: usize,
) -> String {
    match fetch_one_inner(client, ctx, url, max_chars).await {
        Ok(result) => result,
        Err(error) => format!(
            "- url: {url}\n  status: error\n  error: {}",
            truncated(error.to_string(), 400)
        ),
    }
}

async fn fetch_one_inner(
    client: &reqwest::Client,
    ctx: &ToolContext,
    url: &str,
    max_chars: usize,
) -> Result<String> {
    match evaluate_network_policy(&ctx.data_dir, "web_extract", url)? {
        NetworkPolicyPreflight::Allow => {}
        NetworkPolicyPreflight::Deny(reason) => bail!("{reason}"),
    }
    let page = fetch_web_page(client, url, max_chars, MAX_BODY_BYTES).await?;

    Ok(format!(
        "- url: {url}\n  final_url: {final_url}\n  status: ok\n  content_type: {}\n  title: {}\n  truncated_body: {}\n  content:\n{}",
        if page.content_type.is_empty() {
            "unknown"
        } else {
            &page.content_type
        },
        page.title.unwrap_or_else(|| "(none)".to_string()),
        page.truncated_body,
        indent_block(&page.content),
        final_url = page.final_url,
    ))
}

#[cfg(test)]
mod tests {
    use super::WebExtractTool;
    use crate::tools::{Tool, ToolContext};
    use crate::web_content::{clean_plain_text, extract_title, html_to_text};
    use serde_json::json;

    fn ctx(root: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace_root: root.to_path_buf(),
            data_dir: root.join(".data"),
            shell_enabled: false,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            api_mode: crate::llm::ApiMode::ChatCompletions,
            max_iterations: 4,
            current_session_id: "test-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[test]
    fn extracts_html_content() {
        let html = "<html><head><title>Demo Page</title></head><body><h1>Hello</h1><p>World &amp; more.</p></body></html>";
        assert_eq!(extract_title(html).as_deref(), Some("Demo Page"));
        let text = html_to_text(html, 500);
        assert!(text.contains("Hello"));
        assert!(text.contains("World & more."));
    }

    #[test]
    fn cleans_plain_text_content() {
        let text = clean_plain_text("one   two\n\n\nthree\tfour");
        assert_eq!(text, "one two\nthree four");
    }

    #[tokio::test]
    async fn rejects_unsupported_url_scheme() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = WebExtractTool;
        let output = tool
            .execute(
                json!({ "urls": ["file:///tmp/demo.txt"] }),
                &ctx(tmp.path()),
            )
            .await
            .expect("extract");

        assert!(output.contains("status: error"));
        assert!(output.contains("unsupported URL scheme"));
    }

    #[tokio::test]
    async fn blocks_private_network_urls_before_fetch() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = WebExtractTool;
        let output = tool
            .execute(
                json!({ "urls": ["http://127.0.0.1:9/private"] }),
                &ctx(tmp.path()),
            )
            .await
            .expect("extract");

        assert!(output.contains("status: error"));
        assert!(output.contains("blocked by network_policy"));
        assert!(output.contains("private/local host `127.0.0.1`"));
    }
}
