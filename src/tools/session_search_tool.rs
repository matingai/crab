use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::session::SessionStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct SessionSearchTool;

#[derive(Debug, Deserialize)]
struct SessionSearchArgs {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    5
}

#[async_trait]
impl Tool for SessionSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "session_search",
            "Search stored sessions for relevant past conversations and summaries.",
            object_schema(
                json!({
                    "query": {
                        "type": "string",
                        "description": "Search query for matching prior sessions."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of matching sessions to return.",
                        "minimum": 1,
                        "maximum": 50
                    }
                }),
                &["query"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: SessionSearchArgs =
            serde_json::from_value(args).context("invalid session_search arguments")?;
        if args.limit == 0 {
            bail!("session_search requires `limit` to be at least 1");
        }

        let store = SessionStore::new(&ctx.data_dir)?;
        let hits = store.search(&args.query, args.limit)?;
        if hits.is_empty() {
            return Ok(format!("no session results for query `{}`", args.query));
        }

        Ok(hits
            .into_iter()
            .map(|hit| {
                let title = hit
                    .session
                    .title
                    .unwrap_or_else(|| "(untitled)".to_string());
                format!(
                    "- [{}] {} | model={} | score={} | matches={} | matched_messages={}\n  {}",
                    hit.session.session_id,
                    title,
                    hit.session.model,
                    hit.score,
                    hit.match_count,
                    hit.matched_messages,
                    hit.snippet
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::SessionSearchTool;
    use crate::session::{SessionStore, StoredSession};
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    #[tokio::test]
    async fn session_search_returns_matching_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path().join(".data")).expect("store");
        let mut session = StoredSession::new("demo".to_string(), "gpt-5".to_string());
        session.title = Some("Rust notes".to_string());
        session.history.push(crate::types::ChatMessage::user(
            "Investigate borrow checker regression",
        ));
        store.save(&session).expect("save");

        let tool = SessionSearchTool;
        let output = tool
            .execute(
                json!({
                    "query": "borrow checker",
                    "limit": 5
                }),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: tmp.path().join(".data"),
                    shell_enabled: false,
                    skill_platform: "cli".to_string(),
                    provider_id: "openai".to_string(),
                    model: "test-model".to_string(),
                    base_url: "https://example.invalid/v1".to_string(),
                    api_key: None,
                    api_mode: crate::llm::ApiMode::ChatCompletions,
                    worker_model: None,
                    max_iterations: 4,
                    current_session_id: "test-session".to_string(),
                    current_delegate_run_id: None,
                    delegate_depth: 0,
                },
            )
            .await
            .expect("search should succeed");

        assert!(output.contains("[demo]"));
        assert!(output.contains("borrow"));
    }
}
