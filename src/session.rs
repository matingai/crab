use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::privacy::redact_secrets;
use crate::types::ChatMessage;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StoredToolPhase {
    Running,
    Done,
    Approval,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StoredBatchStatus {
    Running,
    Completed,
    AwaitingApproval,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionTimelineEntry {
    User {
        id: String,
        turn_id: String,
        content: String,
    },
    Assistant {
        id: String,
        turn_id: String,
        content: String,
    },
    Tool {
        id: String,
        turn_id: String,
        name: String,
        detail: String,
        phase: StoredToolPhase,
        execution_mode: Option<String>,
        batch_id: Option<String>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    },
    Batch {
        id: String,
        turn_id: String,
        batch_id: String,
        iteration: usize,
        total_calls: usize,
        completed_calls: usize,
        status: StoredBatchStatus,
    },
    Approval {
        id: String,
        turn_id: String,
        approval_id: String,
        tool_name: String,
        reason: String,
        command: String,
        execution_mode: Option<String>,
        batch_id: Option<String>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    },
}

impl SessionTimelineEntry {
    pub fn id(&self) -> &str {
        match self {
            Self::User { id, .. }
            | Self::Assistant { id, .. }
            | Self::Tool { id, .. }
            | Self::Batch { id, .. }
            | Self::Approval { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub session_id: String,
    pub model: String,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub latest_response_id: Option<String>,
    #[serde(default)]
    pub latest_response_runtime_key: Option<String>,
    #[serde(default)]
    pub latest_response_prefix_digest: Option<String>,
    pub history: Vec<ChatMessage>,
    #[serde(default)]
    pub timeline: Vec<SessionTimelineEntry>,
}

impl StoredSession {
    pub fn new(session_id: String, model: String) -> Self {
        let now = unix_now();
        Self {
            session_id,
            model,
            created_at_unix: now,
            updated_at_unix: now,
            title: None,
            latest_response_id: None,
            latest_response_runtime_key: None,
            latest_response_prefix_digest: None,
            history: Vec::new(),
            timeline: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at_unix = unix_now();
    }

    pub fn clear_timeline(&mut self) {
        self.timeline.clear();
    }

    pub fn set_latest_response_state(
        &mut self,
        response_id: Option<String>,
        runtime_key: Option<String>,
        prefix_digest: Option<String>,
    ) {
        self.latest_response_id = response_id;
        self.latest_response_runtime_key = runtime_key;
        self.latest_response_prefix_digest = prefix_digest;
    }

    pub fn current_turn_id(&self) -> String {
        let turn_index = self
            .history
            .iter()
            .filter(|message| message.role == "user")
            .count()
            .max(1);
        format!("turn-{turn_index}")
    }

    pub fn record_user_timeline_entry(&mut self, content: impl Into<String>) -> String {
        let turn_id = self.current_turn_id();
        self.upsert_timeline_entry(SessionTimelineEntry::User {
            id: format!("{turn_id}-user"),
            turn_id: turn_id.clone(),
            content: redact_secrets(content.into()),
        });
        turn_id
    }

    pub fn record_assistant_timeline_entry(&mut self, content: impl Into<String>) {
        let turn_id = self.current_turn_id();
        self.upsert_timeline_entry(SessionTimelineEntry::Assistant {
            id: format!("{turn_id}-assistant"),
            turn_id,
            content: redact_secrets(content.into()),
        });
    }

    pub fn record_tool_timeline_entry(
        &mut self,
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        detail: impl Into<String>,
        phase: StoredToolPhase,
        execution_mode: Option<&str>,
        batch_id: Option<&str>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    ) {
        let turn_id = self.current_turn_id();
        self.upsert_timeline_entry(SessionTimelineEntry::Tool {
            id: tool_call_id.into(),
            turn_id,
            name: name.into(),
            detail: redact_secrets(detail.into()),
            phase,
            execution_mode: execution_mode.map(str::to_string),
            batch_id: batch_id.map(str::to_string),
            batch_index,
            batch_total,
        });
    }

    pub fn record_batch_timeline_entry(
        &mut self,
        batch_id: impl Into<String>,
        iteration: usize,
        total_calls: usize,
        completed_calls: usize,
        status: StoredBatchStatus,
    ) {
        let batch_id = batch_id.into();
        let turn_id = self.current_turn_id();
        self.upsert_timeline_entry(SessionTimelineEntry::Batch {
            id: format!("batch-{batch_id}"),
            turn_id,
            batch_id,
            iteration,
            total_calls,
            completed_calls,
            status,
        });
    }

    pub fn record_approval_timeline_entry(
        &mut self,
        approval_id: impl Into<String>,
        tool_name: impl Into<String>,
        reason: impl Into<String>,
        command: impl Into<String>,
        execution_mode: Option<&str>,
        batch_id: Option<&str>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    ) {
        let approval_id = approval_id.into();
        let turn_id = self.current_turn_id();
        self.upsert_timeline_entry(SessionTimelineEntry::Approval {
            id: format!("approval-{approval_id}"),
            turn_id,
            approval_id,
            tool_name: tool_name.into(),
            reason: redact_secrets(reason.into()),
            command: redact_secrets(command.into()),
            execution_mode: execution_mode.map(str::to_string),
            batch_id: batch_id.map(str::to_string),
            batch_index,
            batch_total,
        });
    }

    fn upsert_timeline_entry(&mut self, entry: SessionTimelineEntry) {
        let entry_id = entry.id().to_string();
        if let Some(index) = self.timeline.iter().position(|item| item.id() == entry_id) {
            self.timeline[index] = entry;
        } else {
            self.timeline.push(entry);
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionSearchHit {
    pub session: StoredSession,
    pub score: usize,
    pub snippet: String,
    pub match_count: usize,
    pub matched_messages: usize,
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("sessions"))
            .with_context(|| format!("failed to create session dir under {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn session_path(&self, session_id: &str) -> PathBuf {
        self.root
            .join("sessions")
            .join(format!("{session_id}.json"))
    }

    pub fn load(&self, session_id: &str) -> Result<Option<StoredSession>> {
        let path = self.session_path(session_id);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read session {}", path.display()))?;
        let session = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse session {}", path.display()))?;
        Ok(Some(session))
    }

    pub fn list(&self) -> Result<Vec<StoredSession>> {
        let sessions_dir = self.root.join("sessions");
        let mut sessions = Vec::new();

        for entry in fs::read_dir(&sessions_dir)
            .with_context(|| format!("failed to read {}", sessions_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }

            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read session {}", path.display()))?;
            let session: StoredSession = serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse session {}", path.display()))?;
            sessions.push(session);
        }

        sessions.sort_by(|a, b| {
            b.updated_at_unix
                .cmp(&a.updated_at_unix)
                .then_with(|| b.created_at_unix.cmp(&a.created_at_unix))
                .then_with(|| a.session_id.cmp(&b.session_id))
        });
        Ok(sessions)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SessionSearchHit>> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let normalized_query = query.to_lowercase();
        let terms = normalized_query
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        let terms = if terms.is_empty() {
            vec![normalized_query.clone()]
        } else {
            terms
        };

        let mut hits = self
            .list()?
            .into_iter()
            .filter_map(|session| score_session(session, &normalized_query, &terms))
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.match_count.cmp(&left.match_count))
                .then_with(|| {
                    right
                        .session
                        .updated_at_unix
                        .cmp(&left.session.updated_at_unix)
                })
                .then_with(|| {
                    right
                        .session
                        .created_at_unix
                        .cmp(&left.session.created_at_unix)
                })
                .then_with(|| left.session.session_id.cmp(&right.session.session_id))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn save(&self, session: &StoredSession) -> Result<PathBuf> {
        let path = self.session_path(&session.session_id);
        let tmp = path.with_extension("json.tmp");
        let raw =
            serde_json::to_string_pretty(session).context("failed to serialize session state")?;
        fs::write(&tmp, raw).with_context(|| format!("failed to write {}", tmp.display()))?;
        fs::rename(&tmp, &path)
            .with_context(|| format!("failed to move {} to {}", tmp.display(), path.display()))?;
        Ok(path)
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn score_session(
    session: StoredSession,
    normalized_query: &str,
    terms: &[String],
) -> Option<SessionSearchHit> {
    let mut score = 0usize;
    let mut match_count = 0usize;
    let mut matched_messages = 0usize;
    let mut best_snippet = String::new();
    let mut best_snippet_score = 0usize;

    let metadata_fields = [
        ("session", session.session_id.as_str(), 7usize),
        ("title", session.title.as_deref().unwrap_or(""), 9usize),
        ("model", session.model.as_str(), 3usize),
    ];

    for (label, text, weight) in metadata_fields {
        let field_score = score_text(text, normalized_query, terms);
        if field_score == 0 {
            continue;
        }
        score += field_score * weight;
        match_count += field_score;
        let snippet = format!("{label}: {}", truncate_text(text, 180));
        if field_score * weight > best_snippet_score {
            best_snippet_score = field_score * weight;
            best_snippet = snippet;
        }
    }

    for message in &session.history {
        let text = message.content_text();
        if text.trim().is_empty() {
            continue;
        }

        let field_score = score_text(&text, normalized_query, terms);
        if field_score == 0 {
            continue;
        }

        matched_messages += 1;
        match_count += field_score;
        let role_weight = match message.role.as_str() {
            "assistant" => 5usize,
            "user" => 4usize,
            "tool" => 2usize,
            "system" => 1usize,
            _ => 1usize,
        };
        let weighted = field_score * role_weight;
        score += weighted;

        if weighted > best_snippet_score {
            best_snippet_score = weighted;
            best_snippet = format!(
                "{}: {}",
                message.role,
                excerpt_for_query(&text, normalized_query, terms, 180)
            );
        }
    }

    if score == 0 {
        return None;
    }

    Some(SessionSearchHit {
        session,
        score,
        snippet: best_snippet,
        match_count,
        matched_messages,
    })
}

fn score_text(text: &str, normalized_query: &str, terms: &[String]) -> usize {
    let lowered = text.to_lowercase();
    let mut score = 0usize;

    if lowered.contains(normalized_query) {
        score += 3;
    }

    for term in terms {
        score += lowered.match_indices(term).count();
    }

    score
}

fn excerpt_for_query(
    text: &str,
    normalized_query: &str,
    terms: &[String],
    max_chars: usize,
) -> String {
    let lowered = text.to_lowercase();
    let mut match_index = lowered.find(normalized_query);
    if match_index.is_none() {
        match_index = terms.iter().filter_map(|term| lowered.find(term)).min();
    }

    let Some(index) = match_index else {
        return truncate_text(text, max_chars);
    };

    let start = index.saturating_sub(max_chars / 3);
    let end = usize::min(
        text.len(),
        index + normalized_query.len() + (max_chars * 2 / 3),
    );
    let mut excerpt = text
        .get(start..end)
        .map(str::trim)
        .unwrap_or(text.trim())
        .to_string();

    if start > 0 {
        excerpt = format!("...{excerpt}");
    }
    if end < text.len() {
        excerpt.push_str("...");
    }
    truncate_text(&excerpt, max_chars)
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SessionStore, SessionTimelineEntry, StoredBatchStatus, StoredSession, StoredToolPhase,
    };

    #[test]
    fn saves_and_loads_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path()).expect("store");
        let mut session = StoredSession::new("demo".to_string(), "model".to_string());
        session
            .history
            .push(crate::types::ChatMessage::user("hello"));
        session.record_user_timeline_entry("hello");
        session.record_tool_timeline_entry(
            "tool-1",
            "read_file",
            "README.md",
            StoredToolPhase::Done,
            Some("sequential"),
            None,
            None,
            None,
        );
        session.record_batch_timeline_entry("parallel-1-read", 1, 2, 1, StoredBatchStatus::Running);
        store.save(&session).expect("save");

        let loaded = store.load("demo").expect("load").expect("session");
        assert_eq!(loaded.session_id, "demo");
        assert_eq!(loaded.history.len(), 1);
        assert_eq!(loaded.timeline, session.timeline);
    }

    #[test]
    fn timeline_entries_redact_secret_like_values() {
        let mut session = StoredSession::new("demo".to_string(), "model".to_string());

        session.record_tool_timeline_entry(
            "tool-1",
            "read_file",
            "OPENAI_API_KEY=sk-test0123456789abcdef",
            StoredToolPhase::Done,
            Some("sequential"),
            None,
            None,
            None,
        );
        session.record_approval_timeline_entry(
            "approval-1",
            "terminal",
            "needs TOKEN=abcdef1234567890abcdef",
            "terminal command with TOKEN=abcdef1234567890abcdef",
            Some("sequential"),
            None,
            None,
            None,
        );

        let raw = serde_json::to_string(&session.timeline).expect("serialize timeline");
        assert!(raw.contains("[REDACTED]"));
        assert!(!raw.contains("sk-test0123456789abcdef"));
        assert!(!raw.contains("abcdef1234567890abcdef"));
        assert!(matches!(
            &session.timeline[0],
            SessionTimelineEntry::Tool { detail, .. } if detail.contains("OPENAI_API_KEY=[REDACTED]")
        ));
    }

    #[test]
    fn lists_sessions_by_updated_desc() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path()).expect("store");

        let mut first = StoredSession::new("first".to_string(), "model-a".to_string());
        first.updated_at_unix = 10;
        store.save(&first).expect("save first");

        let mut second = StoredSession::new("second".to_string(), "model-b".to_string());
        second.updated_at_unix = 20;
        store.save(&second).expect("save second");

        let sessions = store.list().expect("list");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "second");
        assert_eq!(sessions[1].session_id, "first");
    }

    #[test]
    fn loads_legacy_session_without_title() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path()).expect("store");
        std::fs::write(
            store.session_path("legacy"),
            r#"{
  "session_id": "legacy",
  "model": "gpt-4.1-mini",
  "created_at_unix": 1,
  "updated_at_unix": 2,
  "history": []
}"#,
        )
        .expect("write legacy session");

        let loaded = store.load("legacy").expect("load").expect("session");
        assert_eq!(loaded.title, None);
        assert!(loaded.timeline.is_empty());
    }

    #[test]
    fn searches_sessions_by_message_and_metadata() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path()).expect("store");

        let mut first = StoredSession::new("debug-rust".to_string(), "gpt-5".to_string());
        first.title = Some("Rust borrow checker notes".to_string());
        first.history.push(crate::types::ChatMessage::user(
            "Investigate borrow checker error in parser",
        ));
        store.save(&first).expect("save first");

        let mut second = StoredSession::new("frontend".to_string(), "gpt-5".to_string());
        second.history.push(crate::types::ChatMessage::user(
            "Polish landing page typography",
        ));
        store.save(&second).expect("save second");

        let hits = store.search("borrow checker", 10).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session.session_id, "debug-rust");
        assert!(hits[0].score > 0);
        assert!(hits[0].snippet.contains("borrow"));
        assert_eq!(hits[0].matched_messages, 1);
    }

    #[test]
    fn search_returns_empty_for_blank_query() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path()).expect("store");
        let hits = store.search("   ", 5).expect("search");
        assert!(hits.is_empty());
    }
}
