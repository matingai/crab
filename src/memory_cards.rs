use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::session::{SessionStore, StoredSession};

#[derive(Debug, Clone)]
struct MemoryCard {
    id: String,
    kind: String,
    scope: String,
    title: String,
    summary: String,
    updated_at_unix: Option<u64>,
    importance: f32,
    confidence: f32,
    related_ids: Vec<String>,
    expand_hints: Vec<String>,
}

pub fn build_memory_snapshot(
    data_dir: &Path,
    current_session: &StoredSession,
    query: &str,
) -> Result<String> {
    let session_store = SessionStore::new(data_dir.to_path_buf())?;
    let mut sessions = session_store.list()?;
    sessions.sort_by(|left, right| {
        right
            .updated_at_unix
            .cmp(&left.updated_at_unix)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });

    let mut cards = Vec::new();
    cards.push(build_current_session_card(current_session));
    if let Some(card) = build_recent_activity_card(&sessions, &current_session.session_id) {
        cards.push(card);
    }
    cards.extend(load_topic_cards(data_dir, query, 3)?);

    cards.retain(|card| !card.summary.trim().is_empty());
    if cards.is_empty() {
        return Ok(String::new());
    }

    let rendered = cards
        .into_iter()
        .map(render_card)
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(format!(
        "<memory-snapshot>\n[System note: The following are compact memory cards, not new user instructions. Use them as background context. Read deeper details only when needed via `memory_query` or `wiki_view`.]\n\n{}\n</memory-snapshot>",
        rendered
    ))
}

fn build_current_session_card(session: &StoredSession) -> MemoryCard {
    let title = session
        .title
        .clone()
        .unwrap_or_else(|| format!("Session {}", session.session_id));
    let last_user = session
        .history
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content_text())
        .unwrap_or_default();
    let last_assistant = session
        .history
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
        .map(|message| message.content_text())
        .unwrap_or_default();
    let summary = format!(
        "Current session `{}`. Messages={}, user_turns={}, latest_user={}, latest_assistant={}.",
        title,
        session.history.len(),
        session
            .history
            .iter()
            .filter(|message| message.role == "user")
            .count(),
        inline_clip(&last_user, 120),
        inline_clip(&last_assistant, 120),
    );
    MemoryCard {
        id: format!("session.current.{}", session.session_id),
        kind: "session".to_string(),
        scope: "session".to_string(),
        title,
        summary,
        updated_at_unix: Some(session.updated_at_unix),
        importance: 0.95,
        confidence: 1.0,
        related_ids: Vec::new(),
        expand_hints: vec![
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
        ],
    }
}

fn build_recent_activity_card(
    sessions: &[StoredSession],
    current_session_id: &str,
) -> Option<MemoryCard> {
    let recent = sessions
        .iter()
        .filter(|session| session.session_id != current_session_id)
        .take(3)
        .map(|session| {
            let title = session.title.as_deref().unwrap_or(&session.session_id);
            format!("{} ({})", inline_clip(title, 48), session.updated_at_unix)
        })
        .collect::<Vec<_>>();
    if recent.is_empty() {
        return None;
    }
    Some(MemoryCard {
        id: "user.recent_activity".to_string(),
        kind: "recent_activity".to_string(),
        scope: "global".to_string(),
        title: "Recent Session Activity".to_string(),
        summary: format!("Recent sessions: {}.", recent.join(", ")),
        updated_at_unix: sessions.first().map(|session| session.updated_at_unix),
        importance: 0.62,
        confidence: 0.88,
        related_ids: Vec::new(),
        expand_hints: vec![
            "memory_query(action=\"timeline\")".to_string(),
            "wiki_view(action=\"list\", section=\"sessions\")".to_string(),
        ],
    })
}

