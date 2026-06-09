use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::session::{SessionTimelineEntry, StoredSession, StoredToolPhase};
use crate::todo::TodoItem;

#[derive(Debug, Clone)]
pub struct WikiStore {
    root: PathBuf,
}

impl WikiStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        let root = data_dir.join("wiki");
        fs::create_dir_all(root.join("sessions"))
            .with_context(|| format!("failed to create wiki sessions dir {}", root.display()))?;
        fs::create_dir_all(root.join("topics"))
            .with_context(|| format!("failed to create wiki topics dir {}", root.display()))?;
        fs::create_dir_all(root.join("user"))
            .with_context(|| format!("failed to create wiki user dir {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn write_session_page(
        &self,
        session: &StoredSession,
        todos: &[TodoItem],
        workspace_root: &Path,
        provider: &str,
    ) -> Result<PathBuf> {
        let path = self
            .root
            .join("sessions")
            .join(format!("{}.md", session.session_id));
        let content = render_session_page(session, todos, workspace_root, provider);
        write_atomic(&path, &content)?;
        Ok(path)
    }

    pub fn write_user_timeline(
        &self,
        sessions: &[StoredSession],
        current_session_id: Option<&str>,
    ) -> Result<PathBuf> {
        let path = self.root.join("user").join("timeline.md");
        let content = render_user_timeline(sessions, current_session_id);
        write_atomic(&path, &content)?;
        Ok(path)
    }

    pub fn write_topic_pages(&self, sessions: &[StoredSession]) -> Result<Vec<PathBuf>> {
        let topic_root = self.root.join("topics");
        fs::create_dir_all(&topic_root)
            .with_context(|| format!("failed to create topic wiki dir {}", topic_root.display()))?;

        let mut grouped = BTreeMap::<String, TopicAggregate>::new();
        for session in sessions {
            let title = topic_title_for_session(session);
            let slug = slugify(&title);
            let aggregate = grouped
                .entry(slug.clone())
                .or_insert_with(|| TopicAggregate::new(slug.clone(), title.clone()));
            aggregate.push(session);
        }

        let mut paths = Vec::new();
        for aggregate in grouped.values() {
            let path = topic_root.join(format!("{}.md", aggregate.slug));
            let content = render_topic_page(aggregate);
            write_atomic(&path, &content)?;
            paths.push(path);
        }

        let index_path = topic_root.join("index.md");
        write_atomic(
            &index_path,
            &render_topics_index(grouped.values().collect()),
        )?;
        paths.push(index_path);
        Ok(paths)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn render_session_page(
    session: &StoredSession,
    todos: &[TodoItem],
    workspace_root: &Path,
    provider: &str,
) -> String {
    let stats = summarize_session(session);
    let title = session
        .title
        .clone()
        .unwrap_or_else(|| format!("Session {}", session.session_id));
    let mut sections = vec![format!(
        "---\nid: session.{}\ntype: session\nsession_id: {}\ntitle: {}\nmodel: {}\nprovider: {}\nworkspace_root: {}\ncreated_at_unix: {}\nupdated_at_unix: {}\n---",
        session.session_id,
        session.session_id,
        yaml_escape(&title),
        yaml_escape(&session.model),
        yaml_escape(provider),
        yaml_escape(&workspace_root.display().to_string()),
        session.created_at_unix,
        session.updated_at_unix,
    )];

    sections.push("# Summary".to_string());
    sections.push(format!("- Title: {}", title));
    sections.push(format!("- Messages: {}", session.history.len()));
    sections.push(format!("- User turns: {}", stats.user_turns));
    sections.push(format!("- Assistant turns: {}", stats.assistant_turns));
    sections.push(format!("- Tool messages: {}", stats.tool_messages));
    if let Some(last_user) = stats.last_user_message {
        sections.push(format!(
            "- Last user message: {}",
            inline_clip(&last_user, 180)
        ));
    }
    if let Some(last_assistant) = stats.last_assistant_message {
        sections.push(format!(
            "- Last assistant message: {}",
            inline_clip(&last_assistant, 180)
        ));
    }

    sections.push("\n# Timeline".to_string());
    if session.timeline.is_empty() {
        sections.push("- No timeline entries yet.".to_string());
    } else {
        for line in render_timeline(session) {
            sections.push(line);
        }
    }

    sections.push("\n# Active Todos".to_string());
    let active_todos = todos
        .iter()
        .filter(|item| matches!(item.status.as_str(), "pending" | "in_progress"))
        .collect::<Vec<_>>();
    if active_todos.is_empty() {
        sections.push("- No active todos.".to_string());
    } else {
        for item in active_todos {
            sections.push(format!("- {} [{}] {}", item.id, item.status, item.content));
        }
    }

    sections.push("\n# References".to_string());
    sections.push(format!("- Source session: `{}`", session.session_id));
    for turn_id in collect_turn_ids(session) {
        sections.push(format!("- Turn: `{}`", turn_id));
    }

    sections.join("\n")
}

fn render_user_timeline(sessions: &[StoredSession], current_session_id: Option<&str>) -> String {
    let mut sections = vec![
        "---\nid: user.timeline\ntype: user_timeline\ntitle: User Timeline\n---".to_string(),
        "# Timeline".to_string(),
    ];

    if sessions.is_empty() {
        sections.push("- No sessions yet.".to_string());
        return sections.join("\n");
    }

    for session in sessions {
        let title = session.title.as_deref().unwrap_or(&session.session_id);
        let current_marker = current_session_id
            .filter(|current| *current == session.session_id)
            .map(|_| " [current]")
            .unwrap_or("");
        sections.push(format!(
            "- {} · {} · {}{}",
            session.updated_at_unix, title, session.session_id, current_marker
        ));
        if let Some(last_user) = session
            .history
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content_text())
        {
            sections.push(format!("  - User: {}", inline_clip(&last_user, 140)));
        }
        if let Some(last_assistant) = session
            .history
            .iter()
            .rev()
            .find(|message| message.role == "assistant")
            .map(|message| message.content_text())
        {
            sections.push(format!(
                "  - Assistant: {}",
                inline_clip(&last_assistant, 140)
            ));
        }
    }

    sections.join("\n")
}

fn render_topic_page(aggregate: &TopicAggregate) -> String {
    let mut sections = vec![format!(
        "---\nid: topic.{}\ntype: topic\ntitle: {}\nsession_count: {}\nupdated_at_unix: {}\n---",
        aggregate.slug,
        yaml_escape(&aggregate.title),
        aggregate.sessions.len(),
        aggregate.updated_at_unix,
    )];

    sections.push("# Summary".to_string());
    sections.push(format!("- Topic: {}", aggregate.title));
    sections.push(format!("- Sessions: {}", aggregate.sessions.len()));
    if let Some(snippet) = aggregate
        .sessions
        .iter()
        .filter_map(|session| {
            session
                .history
                .iter()
                .rev()
                .find(|message| message.role == "user")
                .map(|message| message.content_text())
        })
        .next()
    {
        sections.push(format!(
            "- Latest user focus: {}",
            inline_clip(&snippet, 180)
        ));
    }

    sections.push("\n# Sessions".to_string());
    for session in &aggregate.sessions {
        sections.push(format!(
            "- {} · {} · {}",
            session.updated_at_unix,
            session.title.as_deref().unwrap_or(&session.session_id),
            session.session_id
        ));
        if let Some(last_user) = session
            .history
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content_text())
        {
            sections.push(format!("  - User: {}", inline_clip(&last_user, 160)));
        }
        if let Some(last_assistant) = session
            .history
            .iter()
            .rev()
            .find(|message| message.role == "assistant")
            .map(|message| message.content_text())
        {
            sections.push(format!(
                "  - Assistant: {}",
                inline_clip(&last_assistant, 160)
            ));
        }
    }

    sections.push("\n# References".to_string());
    for session in &aggregate.sessions {
        sections.push(format!("- session `{}`", session.session_id));
    }

    sections.join("\n")
}

