use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::archive_db::{ArchiveMessageSearchHit, ArchiveStore, ArchiveTurnSummary};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct ArchiveQueryTool;

#[derive(Debug, Deserialize)]
struct ArchiveQueryArgs {
    action: String,
    session_id: Option<String>,
    turn_id: Option<String>,
    tool_call_id: Option<String>,
    query: Option<String>,
    limit: Option<usize>,
}

#[async_trait]
impl Tool for ArchiveQueryTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "archive_query",
            "Read immutable raw archive records from archive.db, including turns, messages, tool calls, and searches.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["session_log", "read_turn", "read_tool", "search_messages"],
                        "description": "Archive query action."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Target session id for session_log, read_turn, or scoped search."
                    },
                    "turn_id": {
                        "type": "string",
                        "description": "Target turn id when action=read_turn."
                    },
                    "tool_call_id": {
                        "type": "string",
                        "description": "Target tool call id when action=read_tool."
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query when action=search_messages."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "description": "Maximum number of rows or summaries to return."
                    }
                }),
                &["action"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: ArchiveQueryArgs =
            serde_json::from_value(args).context("invalid archive_query arguments")?;
        let store = ArchiveStore::new(&ctx.data_dir)?;
        let limit = args.limit.unwrap_or(5).clamp(1, 100);

        match args.action.as_str() {
            "session_log" => {
                let Some(session_id) = args.session_id.as_deref() else {
                    bail!("archive_query action=session_log requires `session_id`");
                };
                render_session_log(&store, session_id, limit)
            }
            "read_turn" => {
                let Some(session_id) = args.session_id.as_deref() else {
                    bail!("archive_query action=read_turn requires `session_id`");
                };
                let Some(turn_id) = args.turn_id.as_deref() else {
                    bail!("archive_query action=read_turn requires `turn_id`");
                };
                render_turn_bundle(&store, session_id, turn_id)
            }
            "read_tool" => {
                let Some(tool_call_id) = args.tool_call_id.as_deref() else {
                    bail!("archive_query action=read_tool requires `tool_call_id`");
                };
                render_tool_call(&store, tool_call_id)
            }
            "search_messages" => {
                let Some(query) = args.query.as_deref() else {
                    bail!("archive_query action=search_messages requires `query`");
                };
                render_message_search(&store, query, args.session_id.as_deref(), limit)
            }
            other => bail!("unsupported archive_query action `{other}`"),
        }
    }
}

fn render_session_log(store: &ArchiveStore, session_id: &str, limit: usize) -> Result<String> {
    let turns = store.list_turn_summaries(session_id, limit)?;
    let mut lines = vec![
        format!("archive session log: {session_id}"),
        format!("turns_shown: {}", turns.len()),
    ];
    if turns.is_empty() {
        lines.push("- no archived turns found".to_string());
        return Ok(lines.join("\n"));
    }

    for turn in turns {
        render_turn_summary(&mut lines, &turn);
    }
    Ok(lines.join("\n"))
}

fn render_turn_summary(lines: &mut Vec<String>, turn: &ArchiveTurnSummary) {
    lines.push(format!(
        "- {} | turn_index={} | messages={} | tool_calls={} | approvals={} | events={} | updated_at_unix_ms={}",
        turn.turn_id,
        turn.turn_index,
        turn.message_count,
        turn.tool_call_count,
        turn.approval_count,
        turn.event_count,
        turn.updated_at_unix_ms
    ));
    if let Some(role) = &turn.last_message_role {
        let snippet = turn.last_message_snippet.as_deref().unwrap_or("(none)");
        lines.push(format!("  - latest {}: {}", role, snippet));
    }
    lines.push(format!(
        "  - expand: archive_query(action=\"read_turn\", session_id=\"{}\", turn_id=\"{}\")",
        turn.session_id, turn.turn_id
    ));
}

