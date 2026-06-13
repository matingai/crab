use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

use crate::session::{
    SessionStore, SessionTimelineEntry, StoredBatchStatus, StoredSession, StoredToolPhase,
};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};
use crate::wiki_store::WikiStore;

pub struct MemoryQueryTool;

#[derive(Debug, Deserialize)]
struct MemoryQueryArgs {
    action: String,
    kind: Option<String>,
    name: Option<String>,
    query: Option<String>,
    session_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug)]
struct TopicHit {
    slug: String,
    title: String,
    summary: String,
    updated_at_unix: Option<u64>,
    session_count: Option<usize>,
    score: usize,
}

#[async_trait]
impl Tool for MemoryQueryTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "memory_query",
            "Search, read, and expand compact memory across sessions, topics, and timelines.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["search", "read", "timeline", "related"],
                        "description": "Memory query action."
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["all", "session", "topic", "user"],
                        "description": "Target kind for action=read/related, or optional filter for action=search."
                    },
                    "name": {
                        "type": "string",
                        "description": "Target page or entity name. For action=read, use the page name without .md."
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query when action=search."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Specific session to inspect when action=timeline."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 50,
                        "description": "Maximum number of results or entries to return."
                    }
                }),
                &["action"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: MemoryQueryArgs =
            serde_json::from_value(args).context("invalid memory_query arguments")?;
        let limit = args.limit.unwrap_or(5).clamp(1, 50);

        match args.action.as_str() {
            "search" => {
                let Some(query) = args.query.as_deref() else {
                    bail!("memory_query action=search requires `query`");
                };
                search_memory(&ctx.data_dir, query, args.kind.as_deref(), limit)
            }
            "read" => {
                let Some(kind) = args.kind.as_deref() else {
                    bail!("memory_query action=read requires `kind`");
                };
                let Some(name) = args.name.as_deref() else {
                    bail!("memory_query action=read requires `name`");
                };
                read_memory_page(&ctx.data_dir, kind, name)
            }
            "timeline" => render_timeline(
                &ctx.data_dir,
                args.session_id.as_deref().or_else(|| args.name.as_deref()),
                limit,
            ),
            "related" => {
                let Some(kind) = args.kind.as_deref() else {
                    bail!("memory_query action=related requires `kind`");
                };
                let Some(name) = args.name.as_deref() else {
                    bail!("memory_query action=related requires `name`");
                };
                render_related(&ctx.data_dir, kind, name, limit)
            }
            other => bail!("unsupported memory_query action `{other}`"),
        }
    }
}

fn search_memory(data_dir: &Path, query: &str, kind: Option<&str>, limit: usize) -> Result<String> {
    let query = query.trim();
    if query.is_empty() {
        bail!("memory_query action=search requires a non-empty `query`");
    }

    let mut sections = vec![format!("memory search query: `{query}`")];
    let filter = kind.unwrap_or("all");

    if matches!(filter, "all" | "topic") {
        let topic_hits = search_topics(data_dir, query, limit)?;
        sections.push(render_topic_hits(&topic_hits));
    }

    if matches!(filter, "all" | "session") {
        let store = SessionStore::new(data_dir)?;
        let session_hits = store.search(query, limit)?;
        sections.push(render_session_hits(&session_hits));
    }

    if sections.len() == 1 {
        sections.push("no memory results".to_string());
    }

    Ok(sections.join("\n\n"))
}

fn read_memory_page(data_dir: &Path, kind: &str, name: &str) -> Result<String> {
    let (section, path) = resolve_memory_page(data_dir, kind, name)?;
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;

    let mut lines = vec![
        format!("memory page kind: {kind}"),
        format!("wiki section: {section}"),
        format!("page: {name}"),
        format!("path: {}", path.display()),
        String::new(),
        content,
    ];

    match kind {
        "session" => {
            lines.push(String::new());
            lines.push("expand:".to_string());
            lines.push(format!(
                "- memory_query(action=\"timeline\", session_id=\"{name}\")"
            ));
            lines.push(format!(
                "- memory_query(action=\"related\", kind=\"session\", name=\"{name}\")"
            ));
            lines.push(format!(
                "- archive_query(action=\"session_log\", session_id=\"{name}\")"
            ));
        }
        "topic" => {
            lines.push(String::new());
            lines.push("expand:".to_string());
            lines.push(format!(
                "- memory_query(action=\"related\", kind=\"topic\", name=\"{name}\")"
            ));
        }
        _ => {}
    }

    Ok(lines.join("\n"))
}

