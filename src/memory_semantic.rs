use anyhow::Result;
use serde::Serialize;
use std::path::Path;

use crate::memory::MemoryStore;
use crate::session::{SessionStore, StoredBatchStatus, StoredSession, StoredToolPhase};
use crate::types::ChatMessage;

const MAX_RECENT_USERS: usize = 3;
const MAX_RECENT_ASSISTANT: usize = 2;
const MAX_RECENT_SIGNALS: usize = 4;
const MAX_OPEN_LOOPS: usize = 4;
const MAX_MEMORY_HITS: usize = 3;

#[derive(Debug, Clone, Serialize)]
pub struct SemanticMemoryDigest {
    pub session_id: String,
    pub title: Option<String>,
    pub query: String,
    pub objective: String,
    pub constraints: Vec<String>,
    pub recent_user_turns: Vec<String>,
    pub recent_assistant_points: Vec<String>,
    pub recent_signals: Vec<String>,
    pub open_loops: Vec<String>,
    pub recalled_memory: Vec<String>,
    pub expand_hints: Vec<String>,
    pub message_count: usize,
    pub user_turn_count: usize,
}

impl SemanticMemoryDigest {
    pub fn render_markdown(&self) -> String {
        let mut lines = vec![
            "<memory-semantic-digest>".to_string(),
            "[System note: The following is a compact semantic digest of prior conversation state. Treat it as background context. Expand only when needed.]".to_string(),
            String::new(),
            format!("session_id: {}", self.session_id),
        ];

        if let Some(title) = self
            .title
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("title: {}", inline_clip(title, 120)));
        }
        if !self.query.trim().is_empty() {
            lines.push(format!("query: {}", inline_clip(&self.query, 120)));
        }
        lines.push(format!("objective: {}", inline_clip(&self.objective, 180)));
        lines.push(format!(
            "stats: messages={} | user_turns={}",
            self.message_count, self.user_turn_count
        ));

        render_section(&mut lines, "constraints", &self.constraints);
        render_section(&mut lines, "recent_user_turns", &self.recent_user_turns);
        render_section(
            &mut lines,
            "recent_assistant_points",
            &self.recent_assistant_points,
        );
        render_section(&mut lines, "recent_signals", &self.recent_signals);
        render_section(&mut lines, "open_loops", &self.open_loops);
        render_section(&mut lines, "recalled_memory", &self.recalled_memory);
        render_section(&mut lines, "expand_hints", &self.expand_hints);

        lines.push("</memory-semantic-digest>".to_string());
        lines.join("\n")
    }
}

pub fn build_semantic_memory_digest(
    data_dir: &Path,
    session: &StoredSession,
    query: &str,
) -> Result<SemanticMemoryDigest> {
    let recent_users = recent_messages(&session.history, "user", MAX_RECENT_USERS);
    let recent_assistant = recent_messages(&session.history, "assistant", MAX_RECENT_ASSISTANT);
    let objective = recent_users
        .last()
        .cloned()
        .or_else(|| {
            session
                .title
                .as_ref()
                .map(|title| format!("Continue session: {title}"))
        })
        .unwrap_or_else(|| "Continue from the current conversation state.".to_string());

    let constraints = extract_constraints(&recent_users);
    let recent_signals = extract_recent_signals(session);
    let open_loops = extract_open_loops(session, &recent_users, &recent_assistant);
    let recalled_memory = recall_memory(data_dir, query, &objective)?;
    let expand_hints = build_expand_hints(session);

    Ok(SemanticMemoryDigest {
        session_id: session.session_id.clone(),
        title: session.title.clone(),
        query: query.trim().to_string(),
        objective,
        constraints,
        recent_user_turns: recent_users
            .into_iter()
            .map(|item| inline_clip(&item, 140))
            .collect(),
        recent_assistant_points: recent_assistant
            .into_iter()
            .map(|item| inline_clip(&item, 140))
            .collect(),
        recent_signals,
        open_loops,
        recalled_memory,
        expand_hints,
        message_count: session.history.len(),
        user_turn_count: session
            .history
            .iter()
            .filter(|message| message.role == "user")
            .count(),
    })
}

