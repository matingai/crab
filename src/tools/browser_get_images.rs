use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::browser_state::BrowserStateStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct BrowserGetImagesTool;

#[async_trait]
impl Tool for BrowserGetImagesTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "browser_get_images",
            "List images extracted from the current stored browser page.",
            object_schema(json!({}), &[]),
        )
    }

    async fn execute(&self, _args: Value, ctx: &ToolContext) -> Result<String> {
        let store = BrowserStateStore::new(&ctx.data_dir)?;
        let session = store.load(&ctx.current_session_id)?.ok_or_else(|| {
            anyhow!("no browser page stored for this session; call browser_navigate first")
        })?;
        let images = &session.current.images;
        if images.is_empty() {
            return Ok("no images found on current browser page".to_string());
        }
        Ok(images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                format!(
                    "- [{}] {}{}",
                    index + 1,
                    image.src,
                    if image.alt.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" | alt={}", image.alt)
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::BrowserGetImagesTool;
    use crate::browser_state::{
        BrowserImage, BrowserPageState, BrowserSessionState, BrowserStateStore,
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
    async fn browser_get_images_lists_page_images() {
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
                    Vec::new(),
                    vec![BrowserImage {
                        src: "https://example.com/logo.png".to_string(),
                        alt: "Logo".to_string(),
                    }],
                    false,
                )),
            )
            .expect("save");

        let tool = BrowserGetImagesTool;
        let output = tool
            .execute(json!({}), &ctx(tmp.path()))
            .await
            .expect("images");
        assert!(output.contains("logo.png"));
        assert!(output.contains("alt=Logo"));
    }
}
