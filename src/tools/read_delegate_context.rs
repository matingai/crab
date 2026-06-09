use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::delegate_runs::{DelegateRunRecord, load_record};
use crate::goal_state::GoalStateStore;
use crate::session::{SessionStore, SessionTimelineEntry, StoredSession};
use crate::todo::TodoStore;
use crate::tools::{Tool, ToolContext, truncated};
use crate::types::{ToolDefinition, object_schema};

pub struct ReadDelegateContextTool;

#[derive(Debug, Deserialize)]
struct ReadDelegateContextArgs {
    action: String,
    #[serde(default = "default_max_messages")]
    max_messages: usize,
}

fn default_max_messages() -> usize {
    8
}

#[async_trait]
impl Tool for ReadDelegateContextTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "read_delegate_context",
            "Read parent-task background from inside a delegated worker run. Use it to fetch the worker brief, parent goal state, parent todos, recent parent conversation, or a combined full background bundle.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["brief", "goal_state", "todos", "conversation", "full_background"],
                        "description": "Which background slice to return."
                    },
                    "max_messages": {
                        "type": "integer",
                        "description": "Maximum number of recent parent conversation messages to return for action=conversation or action=full_background.",
                        "minimum": 1,
                        "maximum": 50
                    }
                }),
                &["action"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: ReadDelegateContextArgs =
            serde_json::from_value(args).context("invalid read_delegate_context arguments")?;
        let delegate_run_id = ctx.current_delegate_run_id.as_deref().ok_or_else(|| {
            anyhow::anyhow!("read_delegate_context is only available inside delegated worker runs")
        })?;
        let record = load_record(&ctx.data_dir, delegate_run_id)?
            .ok_or_else(|| anyhow::anyhow!("delegate run `{delegate_run_id}` not found"))?;

        let store = SessionStore::new(&ctx.data_dir)?;
        let parent_session = store.load(&record.parent_session_id)?.ok_or_else(|| {
            anyhow::anyhow!("parent session `{}` not found", record.parent_session_id)
        })?;

        let output = match args.action.as_str() {
            "brief" => render_brief(&record),
            "goal_state" => render_goal_state(ctx, &record)?,
            "todos" => render_todos(ctx, &record)?,
            "conversation" => render_conversation(&parent_session, args.max_messages),
            "full_background" => {
                render_full_background(ctx, &record, &parent_session, args.max_messages)?
            }
            other => bail!("unsupported read_delegate_context action `{other}`"),
        };

        serde_json::to_string_pretty(&output)
            .context("failed to serialize read_delegate_context response")
    }
}

fn render_brief(record: &DelegateRunRecord) -> Value {
    json!({
        "delegate_run_id": record.id,
        "parent_session_id": record.parent_session_id,
        "worker_task": record.worker_task,
        "prompt_preview": record.prompt_preview,
        "max_iterations": record.max_iterations,
    })
}

fn render_goal_state(ctx: &ToolContext, record: &DelegateRunRecord) -> Result<Value> {
    let store = GoalStateStore::new(&ctx.data_dir)?;
    let state = store.load(&record.parent_session_id)?;
    Ok(json!({
        "parent_session_id": record.parent_session_id,
        "goal_state": state,
    }))
}

fn render_todos(ctx: &ToolContext, record: &DelegateRunRecord) -> Result<Value> {
    let store = TodoStore::new(&ctx.data_dir)?;
    let todos = store.load(&record.parent_session_id)?;
    Ok(json!({
        "parent_session_id": record.parent_session_id,
        "todos": todos,
    }))
}

