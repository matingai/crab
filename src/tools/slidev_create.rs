use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;

use crate::tools::slidev_preview::start_slidev_preview;
use crate::tools::{Tool, ToolContext, relative_display, resolve_workspace_path};
use crate::types::{ToolDefinition, object_schema};

pub struct SlidevCreateTool;

#[derive(Debug, Deserialize)]
struct SlidevCreateArgs {
    #[serde(default)]
    output_path: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    theme: Option<String>,
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    slides: Option<Vec<SlideSpec>>,
    #[serde(default)]
    start_preview: Option<bool>,
    #[serde(default)]
    preview_port: Option<u16>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SlideSpec {
    title: String,
    #[serde(default)]
    bullets: Vec<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    layout: Option<String>,
}

const DEFAULT_SLIDEV_OUTPUT_PATH: &str = "slides.md";

#[async_trait]
impl Tool for SlidevCreateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "slidev_create",
            "Create a new Slidev deck Markdown file. By default it starts a local preview server after writing the deck so presentation work can continue in the browser immediately.",
            object_schema(
                json!({
                    "output_path": {
                        "type": "string",
                        "description": "Optional path relative to the workspace root. Defaults to `slides.md`."
                    },
                    "title": {
                        "type": "string",
                        "description": "Optional deck title."
                    },
                    "subtitle": {
                        "type": "string",
                        "description": "Optional deck subtitle for the cover slide."
                    },
                    "theme": {
                        "type": "string",
                        "description": "Optional Slidev theme. Defaults to the template's preferred theme."
                    },
                    "template": {
                        "type": "string",
                        "description": "Optional built-in template. Supported values: `default`, `pitch`, `product-launch`, `research`, `weekly-review`."
                    },
                    "slides": {
                        "type": "array",
                        "description": "Optional explicit slides. When omitted, a scaffold is generated from the template.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string" },
                                "bullets": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "body": { "type": "string" },
                                "layout": { "type": "string" }
                            },
                            "required": ["title"]
                        }
                    },
                    "start_preview": {
                        "type": "boolean",
                        "description": "Whether to start Slidev preview automatically after writing the deck. Defaults to true."
                    },
                    "preview_port": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 65535,
                        "description": "Optional local preview port."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60,
                        "description": "Optional Slidev preview readiness timeout."
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: SlidevCreateArgs =
            serde_json::from_value(args).context("invalid slidev_create arguments")?;
        let output_path = resolve_workspace_path(
            &ctx.workspace_root,
            args.output_path
                .as_deref()
                .unwrap_or(DEFAULT_SLIDEV_OUTPUT_PATH),
        )?;
        ensure_markdown_output_path(&output_path)?;
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let content = render_slidev_deck(&args);
        fs::write(&output_path, &content)
            .with_context(|| format!("failed to write {}", output_path.display()))?;

        let start_preview = args.start_preview.unwrap_or(true);
        let mut lines = vec![
            format!(
                "created slidev deck\nfile: {}",
                relative_display(&ctx.workspace_root, &output_path)
            ),
            format!(
                "template: {}",
                normalized_template_name(args.template.as_deref())
            ),
        ];

        if start_preview {
            match start_slidev_preview(
                &ctx.workspace_root,
                &ctx.data_dir,
                &ctx.current_session_id,
                &output_path,
                args.preview_port,
                args.timeout_seconds,
            )
            .await
            {
                Ok(preview) => {
                    lines.push(format!("preview_url: {}", preview.url));
                    lines.push(format!("preview_port: {}", preview.port));
                    lines.push(format!("preview_pid: {}", preview.pid));
                }
                Err(error) => {
                    lines.push(format!("preview_warning: {}", error));
                }
            }
        }

        Ok(lines.join("\n"))
    }
}

fn render_slidev_deck(args: &SlidevCreateArgs) -> String {
    let template = normalized_template_name(args.template.as_deref());
    let theme = args
        .theme
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_theme_for_template(template));
    let title = normalized_text(args.title.as_deref(), default_title_for_template(template));
    let subtitle = args
        .subtitle
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_subtitle_for_template(template));
    let slides = args
        .slides
        .as_deref()
        .map(render_explicit_slides)
        .unwrap_or_else(|| render_template_scaffold(template));

    format!(
        "---\ntheme: {}\ntitle: {}\ninfo: {}\ntransition: slide-left\nmdc: true\n---\n\n# {}\n{}\n\n---\n\n{}",
        theme,
        escape_frontmatter(&title),
        escape_frontmatter(subtitle),
        title,
        subtitle,
        slides
    )
}

fn render_explicit_slides(slides: &[SlideSpec]) -> String {
    if slides.is_empty() {
        return render_template_scaffold("default");
    }

    slides
        .iter()
        .enumerate()
        .map(|(index, slide)| render_explicit_slide(index, slide))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

fn render_explicit_slide(index: usize, slide: &SlideSpec) -> String {
    let mut lines = Vec::new();
    if let Some(layout) = slide
        .layout
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push("---".to_string());
        lines.push(format!("layout: {}", layout));
        lines.push("---".to_string());
        lines.push(String::new());
    } else if index == 0 {
        lines.push("---".to_string());
        lines.push("layout: section".to_string());
        lines.push("---".to_string());
        lines.push(String::new());
    }
    lines.push(format!("# {}", slide.title.trim()));
    lines.push(String::new());
    if let Some(body) = slide
        .body
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(body.to_string());
        if !slide.bullets.is_empty() {
            lines.push(String::new());
        }
    }
    for bullet in &slide.bullets {
        let bullet = bullet.trim();
        if !bullet.is_empty() {
            lines.push(format!("- {}", bullet));
        }
    }
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