fn render_timeline(data_dir: &Path, session_id: Option<&str>, limit: usize) -> Result<String> {
    match session_id {
        Some(session_id) => render_session_timeline(data_dir, session_id, limit),
        None => render_user_timeline(data_dir, limit),
    }
}

fn render_session_timeline(data_dir: &Path, session_id: &str, limit: usize) -> Result<String> {
    let store = SessionStore::new(data_dir)?;
    let Some(session) = store.load(session_id)? else {
        bail!("session `{session_id}` not found");
    };

    let title = session
        .title
        .clone()
        .unwrap_or_else(|| session.session_id.clone());
    let total_entries = session.timeline.len();
    let start = total_entries.saturating_sub(limit);
    let visible = &session.timeline[start..];

    let mut lines = vec![
        format!("session timeline: {}", session.session_id),
        format!("title: {title}"),
        format!("entries_shown: {} of {}", visible.len(), total_entries),
    ];

    if visible.is_empty() {
        lines.push("- no timeline entries recorded".to_string());
    } else {
        let mut current_turn = String::new();
        for entry in visible {
            let turn_id = timeline_turn_id(entry);
            if current_turn != turn_id {
                current_turn = turn_id.to_string();
                lines.push(format!("- {turn_id}"));
                lines.push(format!(
                    "  - expand: archive_query(action=\"read_turn\", session_id=\"{}\", turn_id=\"{}\")",
                    session.session_id, turn_id
                ));
            }
            lines.push(format!("  - {}", format_timeline_entry(entry)));
        }
    }

    lines.push("expand:".to_string());
    lines.push(format!(
        "- memory_query(action=\"read\", kind=\"session\", name=\"{}\")",
        session.session_id
    ));
    lines.push(format!(
        "- memory_query(action=\"related\", kind=\"session\", name=\"{}\")",
        session.session_id
    ));
    lines.push(format!(
        "- archive_query(action=\"session_log\", session_id=\"{}\")",
        session.session_id
    ));
    Ok(lines.join("\n"))
}

fn render_user_timeline(data_dir: &Path, limit: usize) -> Result<String> {
    let store = SessionStore::new(data_dir)?;
    let sessions = store.list()?;
    let mut lines = vec!["user timeline".to_string()];
    if sessions.is_empty() {
        lines.push("- no sessions recorded".to_string());
        return Ok(lines.join("\n"));
    }

    for session in sessions.into_iter().take(limit) {
        let title = session
            .title
            .clone()
            .unwrap_or_else(|| session.session_id.clone());
        lines.push(format!(
            "- {} | {} | {}",
            session.updated_at_unix, session.session_id, title
        ));
        if let Some(last_user) = last_message_text(&session, "user") {
            lines.push(format!("  - user: {}", inline_clip(&last_user, 140)));
        }
        if let Some(last_assistant) = last_message_text(&session, "assistant") {
            lines.push(format!(
                "  - assistant: {}",
                inline_clip(&last_assistant, 140)
            ));
        }
        lines.push(format!(
            "  - expand: memory_query(action=\"timeline\", session_id=\"{}\")",
            session.session_id
        ));
    }

    Ok(lines.join("\n"))
}

fn render_related(data_dir: &Path, kind: &str, name: &str, limit: usize) -> Result<String> {
    match kind {
        "session" => related_sessions_for_session(data_dir, name, limit),
        "topic" => related_sessions_for_topic(data_dir, name, limit),
        other => bail!("unsupported related kind `{other}`"),
    }
}

fn related_sessions_for_session(data_dir: &Path, session_id: &str, limit: usize) -> Result<String> {
    let store = SessionStore::new(data_dir)?;
    let Some(session) = store.load(session_id)? else {
        bail!("session `{session_id}` not found");
    };
    let topic_title = session_topic_title(&session);
    let topic_slug = slugify(&topic_title);
    let siblings = store
        .list()?
        .into_iter()
        .filter(|candidate| candidate.session_id != session.session_id)
        .filter(|candidate| slugify(&session_topic_title(candidate)) == topic_slug)
        .take(limit)
        .collect::<Vec<_>>();

    let mut lines = vec![
        format!("related memory for session `{session_id}`"),
        format!("topic: {} (`{}`)", topic_title, topic_slug),
        "expand:".to_string(),
        format!("- memory_query(action=\"read\", kind=\"topic\", name=\"{topic_slug}\")"),
    ];
    if siblings.is_empty() {
        lines.push("- no sibling sessions under the same topic".to_string());
    } else {
        lines.push("sibling_sessions:".to_string());
        for sibling in siblings {
            lines.push(format!(
                "- [{}] {} | updated_at_unix={}",
                sibling.session_id,
                sibling
                    .title
                    .clone()
                    .unwrap_or_else(|| sibling.session_id.clone()),
                sibling.updated_at_unix
            ));
            if let Some(last_user) = last_message_text(&sibling, "user") {
                lines.push(format!("  {}", inline_clip(&last_user, 160)));
            }
            lines.push(format!(
                "  expand: memory_query(action=\"timeline\", session_id=\"{}\")",
                sibling.session_id
            ));
        }
    }
    Ok(lines.join("\n"))
}