pub fn load_session_for_semantic_digest(
    data_dir: &Path,
    session_id: Option<&str>,
) -> Result<Option<StoredSession>> {
    let store = SessionStore::new(data_dir.to_path_buf())?;
    if let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) {
        return store.load(session_id);
    }
    Ok(store.list()?.into_iter().next())
}

fn recent_messages(history: &[ChatMessage], role: &str, limit: usize) -> Vec<String> {
    history
        .iter()
        .rev()
        .filter(|message| message.role == role)
        .map(ChatMessage::content_text)
        .filter(|text| !text.trim().is_empty())
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn extract_constraints(recent_users: &[String]) -> Vec<String> {
    let hints = [
        "不要", "不能", "必须", "优先", "最好", "别", "please", "must", "should", "prefer",
        "avoid", "don't",
    ];
    dedupe_preserve_order(
        recent_users
            .iter()
            .flat_map(|message| message.split(['\n', '。', '.', ';', '；']))
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| {
                let lowered = line.to_lowercase();
                hints
                    .iter()
                    .any(|hint| line.contains(hint) || lowered.contains(&hint.to_lowercase()))
            })
            .map(|line| inline_clip(line, 140))
            .collect(),
    )
}

fn extract_recent_signals(session: &StoredSession) -> Vec<String> {
    let mut signals = Vec::new();
    for entry in session.timeline.iter().rev() {
        let signal = match entry {
            crate::session::SessionTimelineEntry::Tool {
                name,
                detail,
                phase,
                ..
            } => Some(format!(
                "tool {} [{}]: {}",
                name,
                match phase {
                    StoredToolPhase::Running => "running",
                    StoredToolPhase::Done => "done",
                    StoredToolPhase::Error => "error",
                    StoredToolPhase::Approval => "approval",
                },
                inline_clip(detail, 120)
            )),
            crate::session::SessionTimelineEntry::Batch {
                status,
                completed_calls,
                total_calls,
                ..
            } => Some(format!(
                "tool batch [{}]: {}/{} calls",
                match status {
                    StoredBatchStatus::Running => "running",
                    StoredBatchStatus::Completed => "completed",
                    StoredBatchStatus::CompletedWithErrors => "completed_with_errors",
                    StoredBatchStatus::AwaitingApproval => "awaiting_approval",
                    StoredBatchStatus::Canceled => "canceled",
                },
                completed_calls,
                total_calls
            )),
            crate::session::SessionTimelineEntry::Approval {
                tool_name, reason, ..
            } => Some(format!(
                "approval needed for {}: {}",
                tool_name,
                inline_clip(reason, 120)
            )),
            _ => None,
        };
        if let Some(signal) = signal {
            signals.push(signal);
        }
        if signals.len() >= MAX_RECENT_SIGNALS {
            break;
        }
    }
    signals.reverse();
    signals
}

fn extract_open_loops(
    session: &StoredSession,
    recent_users: &[String],
    recent_assistant: &[String],
) -> Vec<String> {
    let mut loops = Vec::new();

    if let Some(last_user) = recent_users.last() {
        loops.push(format!(
            "Resolve the latest user request: {}",
            inline_clip(last_user, 120)
        ));
    }

    for entry in session.timeline.iter().rev() {
        match entry {
            crate::session::SessionTimelineEntry::Approval {
                tool_name, reason, ..
            } => loops.push(format!(
                "Approval pending for {}: {}",
                tool_name,
                inline_clip(reason, 120)
            )),
            crate::session::SessionTimelineEntry::Batch {
                status: StoredBatchStatus::AwaitingApproval,
                ..
            } => loops.push("A tool batch is waiting for approval.".to_string()),
            crate::session::SessionTimelineEntry::Tool {
                name,
                detail,
                phase: StoredToolPhase::Running,
                ..
            } => loops.push(format!(
                "Tool still running or incomplete: {} ({})",
                name,
                inline_clip(detail, 96)
            )),
            _ => {}
        }
        if loops.len() >= MAX_OPEN_LOOPS {
            break;
        }
    }

    if loops.is_empty() {
        if let Some(last_assistant) = recent_assistant.last() {
            loops.push(format!(
                "Continue from the latest assistant direction: {}",
                inline_clip(last_assistant, 120)
            ));
        }
    }

    dedupe_preserve_order(loops)
}