fn render_template_scaffold(template: &str) -> String {
    match template {
        "pitch" => [
            "---\nlayout: section\n---\n\n# Problem\n\n- What is broken today\n- Who feels the pain\n- Why now",
            "# Solution\n\n- Product promise\n- Core workflow\n- Clear differentiation",
            "# Market & GTM\n\n- Ideal customer\n- Adoption wedge\n- Revenue motion",
            "# Product Snapshot\n\n- Key feature 1\n- Key feature 2\n- Key feature 3",
            "# Ask\n\n- What you need next\n- Team / timeline\n- Desired outcome",
            "---\nlayout: end\n---\n\n# Thank You\n\nQuestions?",
        ]
        .join("\n\n---\n\n"),
        "product-launch" => [
            "---\nlayout: section\n---\n\n# Launch Story\n\n- Why this release matters\n- Customer pain it solves\n- Success metric",
            "# What's New\n\n- Highlight 1\n- Highlight 2\n- Highlight 3",
            "# Demo Flow\n\n- Entry point\n- Key interaction\n- Expected result",
            "# Rollout Plan\n\n- Audience\n- Channels\n- Launch checklist",
            "---\nlayout: end\n---\n\n# Ready to Launch\n\nLet's ship it.",
        ]
        .join("\n\n---\n\n"),
        "research" => [
            "---\nlayout: section\n---\n\n# Research Question\n\n- Scope\n- Hypothesis\n- Decision this deck should unlock",
            "# Method\n\n- Sources\n- Approach\n- Constraints",
            "# Key Findings\n\n- Finding 1\n- Finding 2\n- Finding 3",
            "# Implications\n\n- What changed\n- What remains uncertain\n- What to test next",
            "---\nlayout: end\n---\n\n# Recommendation\n\n- Best next action\n- Owner\n- Timing",
        ]
        .join("\n\n---\n\n"),
        "weekly-review" => [
            "---\nlayout: section\n---\n\n# Weekly Review\n\n- Theme of the week\n- Top signal\n- Top risk",
            "# Progress\n\n- Shipped\n- In progress\n- Blocked",
            "# Metrics\n\n- KPI 1\n- KPI 2\n- KPI 3",
            "# Next Week\n\n- Priority 1\n- Priority 2\n- Priority 3",
            "---\nlayout: end\n---\n\n# Decisions Needed\n\n- Ask 1\n- Ask 2",
        ]
        .join("\n\n---\n\n"),
        _ => [
            "---\nlayout: section\n---\n\n# Agenda\n\n- Context\n- Main idea\n- Next steps",
            "# Key Point\n\n- Signal 1\n- Signal 2\n- Signal 3",
            "# Details\n\nAdd charts, screenshots, or speaker notes here.",
            "---\nlayout: end\n---\n\n# Thank You\n\nQuestions?",
        ]
        .join("\n\n---\n\n"),
    }
}

fn default_theme_for_template(template: &str) -> &'static str {
    match template {
        "weekly-review" => "default",
        _ => "seriph",
    }
}

fn default_title_for_template(template: &str) -> &'static str {
    match template {
        "pitch" => "Pitch Deck",
        "product-launch" => "Product Launch",
        "research" => "Research Review",
        "weekly-review" => "Weekly Review",
        _ => "Slidev Deck",
    }
}

fn default_subtitle_for_template(template: &str) -> &'static str {
    match template {
        "pitch" => "A concise story for the room.",
        "product-launch" => "What is launching, why it matters, and how it lands.",
        "research" => "Evidence, implications, and a recommended next move.",
        "weekly-review" => "Progress, metrics, blockers, and next steps.",
        _ => "Drafted with Slidev.",
    }
}

fn normalized_template_name(value: Option<&str>) -> &str {
    match value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("default")
        .to_ascii_lowercase()
        .as_str()
    {
        "pitch" | "startup-pitch" => "pitch",
        "product-launch" | "launch" => "product-launch",
        "research" | "research-report" => "research",
        "weekly-review" | "review" => "weekly-review",
        _ => "default",
    }
}

fn normalized_text(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn ensure_markdown_output_path(path: &std::path::Path) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "md" | "mdx") {
        Ok(())
    } else {
        bail!("slidev_create supports only .md and .mdx output paths");
    }
}

fn escape_frontmatter(value: &str) -> String {
    value.replace('\n', " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::SlidevCreateTool;
    use crate::tools::{Tool, ToolContext};
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
            max_iterations: 4,
            current_session_id: "session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[tokio::test]
    async fn creates_slidev_deck_without_preview() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = SlidevCreateTool;
        let output = tool
            .execute(
                json!({
                    "title": "Roadmap Deck",
                    "template": "weekly-review",
                    "start_preview": false
                }),
                &ctx(tmp.path()),
            )
            .await
            .expect("create");

        let deck = std::fs::read_to_string(tmp.path().join("slides.md")).expect("read deck");
        assert!(output.contains("created slidev deck"));
        assert!(deck.contains("theme: default"));
        assert!(deck.contains("title: Roadmap Deck"));
        assert!(deck.contains("# Roadmap Deck"));
    }
}
