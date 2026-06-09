use anyhow::{Context, Result, bail};
use regex::Regex;
use tokio::time::Duration;

use crate::tools::ToolContext;
use crate::tools::truncated;

#[derive(Debug, Clone)]
pub struct ExtractedWebPage {
    pub final_url: String,
    pub content_type: String,
    pub title: Option<String>,
    pub content: String,
    pub elements: Vec<ExtractedWebElement>,
    pub images: Vec<ExtractedWebImage>,
    pub truncated_body: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedWebElement {
    pub kind: String,
    pub label: String,
    pub target: Option<String>,
    pub field_name: Option<String>,
    pub value: Option<String>,
    pub form_id: Option<String>,
    pub form_action: Option<String>,
    pub form_method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedWebImage {
    pub src: String,
    pub alt: String,
}

pub async fn fetch_web_page(
    client: &reqwest::Client,
    url: &str,
    max_chars: usize,
    max_body_bytes: usize,
) -> Result<ExtractedWebPage> {
    validate_url(url)?;

    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("request failed for {url}"))?;
    let status = response.status().as_u16();
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read response body for {url}"))?;
    build_extracted_page(
        status,
        final_url,
        content_type,
        bytes.to_vec(),
        max_chars,
        max_body_bytes,
        None,
    )
}

pub async fn fetch_web_page_for_context(
    ctx: &ToolContext,
    url: &str,
    max_chars: usize,
    max_body_bytes: usize,
    timeout_seconds: u64,
) -> Result<ExtractedWebPage> {
    let _ = ctx;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .context("failed to build HTTP client")?;
    fetch_web_page(&client, url, max_chars, max_body_bytes).await
}

fn build_extracted_page(
    status: u16,
    final_url: String,
    content_type: String,
    bytes: Vec<u8>,
    max_chars: usize,
    max_body_bytes: usize,
    truncated_override: Option<bool>,
) -> Result<ExtractedWebPage> {
    let truncated_body = truncated_override.unwrap_or(bytes.len() > max_body_bytes);
    let body = if truncated_body {
        &bytes[..max_body_bytes]
    } else {
        &bytes[..]
    };
    let body_text = String::from_utf8_lossy(body).to_string();

    if !(200..300).contains(&status) {
        bail!(
            "http {} from {}{}",
            status,
            final_url,
            if body_text.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", truncated(clean_plain_text(&body_text), 240))
            }
        );
    }

    let (title, content, elements, images) = if looks_like_html(&content_type, &body_text) {
        let title = extract_title(&body_text);
        let content = html_to_text(&body_text, max_chars);
        let elements = extract_elements(&body_text, &final_url, 48);
        let images = extract_images(&body_text, &final_url, 64);
        (title, content, elements, images)
    } else {
        (
            None,
            truncate_plain_text(&body_text, max_chars),
            Vec::new(),
            Vec::new(),
        )
    };

    Ok(ExtractedWebPage {
        final_url,
        content_type,
        title,
        content,
        elements,
        images,
        truncated_body,
    })
}

pub fn validate_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url).with_context(|| format!("invalid URL `{url}`"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        other => bail!("unsupported URL scheme `{other}` for `{url}`"),
    }
}

pub fn looks_like_html(content_type: &str, body: &str) -> bool {
    let lowered = content_type.to_ascii_lowercase();
    lowered.contains("text/html")
        || lowered.contains("application/xhtml")
        || body.contains("<html")
        || body.contains("<body")
}

pub fn extract_title(html: &str) -> Option<String> {
    let regex = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("valid title regex");
    regex
        .captures(html)
        .and_then(|captures| captures.get(1))
        .map(|value| clean_plain_text(&decode_html_entities(value.as_str())))
        .filter(|value| !value.is_empty())
}

pub fn html_to_text(html: &str, max_chars: usize) -> String {
    let comment_re = Regex::new(r"(?is)<!--.*?-->").expect("comment regex");
    let block_re = Regex::new(r"(?i)</?(div|p|br|li|ul|ol|section|article|main|header|footer|h[1-6]|tr|td|th|pre|code|blockquote)[^>]*>").expect("block regex");
    let tag_re = Regex::new(r"(?is)<[^>]+>").expect("tag regex");

    let without_comments = comment_re.replace_all(html, " ");
    let without_scripts = strip_tag_block(&without_comments, "script");
    let without_styles = strip_tag_block(&without_scripts, "style");
    let without_noscript = strip_tag_block(&without_styles, "noscript");
    let without_svg = strip_tag_block(&without_noscript, "svg");
    let with_breaks = block_re.replace_all(&without_svg, "\n");
    let without_tags = tag_re.replace_all(&with_breaks, " ");
    truncate_plain_text(&decode_html_entities(&without_tags), max_chars)
}