fn render_turn_bundle(store: &ArchiveStore, session_id: &str, turn_id: &str) -> Result<String> {
    let Some(bundle) = store.read_turn(session_id, turn_id)? else {
        bail!("archive turn `{turn_id}` not found in session `{session_id}`");
    };

    let mut lines = vec![
        format!("archive turn: {}", bundle.turn.turn_id),
        format!("session: {}", bundle.turn.session_id),
        format!("turn_index: {}", bundle.turn.turn_index),
        format!("created_at_unix_ms: {}", bundle.turn.created_at_unix_ms),
        format!("updated_at_unix_ms: {}", bundle.turn.updated_at_unix_ms),
    ];

    lines.push("messages:".to_string());
    if bundle.messages.is_empty() {
        lines.push("- none".to_string());
    } else {
        for message in &bundle.messages {
            lines.push(format!(
                "- [{}] {} @{}",
                message.role, message.id, message.created_at_unix_ms
            ));
            lines.push(format!("  content: {}", clip_block(&message.content, 600)));
            if let Some(raw_json) = &message.raw_json {
                lines.push(format!("  raw_json: {}", clip_block(raw_json, 400)));
            }
            if let Some(tool_call_id) = &message.tool_call_id {
                lines.push(format!("  tool_call_id: {tool_call_id}"));
            }
        }
    }

    lines.push("tool_calls:".to_string());
    if bundle.tool_calls.is_empty() {
        lines.push("- none".to_string());
    } else {
        for tool_call in &bundle.tool_calls {
            render_tool_call_summary(&mut lines, tool_call);
        }
    }

    lines.push("approvals:".to_string());
    if bundle.approvals.is_empty() {
        lines.push("- none".to_string());
    } else {
        for approval in &bundle.approvals {
            lines.push(format!(
                "- [{}] {} | reason={} | command={}",
                approval.tool_name,
                approval.id,
                clip_line(&approval.reason, 120),
                clip_line(&approval.command, 120)
            ));
        }
    }

    lines.push("events:".to_string());
    if bundle.events.is_empty() {
        lines.push("- none".to_string());
    } else {
        for event in &bundle.events {
            lines.push(format!(
                "- [{}] {} | {}",
                event.event_type,
                event.title,
                clip_line(&event.summary, 160)
            ));
        }
    }

    lines.push("expand:".to_string());
    lines.push(format!(
        "- archive_query(action=\"session_log\", session_id=\"{}\")",
        session_id
    ));
    lines.push(format!(
        "- memory_query(action=\"timeline\", session_id=\"{}\")",
        session_id
    ));
    Ok(lines.join("\n"))
}

fn render_tool_call(store: &ArchiveStore, tool_call_id: &str) -> Result<String> {
    let Some(tool_call) = store.read_tool_call(tool_call_id)? else {
        bail!("archive tool call `{tool_call_id}` not found");
    };
    let approvals = store.read_approvals_for_tool_call(tool_call_id)?;
    let messages = store.read_messages_for_tool_call(tool_call_id)?;

    let mut lines = vec![
        format!("archive tool call: {}", tool_call.id),
        format!("session: {}", tool_call.session_id),
        format!("turn: {}", tool_call.turn_id),
        format!("tool: {}", tool_call.tool_name),
        format!("phase: {}", tool_call.phase),
        format!("execution_mode: {}", tool_call.execution_mode),
    ];
    if let Some(arguments_raw) = &tool_call.arguments_raw {
        lines.push(format!("arguments_raw: {}", clip_block(arguments_raw, 600)));
    }
    if let Some(output_raw) = &tool_call.output_raw {
        lines.push(format!("output_raw: {}", clip_block(output_raw, 600)));
    }

    lines.push("messages:".to_string());
    if messages.is_empty() {
        lines.push("- none".to_string());
    } else {
        for message in messages {
            lines.push(format!(
                "- [{}] {} @{}",
                message.role, message.id, message.created_at_unix_ms
            ));
            lines.push(format!("  content: {}", clip_block(&message.content, 300)));
        }
    }

    lines.push("approvals:".to_string());
    if approvals.is_empty() {
        lines.push("- none".to_string());
    } else {
        for approval in approvals {
            lines.push(format!(
                "- {} | reason={} | command={}",
                approval.id,
                clip_line(&approval.reason, 120),
                clip_line(&approval.command, 120)
            ));
        }
    }

    lines.push("expand:".to_string());
    lines.push(format!(
        "- archive_query(action=\"read_turn\", session_id=\"{}\", turn_id=\"{}\")",
        tool_call.session_id, tool_call.turn_id
    ));
    Ok(lines.join("\n"))
}

fn render_tool_call_summary(
    lines: &mut Vec<String>,
    tool_call: &crate::archive_db::ArchiveToolCallRecord,
) {
    lines.push(format!(
        "- [{}] {} | phase={} | mode={} | updated_at_unix_ms={}",
        tool_call.tool_name,
        tool_call.id,
        tool_call.phase,
        tool_call.execution_mode,
        tool_call.updated_at_unix_ms
    ));
    if let Some(arguments_raw) = &tool_call.arguments_raw {
        lines.push(format!(
            "  arguments_raw: {}",
            clip_block(arguments_raw, 300)
        ));
    }
    if let Some(output_raw) = &tool_call.output_raw {
        lines.push(format!("  output_raw: {}", clip_block(output_raw, 300)));
    }
    lines.push(format!(
        "  expand: archive_query(action=\"read_tool\", tool_call_id=\"{}\")",
        tool_call.id
    ));
}