fn render_conversation(parent_session: &StoredSession, max_messages: usize) -> Value {
    let recent_messages = parent_session
        .history
        .iter()
        .rev()
        .take(max_messages)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|message| {
            json!({
                "role": message.role,
                "content": truncated(message.content_text(), 800),
                "tool_call_id": message.tool_call_id,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "parent_session_id": parent_session.session_id,
        "recent_messages": recent_messages,
    })
}

fn render_full_background(
    ctx: &ToolContext,
    record: &DelegateRunRecord,
    parent_session: &StoredSession,
    max_messages: usize,
) -> Result<Value> {
    let goal_store = GoalStateStore::new(&ctx.data_dir)?;
    let todo_store = TodoStore::new(&ctx.data_dir)?;
    let goal_state = goal_store.load(&record.parent_session_id)?;
    let todos = todo_store.load(&record.parent_session_id)?;
    let recent_timeline = parent_session
        .timeline
        .iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(render_timeline_entry)
        .collect::<Vec<_>>();

    Ok(json!({
        "delegate_run_id": record.id,
        "parent_session_id": record.parent_session_id,
        "worker_task": record.worker_task,
        "goal_state": goal_state,
        "todos": todos,
        "recent_messages": render_conversation(parent_session, max_messages)["recent_messages"].clone(),
        "recent_timeline": recent_timeline,
    }))
}

fn render_timeline_entry(entry: &SessionTimelineEntry) -> String {
    match entry {
        SessionTimelineEntry::User { content, .. } => {
            format!("user: {}", truncated(content, 220))
        }
        SessionTimelineEntry::Assistant { content, .. } => {
            format!("assistant: {}", truncated(content, 220))
        }
        SessionTimelineEntry::Tool {
            name,
            detail,
            phase,
            execution_mode,
            ..
        } => format!(
            "tool {} [{}{}]: {}",
            name,
            format!("{phase:?}").to_lowercase(),
            execution_mode
                .as_ref()
                .map(|mode| format!(", {mode}"))
                .unwrap_or_default(),
            truncated(detail, 220)
        ),
        SessionTimelineEntry::Batch {
            batch_id,
            iteration,
            completed_calls,
            total_calls,
            status,
            ..
        } => format!(
            "batch {batch_id}: iteration {iteration}, {completed_calls}/{total_calls}, {}",
            format!("{status:?}").to_lowercase()
        ),
        SessionTimelineEntry::Approval {
            tool_name, reason, ..
        } => format!("approval {}: {}", tool_name, truncated(reason, 220)),
    }
}

#[cfg(test)]
mod tests {
    use super::ReadDelegateContextTool;
    use crate::delegate_runs::{DelegateRunRecord, DelegateWorkerTask, save_record};
    use crate::session::{SessionStore, StoredSession};
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    fn test_context(root: &std::path::Path) -> ToolContext {
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
            current_session_id: "child-session".to_string(),
            current_delegate_run_id: Some("delegate-1".to_string()),
            delegate_depth: 1,
        }
    }

    #[tokio::test]
    async fn reads_worker_brief_from_delegate_record() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data_dir = tmp.path().join(".data");
        let mut session = StoredSession::new("parent-session".to_string(), "model".to_string());
        session.history.push(crate::types::ChatMessage::user(
            "Investigate the failing build",
        ));
        SessionStore::new(&data_dir)
            .expect("store")
            .save(&session)
            .expect("save session");

        save_record(
            &data_dir,
            &DelegateRunRecord {
                id: "delegate-1".to_string(),
                parent_session_id: "parent-session".to_string(),
                parent_delegate_run_id: None,
                root_delegate_run_id: "delegate-1".to_string(),
                session_id: "child-session".to_string(),
                prompt: "worker prompt".to_string(),
                prompt_preview: "worker prompt".to_string(),
                status: "running".to_string(),
                result_preview: String::new(),
                max_iterations: 4,
                attempt: 1,
                worker_task: Some(DelegateWorkerTask {
                    objective: "Find the build failure".to_string(),
                    focus_goal_id: Some("goal-fix-build".to_string()),
                    background_summary: "Current build fails after recent changes.".to_string(),
                    relevant_state: json!({"beliefs": ["trait bounds may be broken"]}),
                    allowed_tools: vec!["read_file".to_string()],
                    scope: vec!["src/types".to_string()],
                    context_access: "expanded".to_string(),
                    output_schema: Some("worker_result_v1".to_string()),
                }),
                created_at_unix: 1,
                updated_at_unix: 1,
            },
        )
        .expect("save record");

        let tool = ReadDelegateContextTool;
        let output = tool
            .execute(json!({"action": "brief"}), &test_context(tmp.path()))
            .await
            .expect("brief output");
        assert!(output.contains("Find the build failure"));
        assert!(output.contains("goal-fix-build"));
    }
}
