use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::memory_semantic::{build_semantic_memory_digest, load_session_for_semantic_digest};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct MemoryDigestTool;

#[derive(Debug, Deserialize)]
struct MemoryDigestArgs {
    session_id: Option<String>,
    query: Option<String>,
    format: Option<String>,
}

#[async_trait]
impl Tool for MemoryDigestTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "memory_digest",
            "Build a compact semantic digest of prior conversation state for the current or specified session. Useful for compressing memory into objective, constraints, recent signals, open loops, and expand hints.",
            object_schema(
                json!({
                    "session_id": {
                        "type": "string",
                        "description": "Optional session id. Defaults to the current session, or the latest session if the current one is unavailable."
                    },
                    "query": {
                        "type": "string",
                        "description": "Optional query to condition which memories are recalled into the digest."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["markdown", "json"],
                        "description": "Response format. Defaults to markdown."
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: MemoryDigestArgs =
            serde_json::from_value(args).context("invalid memory_digest arguments")?;
        let requested_session_id = args
            .session_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(Some(ctx.current_session_id.as_str()));
        let session = load_session_for_semantic_digest(&ctx.data_dir, requested_session_id)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no session found for memory digest (requested session: {})",
                    requested_session_id.unwrap_or("(latest)")
                )
            })?;
        let digest = build_semantic_memory_digest(
            &ctx.data_dir,
            &session,
            args.query.as_deref().unwrap_or(""),
        )?;
        match args
            .format
            .as_deref()
            .unwrap_or("markdown")
            .trim()
            .to_lowercase()
            .as_str()
        {
            "markdown" => Ok(digest.render_markdown()),
            "json" => Ok(serde_json::to_string_pretty(&digest)?),
            other => bail!("unsupported memory_digest format `{other}`"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryDigestTool;
    use crate::session::{SessionStore, StoredSession};
    use crate::tools::{Tool, ToolContext};
    use crate::types::ChatMessage;
    use serde_json::json;

    #[tokio::test]
    async fn renders_markdown_digest_for_current_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let session_store = SessionStore::new(tmp.path()).expect("session store");
        let mut session = StoredSession::new("session-a".to_string(), "gpt-test".to_string());
        session
            .history
            .push(ChatMessage::user("不要太多启发式，优先稳定。"));
        session
            .history
            .push(ChatMessage::assistant("先做 memory digest 工具。"));
        session_store.save(&session).expect("save");

        let tool = MemoryDigestTool;
        let output = tool
            .execute(
                json!({}),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: tmp.path().to_path_buf(),
                    shell_enabled: false,
                    skill_platform: "test".to_string(),
                    provider_id: "test".to_string(),
                    model: "gpt-test".to_string(),
                    base_url: "mock://test".to_string(),
                    api_key: None,
                    api_mode: crate::llm::ApiMode::ChatCompletions,
                    max_iterations: 8,
                    current_session_id: "session-a".to_string(),
                    current_delegate_run_id: None,
                    delegate_depth: 0,
                },
            )
            .await
            .expect("output");

        assert!(output.contains("<memory-semantic-digest>"));
        assert!(output.contains("objective:"));
        assert!(output.contains("constraints:"));
    }
}