fn related_sessions_for_topic(data_dir: &Path, topic_slug: &str, limit: usize) -> Result<String> {
    validate_page_name(topic_slug)?;
    let store = SessionStore::new(data_dir)?;
    let sessions = store
        .list()?
        .into_iter()
        .filter(|session| slugify(&session_topic_title(session)) == topic_slug)
        .take(limit)
        .collect::<Vec<_>>();

    let topic_title =
        read_topic_title(data_dir, topic_slug)?.unwrap_or_else(|| topic_slug.to_string());
    let mut lines = vec![
        format!("related memory for topic `{topic_slug}`"),
        format!("title: {topic_title}"),
    ];

    if sessions.is_empty() {
        lines.push("- no sessions found for this topic".to_string());
    } else {
        for session in sessions {
            lines.push(format!(
                "- [{}] {} | updated_at_unix={}",
                session.session_id,
                session
                    .title
                    .clone()
                    .unwrap_or_else(|| session.session_id.clone()),
                session.updated_at_unix
            ));
            if let Some(last_user) = last_message_text(&session, "user") {
                lines.push(format!("  {}", inline_clip(&last_user, 160)));
            }
            lines.push(format!(
                "  expand: memory_query(action=\"read\", kind=\"session\", name=\"{}\")",
                session.session_id
            ));
        }
    }

    lines.push("expand:".to_string());
    lines.push(format!(
        "- memory_query(action=\"read\", kind=\"topic\", name=\"{topic_slug}\")"
    ));
    Ok(lines.join("\n"))
}

fn search_topics(data_dir: &Path, query: &str, limit: usize) -> Result<Vec<TopicHit>> {
    let topics_root = WikiStore::new(data_dir)?.root().join("topics");
    let normalized_query = query.to_lowercase();
    let terms = normalized_terms(&normalized_query);
    let mut hits = Vec::new();

    for entry in fs::read_dir(&topics_root)
        .with_context(|| format!("failed to read wiki topics {}", topics_root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if stem == "index" {
            continue;
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let title =
            extract_frontmatter_value(&content, "title").unwrap_or_else(|| stem.to_string());
        let summary = extract_topic_summary(&content);
        let score = score_text(&title, &normalized_query, &terms) * 3
            + score_text(&summary, &normalized_query, &terms) * 2
            + score_text(&content, &normalized_query, &terms);
        if score == 0 {
            continue;
        }

        hits.push(TopicHit {
            slug: stem.to_string(),
            title,
            summary,
            updated_at_unix: extract_frontmatter_value(&content, "updated_at_unix")
                .and_then(|value| value.parse::<u64>().ok()),
            session_count: extract_frontmatter_value(&content, "session_count")
                .and_then(|value| value.parse::<usize>().ok()),
            score,
        });
    }

    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.updated_at_unix.cmp(&left.updated_at_unix))
            .then_with(|| left.slug.cmp(&right.slug))
    });
    hits.truncate(limit);
    Ok(hits)
}

fn render_topic_hits(hits: &[TopicHit]) -> String {
    if hits.is_empty() {
        return "topics:\n- none".to_string();
    }

    let mut lines = vec!["topics:".to_string()];
    for hit in hits {
        let updated = hit
            .updated_at_unix
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let session_count = hit
            .session_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".to_string());
        lines.push(format!(
            "- [topic.{}] {} | score={} | sessions={} | updated_at_unix={}",
            hit.slug, hit.title, hit.score, session_count, updated
        ));
        lines.push(format!("  {}", inline_clip(&hit.summary, 200)));
        lines.push(format!(
            "  expand: memory_query(action=\"read\", kind=\"topic\", name=\"{}\")",
            hit.slug
        ));
    }
    lines.join("\n")
}