fn render_message_search(
    store: &ArchiveStore,
    query: &str,
    session_id: Option<&str>,
    limit: usize,
) -> Result<String> {
    let hits = store.search_messages(query, session_id, limit)?;
    let scope = session_id.unwrap_or("all");
    let mut lines = vec![
        format!("archive message search: `{query}`"),
        format!("scope: {scope}"),
    ];
    if hits.is_empty() {
        lines.push("- no results".to_string());
        return Ok(lines.join("\n"));
    }

    for hit in hits {
        render_search_hit(&mut lines, &hit);
    }
    Ok(lines.join("\n"))
}

fn render_search_hit(lines: &mut Vec<String>, hit: &ArchiveMessageSearchHit) {
    lines.push(format!(
        "- [{}] {} / {} / {} @{}",
        hit.message.role,
        hit.message.session_id,
        hit.message.turn_id,
        hit.message.id,
        hit.message.created_at_unix_ms
    ));
    lines.push(format!("  {}", hit.snippet));
    lines.push(format!(
        "  expand: archive_query(action=\"read_turn\", session_id=\"{}\", turn_id=\"{}\")",
        hit.message.session_id, hit.message.turn_id
    ));
}

fn clip_line(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ").trim().to_string();
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        normalized.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn clip_block(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        trimmed.chars().take(max_chars).collect::<String>() + "..."
    }
}

#[cfg(test)]
mod tests {
    use super::ArchiveQueryTool;
    use crate::archive_db::ArchiveStore;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    fn tool_context(tmp: &tempfile::TempDir) -> ToolContext {
        ToolContext {
            workspace_root: tmp.path().to_path_buf(),
            data_dir: tmp.path().to_path_buf(),
            shell_enabled: false,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            api_mode: crate::llm::ApiMode::ChatCompletions,
            worker_model: None,
            max_iterations: 4,
            current_session_id: "session-a".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    fn seed_archive(store: &ArchiveStore, root: &std::path::Path) {
        store
            .upsert_session("session-a", Some("Demo"), root, "openai", "gpt-test", 1, 2)
            .expect("session");
        store
            .upsert_turn("session-a", "turn-1", 1, 10)
            .expect("turn");
        store
            .append_message(
                "session-a",
                "turn-1",
                "assistant",
                "Adjusted the dialogue UI and collapsed tool cards.",
                Some("tool-1"),
                Some("{\"kind\":\"assistant\"}"),
            )
            .expect("message");
        store
            .upsert_tool_call(
                "tool-1",
                "session-a",
                "turn-1",
                "read_file",
                "sequential",
                None,
                None,
                None,
                "done",
                Some("{\"path\":\"src/ui/chat.tsx\"}"),
                Some("tsx content"),
            )
            .expect("tool");
        store
            .append_event(
                "session-a",
                Some("turn-1"),
                "assistant_message",
                "Assistant replied",
                "Layout updated",
            )
            .expect("event");
    }

    #[tokio::test]
    async fn reads_turn_and_tool_archive_views() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ArchiveStore::new(tmp.path()).expect("archive");
        seed_archive(&store, tmp.path());

        let tool = ArchiveQueryTool;
        let output = tool
            .execute(
                json!({
                    "action": "read_turn",
                    "session_id": "session-a",
                    "turn_id": "turn-1"
                }),
                &tool_context(&tmp),
            )
            .await
            .expect("read_turn should succeed");

        assert!(output.contains("archive turn: turn-1"));
        assert!(output.contains("tool_calls:"));
        assert!(output.contains("archive_query(action=\"read_tool\", tool_call_id=\"tool-1\")"));

        let tool_output = tool
            .execute(
                json!({
                    "action": "read_tool",
                    "tool_call_id": "tool-1"
                }),
                &tool_context(&tmp),
            )
            .await
            .expect("read_tool should succeed");
        assert!(tool_output.contains("tool: read_file"));
        assert!(tool_output.contains("arguments_raw:"));
    }

    #[tokio::test]
    async fn searches_archive_messages_and_lists_session_log() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ArchiveStore::new(tmp.path()).expect("archive");
        seed_archive(&store, tmp.path());

        let tool = ArchiveQueryTool;
        let search = tool
            .execute(
                json!({
                    "action": "search_messages",
                    "query": "tool cards",
                    "limit": 5
                }),
                &tool_context(&tmp),
            )
            .await
            .expect("search should succeed");
        assert!(search.contains("archive message search"));
        assert!(search.contains(
            "archive_query(action=\"read_turn\", session_id=\"session-a\", turn_id=\"turn-1\")"
        ));

        let session_log = tool
            .execute(
                json!({
                    "action": "session_log",
                    "session_id": "session-a",
                    "limit": 5
                }),
                &tool_context(&tmp),
            )
            .await
            .expect("session log should succeed");
        assert!(session_log.contains("archive session log: session-a"));
        assert!(session_log.contains("messages=1"));
    }
}