fn render_topics_index(aggregates: Vec<&TopicAggregate>) -> String {
    let mut sections = vec![
        "---\nid: topics.index\ntype: topic_index\ntitle: Topics Index\n---".to_string(),
        "# Topics".to_string(),
    ];
    if aggregates.is_empty() {
        sections.push("- No topics yet.".to_string());
        return sections.join("\n");
    }
    for aggregate in aggregates {
        sections.push(format!(
            "- {} (`{}`) · {} sessions",
            aggregate.title,
            aggregate.slug,
            aggregate.sessions.len()
        ));
    }
    sections.join("\n")
}

fn render_timeline(session: &StoredSession) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_turn = String::new();
    for entry in &session.timeline {
        let turn_id = match entry {
            SessionTimelineEntry::User { turn_id, .. }
            | SessionTimelineEntry::Assistant { turn_id, .. }
            | SessionTimelineEntry::Tool { turn_id, .. }
            | SessionTimelineEntry::Batch { turn_id, .. }
            | SessionTimelineEntry::Approval { turn_id, .. } => turn_id,
        };
        if current_turn != *turn_id {
            current_turn = turn_id.clone();
            lines.push(format!("- {}:", turn_id));
        }
        match entry {
            SessionTimelineEntry::User { content, .. } => {
                lines.push(format!("  - User: {}", inline_clip(content, 180)));
            }
            SessionTimelineEntry::Assistant { content, .. } => {
                lines.push(format!("  - Assistant: {}", inline_clip(content, 180)));
            }
            SessionTimelineEntry::Tool {
                name,
                detail,
                phase,
                execution_mode,
                batch_id,
                batch_index,
                batch_total,
                ..
            } => {
                let phase = match phase {
                    StoredToolPhase::Running => "running",
                    StoredToolPhase::Done => "done",
                    StoredToolPhase::Approval => "approval",
                };
                let mut meta = vec![format!("phase={phase}")];
                if let Some(mode) = execution_mode {
                    meta.push(format!("mode={mode}"));
                }
                if let (Some(index), Some(total)) = (batch_index, batch_total) {
                    meta.push(format!("batch={index}/{total}"));
                } else if let Some(batch_id) = batch_id {
                    meta.push(format!("batch={batch_id}"));
                }
                lines.push(format!(
                    "  - Tool `{}` [{}]: {}",
                    name,
                    meta.join(", "),
                    inline_clip(detail, 180)
                ));
            }
            SessionTimelineEntry::Batch {
                batch_id,
                iteration,
                completed_calls,
                total_calls,
                status,
                ..
            } => {
                lines.push(format!(
                    "  - Batch `{}`: iteration {}, {}/{}, status={:?}",
                    batch_id, iteration, completed_calls, total_calls, status
                ));
            }
            SessionTimelineEntry::Approval {
                tool_name,
                reason,
                command,
                ..
            } => {
                lines.push(format!(
                    "  - Approval `{}`: {} | command `{}`",
                    tool_name,
                    inline_clip(reason, 120),
                    inline_clip(command, 120)
                ));
            }
        }
    }
    lines
}