fn render_session_hits(hits: &[crate::session::SessionSearchHit]) -> String {
    if hits.is_empty() {
        return "sessions:\n- none".to_string();
    }

    let mut lines = vec!["sessions:".to_string()];
    for hit in hits {
        lines.push(format!(
            "- [session.{}] {} | score={} | matches={} | matched_messages={} | updated_at_unix={}",
            hit.session.session_id,
            hit.session
                .title
                .clone()
                .unwrap_or_else(|| hit.session.session_id.clone()),
            hit.score,
            hit.match_count,
            hit.matched_messages,
            hit.session.updated_at_unix
        ));
        lines.push(format!("  {}", inline_clip(&hit.snippet, 200)));
        lines.push(format!(
            "  expand: memory_query(action=\"timeline\", session_id=\"{}\")",
            hit.session.session_id
        ));
    }
    lines.join("\n")
}

fn resolve_memory_page(data_dir: &Path, kind: &str, name: &str) -> Result<(&'static str, PathBuf)> {
    validate_page_name(name)?;
    let root = WikiStore::new(data_dir)?.root().to_path_buf();
    let (section, path) = match kind {
        "session" => ("sessions", root.join("sessions").join(format!("{name}.md"))),
        "topic" => ("topics", root.join("topics").join(format!("{name}.md"))),
        "user" => ("user", root.join("user").join(format!("{name}.md"))),
        other => bail!("unsupported memory kind `{other}`"),
    };
    if !path.is_file() {
        bail!("memory page `{name}` not found for kind `{kind}`");
    }
    Ok((section, path))
}

fn validate_page_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("memory page name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        bail!("memory page name contains invalid path segments");
    }
    Ok(())
}

fn read_topic_title(data_dir: &Path, topic_slug: &str) -> Result<Option<String>> {
    let (_, path) = resolve_memory_page(data_dir, "topic", topic_slug)?;
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(extract_frontmatter_value(&content, "title"))
}

fn extract_frontmatter_value(content: &str, key: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        let (candidate_key, candidate_value) = line.split_once(':')?;
        if candidate_key.trim() == key {
            return Some(candidate_value.trim().to_string());
        }
    }
    None
}

fn extract_topic_summary(content: &str) -> String {
    let mut in_summary = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "# Summary" {
            in_summary = true;
            continue;
        }
        if in_summary {
            if trimmed.starts_with("# ") {
                break;
            }
            if trimmed.starts_with("- ") {
                return trimmed.trim_start_matches("- ").to_string();
            }
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    String::new()
}

fn normalized_terms(normalized_query: &str) -> Vec<String> {
    let terms = normalized_query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        vec![normalized_query.to_string()]
    } else {
        terms
    }
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

fn last_message_text(session: &StoredSession, role: &str) -> Option<String> {
    session
        .history
        .iter()
        .rev()
        .find(|message| message.role == role)
        .map(|message| message.content_text())
}

fn session_topic_title(session: &StoredSession) -> String {
    if let Some(title) = session
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return inline_clip(title, 80);
    }
    if let Some(last_user) = last_message_text(session, "user") {
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
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "untitled-topic".to_string()
    } else {
        trimmed
    }
}