pub fn extract_elements(
    html: &str,
    base_url: &str,
    max_elements: usize,
) -> Vec<ExtractedWebElement> {
    let mut elements = Vec::new();
    collect_anchor_elements(html, base_url, max_elements, &mut elements);
    collect_button_elements(html, max_elements, &mut elements);
    collect_input_elements(html, max_elements, &mut elements);
    elements
}

pub fn extract_images(html: &str, base_url: &str, max_images: usize) -> Vec<ExtractedWebImage> {
    let mut images = Vec::new();
    let regex = Regex::new(r#"(?is)<img\b([^>]*)/?>"#).expect("img regex");
    for captures in regex.captures_iter(html) {
        if images.len() >= max_images {
            break;
        }
        let attrs = captures.get(1).map(|value| value.as_str()).unwrap_or("");
        let Some(src) = extract_attr(attrs, "src") else {
            continue;
        };
        let Ok(src) = resolve_url(base_url, &src) else {
            continue;
        };
        let alt = extract_attr(attrs, "alt").unwrap_or_default();
        images.push(ExtractedWebImage { src, alt });
    }
    images
}

fn collect_anchor_elements(
    html: &str,
    base_url: &str,
    max_elements: usize,
    out: &mut Vec<ExtractedWebElement>,
) {
    let regex = Regex::new(r#"(?is)<a\b([^>]*)>(.*?)</a>"#).expect("anchor regex");
    for captures in regex.captures_iter(html) {
        if out.len() >= max_elements {
            break;
        }
        let attrs = captures.get(1).map(|value| value.as_str()).unwrap_or("");
        let inner = captures.get(2).map(|value| value.as_str()).unwrap_or("");
        let Some(href) = extract_attr(attrs, "href") else {
            continue;
        };
        let href = href.trim();
        if href.is_empty()
            || href.starts_with('#')
            || href.to_ascii_lowercase().starts_with("javascript:")
        {
            continue;
        }
        let Ok(target) = resolve_url(base_url, href) else {
            continue;
        };
        let label = html_to_text(inner, 120);
        let label = if label.is_empty() {
            href.to_string()
        } else {
            label
        };
        out.push(ExtractedWebElement {
            kind: "link".to_string(),
            label,
            target: Some(target),
            field_name: None,
            value: None,
            form_id: None,
            form_action: None,
            form_method: None,
        });
    }
}

fn collect_button_elements(html: &str, max_elements: usize, out: &mut Vec<ExtractedWebElement>) {
    let regex = Regex::new(r#"(?is)<button\b([^>]*)>(.*?)</button>"#).expect("button regex");
    for captures in regex.captures_iter(html) {
        if out.len() >= max_elements {
            break;
        }
        let attrs = captures.get(1).map(|value| value.as_str()).unwrap_or("");
        let inner = captures.get(2).map(|value| value.as_str()).unwrap_or("");
        let label = first_non_empty(&[
            html_to_text(inner, 120),
            extract_attr(attrs, "aria-label").unwrap_or_default(),
            extract_attr(attrs, "title").unwrap_or_default(),
            "button".to_string(),
        ]);
        out.push(ExtractedWebElement {
            kind: "button".to_string(),
            label,
            target: None,
            field_name: None,
            value: None,
            form_id: extract_attr(attrs, "form"),
            form_action: None,
            form_method: None,
        });
    }
}

fn collect_input_elements(html: &str, max_elements: usize, out: &mut Vec<ExtractedWebElement>) {
    let regex = Regex::new(r#"(?is)<input\b([^>]*)/?>"#).expect("input regex");
    for captures in regex.captures_iter(html) {
        if out.len() >= max_elements {
            break;
        }
        let attrs = captures.get(1).map(|value| value.as_str()).unwrap_or("");
        let input_type = extract_attr(attrs, "type").unwrap_or_else(|| "input".to_string());
        let label = first_non_empty(&[
            extract_attr(attrs, "placeholder").unwrap_or_default(),
            extract_attr(attrs, "aria-label").unwrap_or_default(),
            extract_attr(attrs, "name").unwrap_or_default(),
            input_type.clone(),
        ]);
        out.push(ExtractedWebElement {
            kind: format!("input:{input_type}"),
            label,
            target: None,
            field_name: extract_attr(attrs, "name"),
            value: extract_attr(attrs, "value"),
            form_id: extract_attr(attrs, "form"),
            form_action: None,
            form_method: None,
        });
    }
}

fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    let pattern = format!(
        r#"(?is)\b{}\s*=\s*("([^"]*)"|'([^']*)'|([^\s>]+))"#,
        regex::escape(name)
    );
    let regex = Regex::new(&pattern).expect("attr regex");
    regex.captures(attrs).and_then(|captures| {
        captures
            .get(2)
            .or_else(|| captures.get(3))
            .or_else(|| captures.get(4))
            .map(|value| decode_html_entities(value.as_str()).trim().to_string())
    })
}

fn resolve_url(base_url: &str, target: &str) -> Result<String> {
    reqwest::Url::parse(base_url)
        .with_context(|| format!("invalid base URL `{base_url}`"))?
        .join(target)
        .with_context(|| format!("invalid relative URL `{target}`"))
        .map(|url| url.to_string())
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| "(unnamed)".to_string())
}

fn strip_tag_block(input: &str, tag: &str) -> String {
    let regex = Regex::new(&format!(r"(?is)<{tag}[^>]*>.*?</{tag}>")).expect("tag strip regex");
    regex.replace_all(input, " ").to_string()
}

pub fn truncate_plain_text(text: &str, max_chars: usize) -> String {
    let cleaned = clean_plain_text(text);
    truncated(cleaned, max_chars)
}

pub fn clean_plain_text(text: &str) -> String {
    let mut normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', " ");
    let multi_space = Regex::new(r"[ ]{2,}").expect("space regex");
    let multi_newline = Regex::new(r"\n{3,}").expect("newline regex");
    normalized = multi_space.replace_all(&normalized, " ").to_string();
    normalized = multi_newline.replace_all(&normalized, "\n\n").to_string();
    normalized
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_html_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

pub fn indent_block(text: &str) -> String {
    text.lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{clean_plain_text, extract_elements, extract_images, extract_title, html_to_text};

    #[test]
    fn extracts_title_from_html() {
        assert_eq!(
            extract_title("<html><head><title> Demo &amp; Test </title></head></html>").as_deref(),
            Some("Demo & Test")
        );
    }

    #[test]
    fn converts_html_to_plain_text() {
        let html = r#"
            <html>
                <body>
                    <h1>Hello</h1>
                    <p>World <strong>again</strong></p>
                    <script>ignore()</script>
                </body>
            </html>
        "#;
        let text = html_to_text(html, 200);
        assert!(text.contains("Hello"));
        assert!(text.contains("World again"));
        assert!(!text.contains("ignore()"));
    }

    #[test]
    fn normalizes_plain_text_spacing() {
        let text = clean_plain_text("one\t two\r\n\r\n\r\nthree");
        assert_eq!(text, "one two\nthree");
    }

    #[test]
    fn extracts_links_and_controls() {
        let html = r#"
            <a href="/docs/start">Start Here</a>
            <button aria-label="Submit form"></button>
            <input type="search" placeholder="Search docs" />
        "#;
        let elements = extract_elements(html, "https://example.com/guide/index.html", 10);
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0].kind, "link");
        assert_eq!(
            elements[0].target.as_deref(),
            Some("https://example.com/docs/start")
        );
        assert_eq!(elements[1].kind, "button");
        assert_eq!(elements[2].kind, "input:search");
        assert_eq!(elements[2].field_name, None);
        assert_eq!(elements[2].value, None);
    }

    #[test]
    fn extracts_images() {
        let html = r#"
            <img src="/logo.png" alt="Logo" />
            <img src="https://cdn.example.com/hero.jpg" alt="Hero" />
        "#;
        let images = extract_images(html, "https://example.com/guide/index.html", 10);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].src, "https://example.com/logo.png");
        assert_eq!(images[0].alt, "Logo");
    }
}