fn collect_turn_ids(session: &StoredSession) -> Vec<String> {
    let mut turn_ids = Vec::new();
    for entry in &session.timeline {
        let turn_id = match entry {
            SessionTimelineEntry::User { turn_id, .. }
            | SessionTimelineEntry::Assistant { turn_id, .. }
            | SessionTimelineEntry::Tool { turn_id, .. }
            | SessionTimelineEntry::Batch { turn_id, .. }
            | SessionTimelineEntry::Approval { turn_id, .. } => turn_id,
        };
        if !turn_ids.iter().any(|item| item == turn_id) {
            turn_ids.push(turn_id.clone());
        }
    }
    turn_ids
}

struct SessionStats {
    user_turns: usize,
    assistant_turns: usize,
    tool_messages: usize,
    last_user_message: Option<String>,
    last_assistant_message: Option<String>,
}

#[derive(Debug)]
struct TopicAggregate {
    slug: String,
    title: String,
    updated_at_unix: u64,
    sessions: Vec<StoredSession>,
}

impl TopicAggregate {
    fn new(slug: String, title: String) -> Self {
        Self {
            slug,
            title,
            updated_at_unix: 0,
            sessions: Vec::new(),
        }
    }

    fn push(&mut self, session: &StoredSession) {
        self.updated_at_unix = self.updated_at_unix.max(session.updated_at_unix);
        self.sessions.push(session.clone());
        self.sessions.sort_by(|left, right| {
            right
                .updated_at_unix
                .cmp(&left.updated_at_unix)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
    }
}

fn summarize_session(session: &StoredSession) -> SessionStats {
    SessionStats {
        user_turns: session
            .history
            .iter()
            .filter(|message| message.role == "user")
            .count(),
        assistant_turns: session
            .history
            .iter()
            .filter(|message| message.role == "assistant")
            .count(),
        tool_messages: session
            .history
            .iter()
            .filter(|message| message.role == "tool")
            .count(),
        last_user_message: session
            .history
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content_text()),
        last_assistant_message: session
            .history
            .iter()
            .rev()
            .find(|message| message.role == "assistant")
            .map(|message| message.content_text()),
    }
}

fn inline_clip(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ").trim().to_string();
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        normalized.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn yaml_escape(value: &str) -> String {
    value.replace('\n', " ").replace(':', "\\:")
}

fn topic_title_for_session(session: &StoredSession) -> String {
    if let Some(title) = session
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return inline_clip(title, 80);
    }
    if let Some(last_user) = session
        .history
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content_text())
    {
        return inline_clip(&last_user, 80);
    }
    format!("Session {}", session.session_id)
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
            continue;
        }
        if slug.is_empty() || last_dash {
            continue;
        }
        slug.push('-');
        last_dash = true;
    }
    slug.trim_matches('-')
        .to_string()
        .if_empty("untitled-topic")
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create wiki parent {}", parent.display()))?;
    }
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, content).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to move {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WikiStore;
    use crate::session::{SessionTimelineEntry, StoredSession, StoredToolPhase};
    use crate::todo::TodoItem;

    #[test]
    fn writes_session_and_timeline_pages() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = WikiStore::new(tmp.path()).expect("wiki");
        let mut session = StoredSession::new("demo".to_string(), "gpt-test".to_string());
        session.title = Some("Demo Session".to_string());
        session
            .history
            .push(crate::types::ChatMessage::user("Inspect the UI layout"));
        session.history.push(crate::types::ChatMessage {
            role: "assistant".to_string(),
            content: Some(serde_json::Value::String(
                "I will inspect the UI layout.".to_string(),
            )),
            tool_calls: None,
            tool_call_id: None,
        });
        session.timeline.push(SessionTimelineEntry::User {
            id: "turn-1-user".to_string(),
            turn_id: "turn-1".to_string(),
            content: "Inspect the UI layout".to_string(),
        });
        session.timeline.push(SessionTimelineEntry::Tool {
            id: "tool-1".to_string(),
            turn_id: "turn-1".to_string(),
            name: "read_file".to_string(),
            detail: "desktop-shell/app/page.tsx".to_string(),
            phase: StoredToolPhase::Done,
            execution_mode: Some("sequential".to_string()),
            batch_id: None,
            batch_index: None,
            batch_total: None,
        });

        let todos = vec![TodoItem::new("1", "Tighten spacing", "in_progress")];
        let session_path = store
            .write_session_page(&session, &todos, tmp.path(), "openai")
            .expect("session page");
        let timeline_path = store
            .write_user_timeline(&[session.clone()], Some(&session.session_id))
            .expect("timeline page");

        let session_md = std::fs::read_to_string(session_path).expect("read session page");
        let timeline_md = std::fs::read_to_string(timeline_path).expect("read timeline page");

        assert!(session_md.contains("# Summary"));
        assert!(session_md.contains("Tool `read_file`"));
        assert!(session_md.contains("Active Todos"));
        assert!(timeline_md.contains("# Timeline"));
        assert!(timeline_md.contains("[current]"));
    }

    #[test]
    fn writes_topic_pages() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = WikiStore::new(tmp.path()).expect("wiki");
        let mut first = StoredSession::new("a".to_string(), "gpt-test".to_string());
        first.title = Some("Dialogue UI Layout".to_string());
        first.history.push(crate::types::ChatMessage::user(
            "Keep user messages as bubbles",
        ));
        let mut second = StoredSession::new("b".to_string(), "gpt-test".to_string());
        second.title = Some("Dialogue UI Layout".to_string());
        second
            .history
            .push(crate::types::ChatMessage::user("Make tool cards smaller"));

        let paths = store.write_topic_pages(&[first, second]).expect("topics");
        assert!(paths.iter().any(|path| path.ends_with("index.md")));
        let topic_md = std::fs::read_to_string(store.root().join("topics/dialogue-ui-layout.md"))
            .expect("read topic");
        assert!(topic_md.contains("# Sessions"));
        assert!(topic_md.contains("- Sessions: 2"));
    }
}