fn inline_clip(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ").trim().to_string();
    if normalized.is_empty() {
        return "(none)".to_string();
    }
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        normalized.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn timeline_turn_id(entry: &SessionTimelineEntry) -> &str {
    match entry {
        SessionTimelineEntry::User { turn_id, .. }
        | SessionTimelineEntry::Assistant { turn_id, .. }
        | SessionTimelineEntry::Tool { turn_id, .. }
        | SessionTimelineEntry::Batch { turn_id, .. }
        | SessionTimelineEntry::Approval { turn_id, .. } => turn_id,
    }
}

fn format_timeline_entry(entry: &SessionTimelineEntry) -> String {
    match entry {
        SessionTimelineEntry::User { content, .. } => {
            format!("user: {}", inline_clip(content, 180))
        }
        SessionTimelineEntry::Assistant { content, .. } => {
            format!("assistant: {}", inline_clip(content, 180))
        }
        SessionTimelineEntry::Tool {
            name,
            detail,
            phase,
            execution_mode,
            ..
        } => {
            let phase = match phase {
                StoredToolPhase::Running => "running",
                StoredToolPhase::Done => "done",
                StoredToolPhase::Error => "error",
                StoredToolPhase::Approval => "approval",
            };
            let mode = execution_mode.as_deref().unwrap_or("unknown");
            format!(
                "tool: {} [{} / {}] {}",
                name,
                phase,
                mode,
                inline_clip(detail, 180)
            )
        }
        SessionTimelineEntry::Batch {
            batch_id,
            iteration,
            total_calls,
            completed_calls,
            status,
            ..
        } => {
            let status = match status {
                StoredBatchStatus::Running => "running",
                StoredBatchStatus::Completed => "completed",
                StoredBatchStatus::CompletedWithErrors => "completed_with_errors",
                StoredBatchStatus::AwaitingApproval => "awaiting_approval",
                StoredBatchStatus::Canceled => "canceled",
            };
            format!(
                "batch: {} [{}] iteration={} completed={}/{}",
                batch_id, status, iteration, completed_calls, total_calls
            )
        }
        SessionTimelineEntry::Approval {
            tool_name,
            reason,
            command,
            ..
        } => {
            format!(
                "approval: {} | reason={} | command={}",
                tool_name,
                inline_clip(reason, 120),
                inline_clip(command, 120)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryQueryTool;
    use crate::session::{SessionStore, StoredSession, StoredToolPhase};
    use crate::tools::{Tool, ToolContext};
    use crate::wiki_store::WikiStore;
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

    #[tokio::test]
    async fn searches_topics_and_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path()).expect("session store");
        let wiki = WikiStore::new(tmp.path()).expect("wiki");

        let mut session = StoredSession::new("session-a".to_string(), "gpt-test".to_string());
        session.title = Some("Dialogue UI Layout".to_string());
        session.history.push(crate::types::ChatMessage::user(
            "Keep user messages as bubbles",
        ));
        session.history.push(crate::types::ChatMessage {
            role: "assistant".to_string(),
            content: Some(serde_json::Value::String(
                "Assistant messages stay flat".to_string(),
            )),
            tool_calls: None,
            tool_call_id: None,
        });
        store.save(&session).expect("save session");
        wiki.write_topic_pages(&[session.clone()]).expect("topics");

        let tool = MemoryQueryTool;
        let output = tool
            .execute(
                json!({
                    "action": "search",
                    "query": "dialogue bubbles",
                    "limit": 5
                }),
                &tool_context(&tmp),
            )
            .await
            .expect("search should succeed");

        assert!(output.contains("topic.dialogue-ui-layout"));
        assert!(output.contains("session.session-a"));
        assert!(output.contains("memory_query(action=\"timeline\", session_id=\"session-a\")"));
    }

    #[tokio::test]
    async fn reads_timeline_and_related_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path()).expect("session store");
        let wiki = WikiStore::new(tmp.path()).expect("wiki");

        let mut first = StoredSession::new("session-a".to_string(), "gpt-test".to_string());
        first.title = Some("Dialogue UI Layout".to_string());
        first.history.push(crate::types::ChatMessage::user(
            "Keep user messages as bubbles",
        ));
        first.record_user_timeline_entry("Keep user messages as bubbles");
        first.record_tool_timeline_entry(
            "tool-1",
            "read_file",
            "src/ui/chat.tsx",
            StoredToolPhase::Done,
            Some("sequential"),
            None,
            None,
            None,
        );
        first.record_assistant_timeline_entry("Adjusted the assistant layout");

        let mut second = StoredSession::new("session-b".to_string(), "gpt-test".to_string());
        second.title = Some("Dialogue UI Layout".to_string());
        second.history.push(crate::types::ChatMessage::user(
            "Tool cards should default collapsed",
        ));

        store.save(&first).expect("save first");
        store.save(&second).expect("save second");
        wiki.write_topic_pages(&[first.clone(), second.clone()])
            .expect("topics");

        let tool = MemoryQueryTool;
        let timeline = tool
            .execute(
                json!({
                    "action": "timeline",
                    "session_id": "session-a",
                    "limit": 10
                }),
                &tool_context(&tmp),
            )
            .await
            .expect("timeline should succeed");
        assert!(timeline.contains("tool: read_file [done / sequential]"));
        assert!(timeline.contains("assistant: Adjusted the assistant layout"));

        let related = tool
            .execute(
                json!({
                    "action": "related",
                    "kind": "session",
                    "name": "session-a",
                    "limit": 5
                }),
                &tool_context(&tmp),
            )
            .await
            .expect("related should succeed");
        assert!(related.contains("topic: Dialogue UI Layout (`dialogue-ui-layout`)"));
        assert!(related.contains("[session-b]"));
    }
}
