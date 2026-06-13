use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::experience_store::ExperienceStore;
use crate::meta_pattern_store::MetaPatternStore;
use crate::session::SessionStore;
use crate::solve_trace::SolveTraceStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct ContextModuleTool;

#[derive(Debug, Deserialize)]
struct ContextModuleArgs {
    module: String,
    query: Option<String>,
    session_id: Option<String>,
}

#[async_trait]
impl Tool for ContextModuleTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "context_module",
            "Load optional prior solve-process context on demand instead of keeping it preloaded in every prompt.",
            object_schema(
                json!({
                    "module": {
                        "type": "string",
                        "enum": ["solve_trace", "meta_pattern", "experience"],
                        "description": "Which optional context module to load."
                    },
                    "query": {
                        "type": "string",
                        "description": "Optional query used to rank the most relevant items. Defaults to the current task if omitted."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional session id. Defaults to the current session."
                    }
                }),
                &["module"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: ContextModuleArgs =
            serde_json::from_value(args).context("invalid context_module arguments")?;
        let session_id = args
            .session_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(ctx.current_session_id.as_str());
        let query = resolve_query(&ctx.data_dir, session_id, args.query.as_deref())?;

        match args.module.trim() {
            "solve_trace" => {
                let store = SolveTraceStore::new(&ctx.data_dir)?;
                Ok(store
                    .build_context_block(session_id, &query, None)?
                    .unwrap_or_else(|| {
                        format!("no solve_trace context available for session `{session_id}`")
                    }))
            }
            "meta_pattern" => {
                let store = MetaPatternStore::new(&ctx.data_dir)?;
                Ok(store
                    .build_context_block(session_id, &query)?
                    .unwrap_or_else(|| {
                        format!("no meta_pattern context available for session `{session_id}`")
                    }))
            }
            "experience" => {
                let store = ExperienceStore::new(&ctx.data_dir)?;
                Ok(store
                    .build_context_block(session_id, &query, None)?
                    .unwrap_or_else(|| {
                        format!("no experience context available for session `{session_id}`")
                    }))
            }
            other => bail!("unsupported context_module `{other}`"),
        }
    }
}

fn resolve_query(
    data_dir: &std::path::Path,
    session_id: &str,
    query: Option<&str>,
) -> Result<String> {
    if let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(query.to_string());
    }

    let store = SessionStore::new(data_dir)?;
    let session = store
        .load(session_id)?
        .ok_or_else(|| anyhow::anyhow!("session `{session_id}` not found"))?;

    let fallback = session
        .history
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content_text())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| session.title.clone())
        .unwrap_or_else(|| "current task".to_string());
    Ok(fallback)
}

#[cfg(test)]
mod tests {
    use super::ContextModuleTool;
    use crate::session::{SessionStore, StoredSession};
    use crate::solve_trace::{SolveEpisode, SolveOutcome, SolveTraceState, SolveTraceStore};
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    #[tokio::test]
    async fn loads_solve_trace_context_for_current_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data_dir = tmp.path().join(".data");
        let session_store = SessionStore::new(&data_dir).expect("store");
        let mut session = StoredSession::new("session-a".to_string(), "gpt-test".to_string());
        session
            .history
            .push(crate::types::ChatMessage::user("fix the build failure"));
        session_store.save(&session).expect("save session");

        let trace_store = SolveTraceStore::new(&data_dir).expect("trace store");
        let state = SolveTraceState {
            episodes: vec![SolveEpisode {
                id: "episode-a".to_string(),
                turn_id: "turn-1".to_string(),
                goal: "Fix build".to_string(),
                user_input: "fix the build failure".to_string(),
                focus_goal_id: None,
                focus_goal_title: None,
                status: "completed".to_string(),
                supplements: vec!["checked compiler output".to_string()],
                steps: Vec::new(),
                decisions: Vec::new(),
                outcome: Some(SolveOutcome {
                    status: "completed".to_string(),
                    summary: "Build issue isolated to trait bounds".to_string(),
                    next_focus: None,
                    created_at_unix: 1,
                }),
                created_at_unix: 1,
                updated_at_unix: 1,
            }],
        };
        trace_store.save("session-a", &state).expect("save trace");

        let tool = ContextModuleTool;
        let output = tool
            .execute(
                json!({ "module": "solve_trace" }),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: data_dir.clone(),
                    shell_enabled: false,
                    skill_platform: "cli".to_string(),
                    provider_id: "openai".to_string(),
                    model: "test-model".to_string(),
                    base_url: "https://example.invalid/v1".to_string(),
                    api_key: None,
                    api_mode: crate::llm::ApiMode::ChatCompletions,
                    max_iterations: 4,
                    current_session_id: "session-a".to_string(),
                    current_delegate_run_id: None,
                    delegate_depth: 0,
                },
            )
            .await
            .expect("context_module should succeed");

        assert!(output.contains("<solve-trace-context>"));
        assert!(output.contains("Fix build"));
    }
}