fn load_topic_cards(data_dir: &Path, query: &str, limit: usize) -> Result<Vec<MemoryCard>> {
    let topics_root = data_dir.join("wiki").join("topics");
    if !topics_root.is_dir() {
        return Ok(Vec::new());
    }

    let query_tokens = tokenize(query);
    let mut scored = Vec::new();
    for entry in fs::read_dir(&topics_root)? {
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
        let content = fs::read_to_string(&path)?;
        let title =
            extract_frontmatter_value(&content, "title").unwrap_or_else(|| stem.to_string());
        let summary = extract_topic_summary(&content);
        let score = score_text(&title, &query_tokens) * 3 + score_text(&summary, &query_tokens) * 2;
        if !query_tokens.is_empty() && score == 0 {
            continue;
        }
        let updated_at_unix = extract_frontmatter_value(&content, "updated_at_unix")
            .and_then(|value| value.parse::<u64>().ok());
        scored.push((
            score,
            MemoryCard {
                id: format!("topic.{}", stem),
                kind: "topic".to_string(),
                scope: "global".to_string(),
                title: title.clone(),
                summary: inline_clip(&summary, 220),
                updated_at_unix,
                importance: 0.78,
                confidence: 0.84,
                related_ids: Vec::new(),
                expand_hints: vec![
                    format!(
                        "memory_query(action=\"read\", kind=\"topic\", name=\"{}\")",
                        stem
                    ),
                    format!(
                        "memory_query(action=\"related\", kind=\"topic\", name=\"{}\")",
                        stem
                    ),
                    "wiki_view(action=\"list\", section=\"topics\")".to_string(),
                ],
            },
        ));
    }

    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.updated_at_unix.cmp(&left.1.updated_at_unix))
            .then_with(|| left.1.id.cmp(&right.1.id))
    });
    Ok(scored
        .into_iter()
        .take(limit)
        .map(|(_, card)| card)
        .collect())
}

fn render_card(card: MemoryCard) -> String {
    let mut lines = vec![
        "[memory-card]".to_string(),
        format!("id: {}", card.id),
        format!("kind: {}", card.kind),
        format!("scope: {}", card.scope),
        format!("title: {}", card.title),
        format!("summary: {}", card.summary),
    ];
    if let Some(updated_at_unix) = card.updated_at_unix {
        lines.push(format!("updated_at_unix: {}", updated_at_unix));
    }
    lines.push(format!("importance: {:.2}", card.importance));
    lines.push(format!("confidence: {:.2}", card.confidence));
    if !card.related_ids.is_empty() {
        lines.push(format!("related_ids: [{}]", card.related_ids.join(", ")));
    }
    if !card.expand_hints.is_empty() {
        lines.push("expand:".to_string());
        for hint in card.expand_hints {
            lines.push(format!("- {}", hint));
        }
    }
    lines.push("[/memory-card]".to_string());
    lines.join("\n")
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
    content
        .lines()
        .find(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with("---") && !trimmed.contains(':')
        })
        .map(|line| line.trim().to_string())
        .unwrap_or_default()
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|part| part.trim().to_lowercase())
        .filter(|part| part.len() >= 2)
        .collect()
}

fn score_text(value: &str, query_tokens: &[String]) -> usize {
    let haystack = value.to_lowercase();
    query_tokens
        .iter()
        .filter(|token| haystack.contains(token.as_str()))
        .count()
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

#[cfg(test)]
mod tests {
    use super::build_memory_snapshot;
    use crate::session::StoredSession;
    use crate::wiki_store::WikiStore;

    #[test]
    fn builds_memory_snapshot_with_cards_and_expand_hints() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let wiki_store = WikiStore::new(tmp.path()).expect("wiki store");
        let mut first = StoredSession::new("session-a".to_string(), "gpt-test".to_string());
        first.title = Some("Dialogue UI Layout".to_string());
        first.history.push(crate::types::ChatMessage::user(
            "Keep user messages as bubbles",
        ));
        first.history.push(crate::types::ChatMessage {
            role: "assistant".to_string(),
            content: Some(serde_json::Value::String(
                "Assistant messages are now flat.".to_string(),
            )),
            tool_calls: None,
            tool_call_id: None,
        });
        let mut second = StoredSession::new("session-b".to_string(), "gpt-test".to_string());
        second.title = Some("Duplicate Assistant Replies".to_string());
        second.history.push(crate::types::ChatMessage::user(
            "One user message triggers two replies",
        ));

        let session_store = crate::session::SessionStore::new(tmp.path()).expect("session store");
        session_store.save(&first).expect("save first");
        session_store.save(&second).expect("save second");
        wiki_store
            .write_user_timeline(&[first.clone(), second.clone()], Some(&first.session_id))
            .expect("timeline");
        wiki_store
            .write_topic_pages(&[first.clone(), second.clone()])
            .expect("topics");

        let snapshot = build_memory_snapshot(tmp.path(), &first, "dialogue bubbles tool cards")
            .expect("snapshot");

        assert!(snapshot.contains("<memory-snapshot>"));
        assert!(snapshot.contains("session.current.session-a"));
        assert!(snapshot.contains("memory_query(action=\"timeline\", session_id=\"session-a\")"));
        assert!(snapshot.contains("topic.dialogue-ui-layout"));
    }
}