fn recall_memory(data_dir: &Path, query: &str, objective: &str) -> Result<Vec<String>> {
    let store = MemoryStore::new(data_dir)?;
    let recall_query = if query.trim().is_empty() {
        objective
    } else {
        query
    };
    let mut entries = store.search(recall_query, MAX_MEMORY_HITS)?;
    entries.extend(store.search_target(recall_query, Some("user"), 1)?);
    if entries.is_empty() {
        entries.extend(store.list_target(Some("memory"))?.into_iter().take(2));
        entries.extend(store.list_target(Some("user"))?.into_iter().take(1));
    }
    Ok(dedupe_preserve_order(
        entries
            .into_iter()
            .map(|entry| format!("[{}] {}", entry.target, inline_clip(&entry.content, 140)))
            .collect(),
    ))
}

fn build_expand_hints(session: &StoredSession) -> Vec<String> {
    vec![
        format!(
            "memory_query(action=\"timeline\", session_id=\"{}\")",
            session.session_id
        ),
        format!(
            "memory_query(action=\"read\", kind=\"session\", name=\"{}\")",
            session.session_id
        ),
        format!(
            "archive_query(action=\"session_log\", session_id=\"{}\")",
            session.session_id
        ),
    ]
}

fn render_section(lines: &mut Vec<String>, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    lines.push(format!("{title}:"));
    for item in items {
        lines.push(format!("- {}", inline_clip(item, 180)));
    }
}

fn inline_clip(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ").trim().to_string();
    if normalized.is_empty() {
        "(none)".to_string()
    } else if normalized.chars().count() <= max_chars {
        normalized
    } else {
        normalized.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn dedupe_preserve_order(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for item in items {
        let key = item.to_lowercase();
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::{build_semantic_memory_digest, load_session_for_semantic_digest};
    use crate::memory::MemoryStore;
    use crate::session::{SessionStore, StoredSession};
    use crate::types::ChatMessage;

    #[test]
    fn builds_semantic_memory_digest_from_session_and_memory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let session_store = SessionStore::new(tmp.path()).expect("session store");
        let memory_store = MemoryStore::new(tmp.path()).expect("memory store");
        memory_store
            .add_to("memory", "Project prefers compact structured summaries.")
            .expect("memory");

        let mut session = StoredSession::new("session-a".to_string(), "gpt-test".to_string());
        session.title = Some("Prompt Compression".to_string());
        session
            .history
            .push(ChatMessage::user("不要太多启发式，优先做稳定的压缩。"));
        session
            .history
            .push(ChatMessage::assistant("先把 goal_state 和 skills 收短。"));
        session
            .history
            .push(ChatMessage::user("可以做一个记忆压缩工具来测试。"));
        session_store.save(&session).expect("save");

        let digest = build_semantic_memory_digest(tmp.path(), &session, "memory compression")
            .expect("digest");

        assert!(digest.objective.contains("记忆压缩工具"));
        assert!(
            digest
                .constraints
                .iter()
                .any(|item| item.contains("启发式"))
        );
        assert!(
            digest
                .recalled_memory
                .iter()
                .any(|item| item.contains("structured summaries"))
        );
        assert!(
            digest
                .expand_hints
                .iter()
                .any(|item| item.contains("memory_query"))
        );
    }

    #[test]
    fn loads_latest_session_when_session_id_is_not_provided() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let session_store = SessionStore::new(tmp.path()).expect("session store");
        let mut session = StoredSession::new("session-a".to_string(), "gpt-test".to_string());
        session.history.push(ChatMessage::user("hello"));
        session_store.save(&session).expect("save");

        let loaded = load_session_for_semantic_digest(tmp.path(), None)
            .expect("load")
            .expect("session");
        assert_eq!(loaded.session_id, "session-a");
    }
}
