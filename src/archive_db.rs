use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::session::{SessionTimelineEntry, StoredSession, StoredToolPhase};
use crate::types::ToolCall;

#[derive(Debug, Clone)]
pub struct ArchiveTurnSummary {
    pub session_id: String,
    pub turn_id: String,
    pub turn_index: usize,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
    pub message_count: usize,
    pub tool_call_count: usize,
    pub approval_count: usize,
    pub event_count: usize,
    pub last_message_role: Option<String>,
    pub last_message_snippet: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArchiveTurnRecord {
    pub session_id: String,
    pub turn_id: String,
    pub turn_index: usize,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ArchiveMessageRecord {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub raw_json: Option<String>,
    pub created_at_unix_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ArchiveToolCallRecord {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub tool_name: String,
    pub execution_mode: String,
    pub batch_id: Option<String>,
    pub batch_index: Option<usize>,
    pub batch_total: Option<usize>,
    pub phase: String,
    pub arguments_raw: Option<String>,
    pub output_raw: Option<String>,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ArchiveApprovalRecord {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub reason: String,
    pub command: String,
    pub execution_mode: String,
    pub batch_id: Option<String>,
    pub batch_index: Option<usize>,
    pub batch_total: Option<usize>,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ArchiveEventRecord {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub event_type: String,
    pub title: String,
    pub summary: String,
    pub created_at_unix_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ArchiveTurnBundle {
    pub turn: ArchiveTurnRecord,
    pub messages: Vec<ArchiveMessageRecord>,
    pub tool_calls: Vec<ArchiveToolCallRecord>,
    pub approvals: Vec<ArchiveApprovalRecord>,
    pub events: Vec<ArchiveEventRecord>,
}

#[derive(Debug, Clone)]
pub struct ArchiveMessageSearchHit {
    pub message: ArchiveMessageRecord,
    pub snippet: String,
}

#[derive(Debug, Clone)]
struct BackfillToolCallState {
    id: String,
    turn_id: String,
    turn_index: usize,
    tool_name: String,
    execution_mode: String,
    batch_id: Option<String>,
    batch_index: Option<usize>,
    batch_total: Option<usize>,
    phase: String,
    arguments_raw: Option<String>,
    output_raw: Option<String>,
    created_at_unix_ms: i64,
    updated_at_unix_ms: i64,
}

#[derive(Debug, Clone)]
struct BackfillEventState {
    turn_id: Option<String>,
    event_type: String,
    title: String,
    summary: String,
    created_at_unix_ms: i64,
}

pub struct ArchiveStore {
    path: PathBuf,
}

impl ArchiveStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        fs::create_dir_all(data_dir)
            .with_context(|| format!("failed to create archive dir {}", data_dir.display()))?;
        let store = Self {
            path: data_dir.join("archive.db"),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn upsert_session(
        &self,
        session_id: &str,
        title: Option<&str>,
        workspace_root: &Path,
        provider: &str,
        model: &str,
        created_at_unix: u64,
        updated_at_unix: u64,
    ) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            r#"
            INSERT INTO sessions (
                id, title, workspace_root, provider, model, created_at_unix, updated_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                workspace_root = excluded.workspace_root,
                provider = excluded.provider,
                model = excluded.model,
                updated_at_unix = excluded.updated_at_unix
            "#,
            params![
                session_id,
                title,
                workspace_root.display().to_string(),
                provider,
                model,
                created_at_unix as i64,
                updated_at_unix as i64,
            ],
        )
        .context("failed to upsert archive session")?;
        Ok(())
    }

    pub fn upsert_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        turn_index: usize,
        created_at_unix_ms: u128,
    ) -> Result<()> {
        self.upsert_turn_at(
            session_id,
            turn_id,
            turn_index,
            created_at_unix_ms as i64,
            created_at_unix_ms as i64,
        )
    }

    pub fn append_message(
        &self,
        session_id: &str,
        turn_id: &str,
        role: &str,
        content: &str,
        tool_call_id: Option<&str>,
        raw_json: Option<&str>,
    ) -> Result<String> {
        self.append_message_at(
            session_id,
            turn_id,
            role,
            content,
            tool_call_id,
            raw_json,
            unix_now_ms() as i64,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_tool_call(
        &self,
        tool_call_id: &str,
        session_id: &str,
        turn_id: &str,
        tool_name: &str,
        execution_mode: &str,
        batch_id: Option<&str>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
        phase: &str,
        arguments_raw: Option<&str>,
        output_raw: Option<&str>,
    ) -> Result<()> {
        let now = unix_now_ms() as i64;
        self.upsert_tool_call_at(
            tool_call_id,
            session_id,
            turn_id,
            tool_name,
            execution_mode,
            batch_id,
            batch_index,
            batch_total,
            phase,
            arguments_raw,
            output_raw,
            now,
            now,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn upsert_tool_call_at(
        &self,
        tool_call_id: &str,
        session_id: &str,
        turn_id: &str,
        tool_name: &str,
        execution_mode: &str,
        batch_id: Option<&str>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
        phase: &str,
        arguments_raw: Option<&str>,
        output_raw: Option<&str>,
        created_at_unix_ms: i64,
        updated_at_unix_ms: i64,
    ) -> Result<()> {
        let conn = self.connection()?;
        let existing_created_at = conn
            .query_row(
                "SELECT created_at_unix_ms FROM tool_calls WHERE id = ?1",
                params![tool_call_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .context("failed to load existing tool call archive timestamp")?;
        conn.execute(
            r#"
            INSERT INTO tool_calls (
                id, session_id, turn_id, tool_name, execution_mode, batch_id, batch_index,
                batch_total, phase, arguments_raw, output_raw, created_at_unix_ms, updated_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(id) DO UPDATE SET
                turn_id = excluded.turn_id,
                tool_name = excluded.tool_name,
                execution_mode = excluded.execution_mode,
                batch_id = excluded.batch_id,
                batch_index = excluded.batch_index,
                batch_total = excluded.batch_total,
                phase = excluded.phase,
                arguments_raw = COALESCE(excluded.arguments_raw, tool_calls.arguments_raw),
                output_raw = COALESCE(excluded.output_raw, tool_calls.output_raw),
                updated_at_unix_ms = excluded.updated_at_unix_ms
            "#,
            params![
                tool_call_id,
                session_id,
                turn_id,
                tool_name,
                execution_mode,
                batch_id,
                batch_index.map(|value| value as i64),
                batch_total.map(|value| value as i64),
                phase,
                arguments_raw,
                output_raw,
                existing_created_at.unwrap_or(created_at_unix_ms),
                updated_at_unix_ms,
            ],
        )
        .context("failed to upsert archive tool call")?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_approval(
        &self,
        approval_id: &str,
        session_id: &str,
        turn_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        reason: &str,
        command: &str,
        execution_mode: &str,
        batch_id: Option<&str>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    ) -> Result<()> {
        let now = unix_now_ms() as i64;
        self.upsert_approval_at(
            approval_id,
            session_id,
            turn_id,
            tool_call_id,
            tool_name,
            reason,
            command,
            execution_mode,
            batch_id,
            batch_index,
            batch_total,
            now,
            now,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn upsert_approval_at(
        &self,
        approval_id: &str,
        session_id: &str,
        turn_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        reason: &str,
        command: &str,
        execution_mode: &str,
        batch_id: Option<&str>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
        created_at_unix_ms: i64,
        updated_at_unix_ms: i64,
    ) -> Result<()> {
        let conn = self.connection()?;
        let existing_created_at = conn
            .query_row(
                "SELECT created_at_unix_ms FROM approvals WHERE id = ?1",
                params![approval_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .context("failed to load existing approval archive timestamp")?;
        conn.execute(
            r#"
            INSERT INTO approvals (
                id, session_id, turn_id, tool_call_id, tool_name, reason, command, execution_mode,
                batch_id, batch_index, batch_total, created_at_unix_ms, updated_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(id) DO UPDATE SET
                turn_id = excluded.turn_id,
                reason = excluded.reason,
                command = excluded.command,
                updated_at_unix_ms = excluded.updated_at_unix_ms
            "#,
            params![
                approval_id,
                session_id,
                turn_id,
                tool_call_id,
                tool_name,
                reason,
                command,
                execution_mode,
                batch_id,
                batch_index.map(|value| value as i64),
                batch_total.map(|value| value as i64),
                existing_created_at.unwrap_or(created_at_unix_ms),
                updated_at_unix_ms,
            ],
        )
        .context("failed to upsert archive approval")?;
        Ok(())
    }

    pub fn append_event(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        event_type: &str,
        title: &str,
        summary: &str,
    ) -> Result<String> {
        self.append_event_at(
            session_id,
            turn_id,
            event_type,
            title,
            summary,
            unix_now_ms() as i64,
        )
    }

    fn append_event_at(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        event_type: &str,
        title: &str,
        summary: &str,
        created_at_unix_ms: i64,
    ) -> Result<String> {
        let conn = self.connection()?;
        let event_id = format!("evt_{}", Uuid::new_v4());
        conn.execute(
            r#"
            INSERT INTO events (
                id, session_id, turn_id, event_type, title, summary, created_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                event_id,
                session_id,
                turn_id,
                event_type,
                title,
                summary,
                created_at_unix_ms,
            ],
        )
        .context("failed to append archive event")?;
        Ok(event_id)
    }

    fn upsert_turn_at(
        &self,
        session_id: &str,
        turn_id: &str,
        turn_index: usize,
        created_at_unix_ms: i64,
        updated_at_unix_ms: i64,
    ) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            r#"
            INSERT INTO turns (id, session_id, turn_index, created_at_unix_ms, updated_at_unix_ms)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(session_id, id) DO UPDATE SET
                updated_at_unix_ms = excluded.updated_at_unix_ms
            "#,
            params![
                turn_id,
                session_id,
                turn_index as i64,
                created_at_unix_ms,
                updated_at_unix_ms,
            ],
        )
        .context("failed to upsert archive turn")?;
        Ok(())
    }

    fn append_message_at(
        &self,
        session_id: &str,
        turn_id: &str,
        role: &str,
        content: &str,
        tool_call_id: Option<&str>,
        raw_json: Option<&str>,
        created_at_unix_ms: i64,
    ) -> Result<String> {
        let conn = self.connection()?;
        let message_id = format!("msg_{}", Uuid::new_v4());
        conn.execute(
            r#"
            INSERT INTO messages (
                id, session_id, turn_id, role, content, tool_call_id, raw_json, created_at_unix_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                message_id,
                session_id,
                turn_id,
                role,
                content,
                tool_call_id,
                raw_json,
                created_at_unix_ms,
            ],
        )
        .context("failed to append archive message")?;
        Ok(message_id)
    }

    pub fn session_has_content(&self, session_id: &str) -> Result<bool> {
        let conn = self.connection()?;
        let mut total = 0i64;
        for sql in [
            "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
            "SELECT COUNT(*) FROM tool_calls WHERE session_id = ?1",
            "SELECT COUNT(*) FROM approvals WHERE session_id = ?1",
            "SELECT COUNT(*) FROM events WHERE session_id = ?1",
        ] {
            total += conn
                .query_row(sql, params![session_id], |row| row.get::<_, i64>(0))
                .with_context(|| {
                    format!("failed to inspect archive content for session `{session_id}`")
                })?;
        }
        Ok(total > 0)
    }

    pub fn backfill_sessions(
        &self,
        sessions: &[StoredSession],
        workspace_root: &Path,
        provider: &str,
    ) -> Result<usize> {
        let mut imported = 0usize;
        for session in sessions {
            self.upsert_session(
                &session.session_id,
                session.title.as_deref(),
                workspace_root,
                provider,
                &session.model,
                session.created_at_unix,
                session.updated_at_unix,
            )?;
            if self.session_has_content(&session.session_id)? {
                continue;
            }
            self.backfill_session(session)?;
            imported += 1;
        }
        Ok(imported)
    }

    fn backfill_session(&self, session: &StoredSession) -> Result<()> {
        if session.history.is_empty() && session.timeline.is_empty() {
            return Ok(());
        }

        let mut clock = BackfillClock::new(session.created_at_unix);
        let mut turn_updates = BTreeMap::<String, (usize, i64, i64)>::new();
        let mut current_turn_id = String::new();
        let mut current_turn_index = 0usize;
        let mut tool_states = BTreeMap::<String, BackfillToolCallState>::new();
        let mut tool_ids_by_turn = BTreeMap::<String, Vec<String>>::new();
        let mut synthetic_tool_counter = 0usize;
        let mut events = Vec::<BackfillEventState>::new();

        for message in &session.history {
            let message_turn_id = if message.role == "user" {
                current_turn_index += 1;
                current_turn_id = format!("turn-{current_turn_index}");
                current_turn_id.clone()
            } else if current_turn_index == 0 {
                current_turn_index = 1;
                current_turn_id = "turn-1".to_string();
                current_turn_id.clone()
            } else {
                current_turn_id.clone()
            };

            let turn_index = parse_turn_index(&message_turn_id);
            let turn_ts = clock.next();
            record_turn_seen(&mut turn_updates, &message_turn_id, turn_index, turn_ts);

            let content = message.content_text();
            let raw_json = serde_json::to_string(message).ok();
            self.append_message_at(
                &session.session_id,
                &message_turn_id,
                &message.role,
                &content,
                message.tool_call_id.as_deref(),
                raw_json.as_deref(),
                clock.next(),
            )?;

            if message.role == "user" {
                events.push(BackfillEventState {
                    turn_id: Some(message_turn_id.clone()),
                    event_type: "turn_started".to_string(),
                    title: "User turn started".to_string(),
                    summary: truncate_middle(&content, 320),
                    created_at_unix_ms: clock.next(),
                });
            } else if message.role == "assistant"
                && message
                    .tool_calls
                    .as_ref()
                    .is_none_or(|calls| calls.is_empty())
            {
                events.push(BackfillEventState {
                    turn_id: Some(message_turn_id.clone()),
                    event_type: "assistant_message".to_string(),
                    title: "Assistant replied".to_string(),
                    summary: truncate_middle(&content, 320),
                    created_at_unix_ms: clock.next(),
                });
            }

            if let Some(tool_calls) = &message.tool_calls {
                apply_assistant_tool_calls(
                    &mut tool_states,
                    &mut tool_ids_by_turn,
                    tool_calls,
                    &message_turn_id,
                    turn_index,
                    &mut clock,
                );
            }

            if let Some(tool_call_id) = &message.tool_call_id {
                let entry = tool_states.entry(tool_call_id.clone()).or_insert_with(|| {
                    BackfillToolCallState {
                        id: tool_call_id.clone(),
                        turn_id: message_turn_id.clone(),
                        turn_index,
                        tool_name: "unknown".to_string(),
                        execution_mode: "unknown".to_string(),
                        batch_id: None,
                        batch_index: None,
                        batch_total: None,
                        phase: "done".to_string(),
                        arguments_raw: None,
                        output_raw: None,
                        created_at_unix_ms: clock.peek(),
                        updated_at_unix_ms: clock.peek(),
                    }
                });
                if !tool_ids_by_turn
                    .entry(message_turn_id.clone())
                    .or_default()
                    .contains(tool_call_id)
                {
                    tool_ids_by_turn
                        .entry(message_turn_id.clone())
                        .or_default()
                        .push(tool_call_id.clone());
                }
                if entry.output_raw.is_none() && !content.trim().is_empty() {
                    entry.output_raw = Some(content.clone());
                }
                entry.phase = "done".to_string();
                entry.updated_at_unix_ms = clock.next();
            }
        }

        for entry in &session.timeline {
            match entry {
                SessionTimelineEntry::Tool {
                    id,
                    turn_id,
                    name,
                    detail,
                    phase,
                    execution_mode,
                    batch_id,
                    batch_index,
                    batch_total,
                } => {
                    let turn_index = parse_turn_index(turn_id);
                    record_turn_seen(&mut turn_updates, turn_id, turn_index, clock.peek());
                    let state =
                        tool_states
                            .entry(id.clone())
                            .or_insert_with(|| BackfillToolCallState {
                                id: id.clone(),
                                turn_id: turn_id.clone(),
                                turn_index,
                                tool_name: name.clone(),
                                execution_mode: execution_mode
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                batch_id: batch_id.clone(),
                                batch_index: *batch_index,
                                batch_total: *batch_total,
                                phase: phase_to_str(phase).to_string(),
                                arguments_raw: None,
                                output_raw: None,
                                created_at_unix_ms: clock.next(),
                                updated_at_unix_ms: clock.peek(),
                            });
                    state.turn_id = turn_id.clone();
                    state.turn_index = turn_index;
                    state.tool_name = name.clone();
                    state.execution_mode = execution_mode
                        .clone()
                        .unwrap_or_else(|| state.execution_mode.clone());
                    state.batch_id = batch_id.clone().or_else(|| state.batch_id.clone());
                    state.batch_index = batch_index.or(state.batch_index);
                    state.batch_total = batch_total.or(state.batch_total);
                    state.phase = phase_to_str(phase).to_string();
                    match phase {
                        StoredToolPhase::Running => {
                            if state.arguments_raw.is_none() {
                                state.arguments_raw = Some(detail.clone());
                            }
                        }
                        StoredToolPhase::Approval
                        | StoredToolPhase::Done
                        | StoredToolPhase::Error => {
                            state.output_raw = Some(detail.clone());
                        }
                    }
                    state.updated_at_unix_ms = clock.next();
                    if !tool_ids_by_turn
                        .entry(turn_id.clone())
                        .or_default()
                        .contains(id)
                    {
                        tool_ids_by_turn
                            .entry(turn_id.clone())
                            .or_default()
                            .push(id.clone());
                    }
                }
                SessionTimelineEntry::Approval {
                    approval_id,
                    turn_id,
                    tool_name,
                    reason,
                    command,
                    execution_mode,
                    batch_id,
                    batch_index,
                    batch_total,
                    ..
                } => {
                    let turn_index = parse_turn_index(turn_id);
                    record_turn_seen(&mut turn_updates, turn_id, turn_index, clock.peek());
                    let tool_call_id = find_or_create_approval_tool_call(
                        &mut tool_states,
                        &mut tool_ids_by_turn,
                        turn_id,
                        turn_index,
                        tool_name,
                        reason,
                        execution_mode.as_deref().unwrap_or("unknown"),
                        batch_id.as_deref(),
                        *batch_index,
                        *batch_total,
                        approval_id,
                        &mut synthetic_tool_counter,
                        &mut clock,
                    );
                    self.upsert_approval_at(
                        approval_id,
                        &session.session_id,
                        turn_id,
                        &tool_call_id,
                        tool_name,
                        reason,
                        command,
                        execution_mode.as_deref().unwrap_or("unknown"),
                        batch_id.as_deref(),
                        *batch_index,
                        *batch_total,
                        clock.next(),
                        clock.next(),
                    )?;
                    events.push(BackfillEventState {
                        turn_id: Some(turn_id.clone()),
                        event_type: "approval_required".to_string(),
                        title: "Tool approval required".to_string(),
                        summary: truncate_middle(reason, 240),
                        created_at_unix_ms: clock.next(),
                    });
                }
                _ => {}
            }
        }

        for (turn_id, (turn_index, created_at_unix_ms, updated_at_unix_ms)) in &turn_updates {
            self.upsert_turn_at(
                &session.session_id,
                turn_id,
                *turn_index,
                *created_at_unix_ms,
                *updated_at_unix_ms,
            )?;
        }

        for state in tool_states.values() {
            self.upsert_turn_at(
                &session.session_id,
                &state.turn_id,
                state.turn_index,
                state.created_at_unix_ms,
                state.updated_at_unix_ms,
            )?;
            self.upsert_tool_call_at(
                &state.id,
                &session.session_id,
                &state.turn_id,
                &state.tool_name,
                &state.execution_mode,
                state.batch_id.as_deref(),
                state.batch_index,
                state.batch_total,
                &state.phase,
                state.arguments_raw.as_deref(),
                state.output_raw.as_deref(),
                state.created_at_unix_ms,
                state.updated_at_unix_ms,
            )?;
        }

        events.push(BackfillEventState {
            turn_id: None,
            event_type: "archive_backfilled".to_string(),
            title: "Legacy session archived".to_string(),
            summary: format!(
                "Imported session `{}` from stored session.json",
                session.session_id
            ),
            created_at_unix_ms: clock.next(),
        });
        for event in events {
            self.append_event_at(
                &session.session_id,
                event.turn_id.as_deref(),
                &event.event_type,
                &event.title,
                &event.summary,
                event.created_at_unix_ms,
            )?;
        }

        Ok(())
    }

    pub fn list_turn_summaries(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<ArchiveTurnSummary>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT
                t.session_id,
                t.id,
                t.turn_index,
                t.created_at_unix_ms,
                t.updated_at_unix_ms,
                (SELECT COUNT(*) FROM messages m WHERE m.session_id = t.session_id AND m.turn_id = t.id),
                (SELECT COUNT(*) FROM tool_calls c WHERE c.session_id = t.session_id AND c.turn_id = t.id),
                (SELECT COUNT(*) FROM approvals a WHERE a.session_id = t.session_id AND a.turn_id = t.id),
                (SELECT COUNT(*) FROM events e WHERE e.session_id = t.session_id AND e.turn_id = t.id),
                (SELECT m.role FROM messages m WHERE m.session_id = t.session_id AND m.turn_id = t.id ORDER BY m.created_at_unix_ms DESC, m.id DESC LIMIT 1),
                (SELECT m.content FROM messages m WHERE m.session_id = t.session_id AND m.turn_id = t.id ORDER BY m.created_at_unix_ms DESC, m.id DESC LIMIT 1)
            FROM turns t
            WHERE t.session_id = ?1
            ORDER BY t.turn_index DESC, t.updated_at_unix_ms DESC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(ArchiveTurnSummary {
                session_id: row.get(0)?,
                turn_id: row.get(1)?,
                turn_index: to_usize(row.get::<_, i64>(2)?)?,
                created_at_unix_ms: row.get(3)?,
                updated_at_unix_ms: row.get(4)?,
                message_count: to_usize(row.get::<_, i64>(5)?)?,
                tool_call_count: to_usize(row.get::<_, i64>(6)?)?,
                approval_count: to_usize(row.get::<_, i64>(7)?)?,
                event_count: to_usize(row.get::<_, i64>(8)?)?,
                last_message_role: row.get(9)?,
                last_message_snippet: row
                    .get::<_, Option<String>>(10)?
                    .map(|value| truncate_middle(&value, 220)),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to list archive turn summaries")
    }

    pub fn read_turn(&self, session_id: &str, turn_id: &str) -> Result<Option<ArchiveTurnBundle>> {
        let conn = self.connection()?;
        let turn = conn
            .query_row(
                r#"
                SELECT session_id, id, turn_index, created_at_unix_ms, updated_at_unix_ms
                FROM turns
                WHERE session_id = ?1 AND id = ?2
                "#,
                params![session_id, turn_id],
                map_turn_record,
            )
            .optional()
            .context("failed to read archive turn")?;
        let Some(turn) = turn else {
            return Ok(None);
        };

        let messages = read_messages_for_turn(&conn, session_id, turn_id)?;
        let tool_calls = read_tool_calls_for_turn(&conn, session_id, turn_id)?;
        let approvals = read_approvals_for_turn(&conn, session_id, turn_id)?;
        let events = read_events_for_turn(&conn, session_id, turn_id)?;

        Ok(Some(ArchiveTurnBundle {
            turn,
            messages,
            tool_calls,
            approvals,
            events,
        }))
    }

    pub fn read_tool_call(&self, tool_call_id: &str) -> Result<Option<ArchiveToolCallRecord>> {
        let conn = self.connection()?;
        conn.query_row(
            r#"
            SELECT
                id, session_id, turn_id, tool_name, execution_mode, batch_id, batch_index,
                batch_total, phase, arguments_raw, output_raw, created_at_unix_ms, updated_at_unix_ms
            FROM tool_calls
            WHERE id = ?1
            "#,
            params![tool_call_id],
            map_tool_call_record,
        )
        .optional()
        .context("failed to read archive tool call")
    }

    pub fn read_approvals_for_tool_call(
        &self,
        tool_call_id: &str,
    ) -> Result<Vec<ArchiveApprovalRecord>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT
                id, session_id, turn_id, tool_call_id, tool_name, reason, command, execution_mode,
                batch_id, batch_index, batch_total, created_at_unix_ms, updated_at_unix_ms
            FROM approvals
            WHERE tool_call_id = ?1
            ORDER BY created_at_unix_ms ASC, id ASC
            "#,
        )?;
        let rows = stmt.query_map(params![tool_call_id], map_approval_record)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read approvals for tool call")
    }

    pub fn read_messages_for_tool_call(
        &self,
        tool_call_id: &str,
    ) -> Result<Vec<ArchiveMessageRecord>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT
                id, session_id, turn_id, role, content, tool_call_id, raw_json, created_at_unix_ms
            FROM messages
            WHERE tool_call_id = ?1
            ORDER BY created_at_unix_ms ASC, id ASC
            "#,
        )?;
        let rows = stmt.query_map(params![tool_call_id], map_message_record)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read messages for tool call")
    }

    pub fn search_messages(
        &self,
        query: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ArchiveMessageSearchHit>> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let conn = self.connection()?;
        let pattern = format!("%{}%", query.to_lowercase());
        let sql = if session_id.is_some() {
            r#"
            SELECT id, session_id, turn_id, role, content, tool_call_id, raw_json, created_at_unix_ms
            FROM messages
            WHERE session_id = ?1 AND lower(content) LIKE ?2
            ORDER BY created_at_unix_ms DESC, id DESC
            LIMIT ?3
            "#
        } else {
            r#"
            SELECT id, session_id, turn_id, role, content, tool_call_id, raw_json, created_at_unix_ms
            FROM messages
            WHERE lower(content) LIKE ?1
            ORDER BY created_at_unix_ms DESC, id DESC
            LIMIT ?2
            "#
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = if let Some(session_id) = session_id {
            stmt.query_map(params![session_id, pattern, limit as i64], |row| {
                let message = map_message_record(row)?;
                Ok(ArchiveMessageSearchHit {
                    snippet: excerpt_for_query(&message.content, query, 220),
                    message,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
        } else {
            stmt.query_map(params![pattern, limit as i64], |row| {
                let message = map_message_record(row)?;
                Ok(ArchiveMessageSearchHit {
                    snippet: excerpt_for_query(&message.content, query, 220),
                    message,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
        };
        rows.context("failed to search archive messages")
    }

    fn initialize(&self) -> Result<()> {
        let conn = self.connection()?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            "#,
        )
        .context("failed to initialize archive pragmas")?;
        migrate_legacy_turns_schema(&conn)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT,
                workspace_root TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                created_at_unix INTEGER NOT NULL,
                updated_at_unix INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS turns (
                id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                turn_index INTEGER NOT NULL,
                created_at_unix_ms INTEGER NOT NULL,
                updated_at_unix_ms INTEGER NOT NULL,
                PRIMARY KEY (session_id, id)
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                tool_call_id TEXT,
                raw_json TEXT,
                created_at_unix_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tool_calls (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                execution_mode TEXT NOT NULL,
                batch_id TEXT,
                batch_index INTEGER,
                batch_total INTEGER,
                phase TEXT NOT NULL,
                arguments_raw TEXT,
                output_raw TEXT,
                created_at_unix_ms INTEGER NOT NULL,
                updated_at_unix_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS approvals (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                tool_call_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                reason TEXT NOT NULL,
                command TEXT NOT NULL,
                execution_mode TEXT NOT NULL,
                batch_id TEXT,
                batch_index INTEGER,
                batch_total INTEGER,
                created_at_unix_ms INTEGER NOT NULL,
                updated_at_unix_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                turn_id TEXT,
                event_type TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                created_at_unix_ms INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_turns_session_id ON turns(session_id, turn_index);
            CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id, created_at_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_tool_calls_session_id ON tool_calls(session_id, updated_at_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_approvals_session_id ON approvals(session_id, updated_at_unix_ms);
            CREATE INDEX IF NOT EXISTS idx_events_session_id ON events(session_id, created_at_unix_ms);
            "#,
        )
        .context("failed to initialize archive schema")?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection> {
        Connection::open(&self.path)
            .with_context(|| format!("failed to open archive db {}", self.path.display()))
    }
}

#[derive(Debug, Clone)]
struct BackfillClock {
    next_unix_ms: i64,
}

impl BackfillClock {
    fn new(created_at_unix: u64) -> Self {
        Self {
            next_unix_ms: (created_at_unix as i64).saturating_mul(1000).max(1),
        }
    }

    fn next(&mut self) -> i64 {
        let current = self.next_unix_ms;
        self.next_unix_ms = self.next_unix_ms.saturating_add(1);
        current
    }

    fn peek(&self) -> i64 {
        self.next_unix_ms
    }
}

fn apply_assistant_tool_calls(
    tool_states: &mut BTreeMap<String, BackfillToolCallState>,
    tool_ids_by_turn: &mut BTreeMap<String, Vec<String>>,
    tool_calls: &[ToolCall],
    turn_id: &str,
    turn_index: usize,
    clock: &mut BackfillClock,
) {
    for call in tool_calls {
        let state = tool_states
            .entry(call.id.clone())
            .or_insert_with(|| BackfillToolCallState {
                id: call.id.clone(),
                turn_id: turn_id.to_string(),
                turn_index,
                tool_name: call.function.name.clone(),
                execution_mode: "sequential".to_string(),
                batch_id: None,
                batch_index: None,
                batch_total: None,
                phase: "running".to_string(),
                arguments_raw: Some(call.function.arguments.clone()),
                output_raw: None,
                created_at_unix_ms: clock.next(),
                updated_at_unix_ms: clock.peek(),
            });
        state.turn_id = turn_id.to_string();
        state.turn_index = turn_index;
        state.tool_name = call.function.name.clone();
        state.arguments_raw = state
            .arguments_raw
            .clone()
            .or_else(|| Some(call.function.arguments.clone()));
        state.updated_at_unix_ms = clock.next();
        let ids = tool_ids_by_turn.entry(turn_id.to_string()).or_default();
        if !ids.contains(&call.id) {
            ids.push(call.id.clone());
        }
    }
}

fn record_turn_seen(
    turn_updates: &mut BTreeMap<String, (usize, i64, i64)>,
    turn_id: &str,
    turn_index: usize,
    seen_at_unix_ms: i64,
) {
    turn_updates
        .entry(turn_id.to_string())
        .and_modify(|existing| {
            existing.0 = turn_index;
            existing.2 = seen_at_unix_ms;
        })
        .or_insert((turn_index, seen_at_unix_ms, seen_at_unix_ms));
}

fn parse_turn_index(turn_id: &str) -> usize {
    turn_id
        .strip_prefix("turn-")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
}

fn phase_to_str(phase: &StoredToolPhase) -> &'static str {
    match phase {
        StoredToolPhase::Running => "running",
        StoredToolPhase::Done => "done",
        StoredToolPhase::Error => "error",
        StoredToolPhase::Approval => "approval",
    }
}

#[allow(clippy::too_many_arguments)]
fn find_or_create_approval_tool_call(
    tool_states: &mut BTreeMap<String, BackfillToolCallState>,
    tool_ids_by_turn: &mut BTreeMap<String, Vec<String>>,
    turn_id: &str,
    turn_index: usize,
    tool_name: &str,
    reason: &str,
    execution_mode: &str,
    batch_id: Option<&str>,
    batch_index: Option<usize>,
    batch_total: Option<usize>,
    approval_id: &str,
    synthetic_tool_counter: &mut usize,
    clock: &mut BackfillClock,
) -> String {
    if let Some(existing_id) = tool_ids_by_turn
        .get(turn_id)
        .and_then(|ids| {
            ids.iter().rev().find(|tool_call_id| {
                tool_states
                    .get(*tool_call_id)
                    .is_some_and(|state| state.tool_name == tool_name)
            })
        })
        .cloned()
    {
        if let Some(state) = tool_states.get_mut(&existing_id) {
            state.phase = "approval".to_string();
            state.output_raw = state
                .output_raw
                .clone()
                .or_else(|| Some(reason.to_string()));
            state.execution_mode = execution_mode.to_string();
            state.batch_id = batch_id
                .map(str::to_string)
                .or_else(|| state.batch_id.clone());
            state.batch_index = batch_index.or(state.batch_index);
            state.batch_total = batch_total.or(state.batch_total);
            state.updated_at_unix_ms = clock.next();
        }
        return existing_id;
    }

    *synthetic_tool_counter += 1;
    let synthetic_id = format!("synthetic-tool-{approval_id}-{}", synthetic_tool_counter);
    tool_states.insert(
        synthetic_id.clone(),
        BackfillToolCallState {
            id: synthetic_id.clone(),
            turn_id: turn_id.to_string(),
            turn_index,
            tool_name: tool_name.to_string(),
            execution_mode: execution_mode.to_string(),
            batch_id: batch_id.map(str::to_string),
            batch_index,
            batch_total,
            phase: "approval".to_string(),
            arguments_raw: None,
            output_raw: Some(reason.to_string()),
            created_at_unix_ms: clock.next(),
            updated_at_unix_ms: clock.peek(),
        },
    );
    tool_ids_by_turn
        .entry(turn_id.to_string())
        .or_default()
        .push(synthetic_id.clone());
    synthetic_id
}

fn migrate_legacy_turns_schema(conn: &Connection) -> Result<()> {
    if !turns_table_uses_legacy_primary_key(conn)? {
        return Ok(());
    }

    conn.execute_batch(
        r#"
        ALTER TABLE turns RENAME TO turns_legacy;
        CREATE TABLE turns (
            id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            turn_index INTEGER NOT NULL,
            created_at_unix_ms INTEGER NOT NULL,
            updated_at_unix_ms INTEGER NOT NULL,
            PRIMARY KEY (session_id, id)
        );
        INSERT OR REPLACE INTO turns (id, session_id, turn_index, created_at_unix_ms, updated_at_unix_ms)
        SELECT id, session_id, turn_index, created_at_unix_ms, updated_at_unix_ms
        FROM turns_legacy;
        DROP TABLE turns_legacy;
        "#,
    )
    .context("failed to migrate legacy archive turns schema")
}

fn turns_table_uses_legacy_primary_key(conn: &Connection) -> Result<bool> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'turns' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .optional()
        .context("failed to inspect turns table")?
        .is_some();
    if !exists {
        return Ok(false);
    }

    let mut stmt = conn
        .prepare("PRAGMA table_info(turns)")
        .context("failed to inspect turns schema")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
    })?;

    let mut id_pk = 0;
    let mut session_id_pk = 0;
    for row in rows {
        let (name, pk) = row.context("failed to read turns schema row")?;
        match name.as_str() {
            "id" => id_pk = pk,
            "session_id" => session_id_pk = pk,
            _ => {}
        }
    }

    Ok(id_pk == 1 && session_id_pk == 0)
}

fn map_turn_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArchiveTurnRecord> {
    Ok(ArchiveTurnRecord {
        session_id: row.get(0)?,
        turn_id: row.get(1)?,
        turn_index: to_usize(row.get::<_, i64>(2)?)?,
        created_at_unix_ms: row.get(3)?,
        updated_at_unix_ms: row.get(4)?,
    })
}

fn map_message_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArchiveMessageRecord> {
    Ok(ArchiveMessageRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        turn_id: row.get(2)?,
        role: row.get(3)?,
        content: row.get(4)?,
        tool_call_id: row.get(5)?,
        raw_json: row.get(6)?,
        created_at_unix_ms: row.get(7)?,
    })
}

fn map_tool_call_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArchiveToolCallRecord> {
    Ok(ArchiveToolCallRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        turn_id: row.get(2)?,
        tool_name: row.get(3)?,
        execution_mode: row.get(4)?,
        batch_id: row.get(5)?,
        batch_index: row.get::<_, Option<i64>>(6)?.map(to_usize).transpose()?,
        batch_total: row.get::<_, Option<i64>>(7)?.map(to_usize).transpose()?,
        phase: row.get(8)?,
        arguments_raw: row.get(9)?,
        output_raw: row.get(10)?,
        created_at_unix_ms: row.get(11)?,
        updated_at_unix_ms: row.get(12)?,
    })
}

fn map_approval_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArchiveApprovalRecord> {
    Ok(ArchiveApprovalRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        turn_id: row.get(2)?,
        tool_call_id: row.get(3)?,
        tool_name: row.get(4)?,
        reason: row.get(5)?,
        command: row.get(6)?,
        execution_mode: row.get(7)?,
        batch_id: row.get(8)?,
        batch_index: row.get::<_, Option<i64>>(9)?.map(to_usize).transpose()?,
        batch_total: row.get::<_, Option<i64>>(10)?.map(to_usize).transpose()?,
        created_at_unix_ms: row.get(11)?,
        updated_at_unix_ms: row.get(12)?,
    })
}

fn map_event_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArchiveEventRecord> {
    Ok(ArchiveEventRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        turn_id: row.get(2)?,
        event_type: row.get(3)?,
        title: row.get(4)?,
        summary: row.get(5)?,
        created_at_unix_ms: row.get(6)?,
    })
}

fn read_messages_for_turn(
    conn: &Connection,
    session_id: &str,
    turn_id: &str,
) -> Result<Vec<ArchiveMessageRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, session_id, turn_id, role, content, tool_call_id, raw_json, created_at_unix_ms
        FROM messages
        WHERE session_id = ?1 AND turn_id = ?2
        ORDER BY created_at_unix_ms ASC, id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![session_id, turn_id], map_message_record)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read messages for archive turn")
}

fn read_tool_calls_for_turn(
    conn: &Connection,
    session_id: &str,
    turn_id: &str,
) -> Result<Vec<ArchiveToolCallRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            id, session_id, turn_id, tool_name, execution_mode, batch_id, batch_index,
            batch_total, phase, arguments_raw, output_raw, created_at_unix_ms, updated_at_unix_ms
        FROM tool_calls
        WHERE session_id = ?1 AND turn_id = ?2
        ORDER BY created_at_unix_ms ASC, id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![session_id, turn_id], map_tool_call_record)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read tool calls for archive turn")
}

fn read_approvals_for_turn(
    conn: &Connection,
    session_id: &str,
    turn_id: &str,
) -> Result<Vec<ArchiveApprovalRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            id, session_id, turn_id, tool_call_id, tool_name, reason, command, execution_mode,
            batch_id, batch_index, batch_total, created_at_unix_ms, updated_at_unix_ms
        FROM approvals
        WHERE session_id = ?1 AND turn_id = ?2
        ORDER BY created_at_unix_ms ASC, id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![session_id, turn_id], map_approval_record)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read approvals for archive turn")
}

fn read_events_for_turn(
    conn: &Connection,
    session_id: &str,
    turn_id: &str,
) -> Result<Vec<ArchiveEventRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, session_id, turn_id, event_type, title, summary, created_at_unix_ms
        FROM events
        WHERE session_id = ?1 AND turn_id = ?2
        ORDER BY created_at_unix_ms ASC, id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![session_id, turn_id], map_event_record)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read events for archive turn")
}

fn to_usize(value: i64) -> rusqlite::Result<usize> {
    value.try_into().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn truncate_middle(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ").trim().to_string();
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let head = max_chars / 2;
    let tail = max_chars.saturating_sub(head + 3);
    let start = normalized.chars().take(head).collect::<String>();
    let end = normalized
        .chars()
        .rev()
        .take(tail)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{start}...{end}")
}

fn excerpt_for_query(value: &str, query: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ").trim().to_string();
    let lower = normalized.to_lowercase();
    let query_lower = query.to_lowercase();
    let Some(start) = lower.find(&query_lower) else {
        return truncate_middle(&normalized, max_chars);
    };

    let prefix_chars = lower[..start].chars().count();
    let query_chars = query.chars().count();
    let half_window = max_chars / 2;
    let start_char = prefix_chars.saturating_sub(half_window);
    let end_char = (prefix_chars + query_chars + half_window).min(normalized.chars().count());
    let snippet = normalized
        .chars()
        .skip(start_char)
        .take(end_char.saturating_sub(start_char))
        .collect::<String>();

    if snippet.chars().count() < normalized.chars().count() {
        truncate_middle(&snippet, max_chars)
    } else {
        snippet
    }
}

fn unix_now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::ArchiveStore;
    use crate::session::{StoredSession, StoredToolPhase};
    use crate::types::{ChatMessage, ToolCall, ToolFunctionCall};

    #[test]
    fn appends_session_turn_message_and_tool_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ArchiveStore::new(tmp.path()).expect("archive");

        store
            .upsert_session(
                "session-1",
                Some("Demo"),
                tmp.path(),
                "openai",
                "gpt-test",
                1,
                2,
            )
            .expect("session");
        store
            .upsert_turn("session-1", "turn-1", 1, 10)
            .expect("turn");
        store
            .append_message("session-1", "turn-1", "user", "hello", None, None)
            .expect("message");
        store
            .upsert_tool_call(
                "tool-1",
                "session-1",
                "turn-1",
                "read_file",
                "sequential",
                None,
                None,
                None,
                "done",
                Some("{\"path\":\"README.md\"}"),
                Some("README content"),
            )
            .expect("tool");
        store
            .upsert_approval(
                "approval-1",
                "session-1",
                "turn-1",
                "tool-1",
                "terminal",
                "needs permission",
                "rm -rf /",
                "sequential",
                None,
                None,
                None,
            )
            .expect("approval");
        store
            .append_event(
                "session-1",
                Some("turn-1"),
                "assistant_message",
                "Assistant replied",
                "Done",
            )
            .expect("event");

        let conn = rusqlite::Connection::open(store.path()).expect("open");
        let sessions: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .expect("count sessions");
        let turns: i64 = conn
            .query_row("SELECT COUNT(*) FROM turns", [], |row| row.get(0))
            .expect("count turns");
        let messages: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .expect("count messages");
        let tools: i64 = conn
            .query_row("SELECT COUNT(*) FROM tool_calls", [], |row| row.get(0))
            .expect("count tools");
        let approvals: i64 = conn
            .query_row("SELECT COUNT(*) FROM approvals", [], |row| row.get(0))
            .expect("count approvals");
        let events: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .expect("count events");

        assert_eq!(sessions, 1);
        assert_eq!(turns, 1);
        assert_eq!(messages, 1);
        assert_eq!(tools, 1);
        assert_eq!(approvals, 1);
        assert_eq!(events, 1);
    }

    #[test]
    fn preserves_same_turn_id_across_multiple_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ArchiveStore::new(tmp.path()).expect("archive");

        for session_id in ["session-a", "session-b"] {
            store
                .upsert_session(session_id, None, tmp.path(), "openai", "gpt-test", 1, 1)
                .expect("session");
            store
                .upsert_turn(session_id, "turn-1", 1, 10)
                .expect("turn");
            store
                .append_message(session_id, "turn-1", "user", session_id, None, None)
                .expect("message");
        }

        let session_a = store
            .read_turn("session-a", "turn-1")
            .expect("read turn")
            .expect("turn exists");
        let session_b = store
            .read_turn("session-b", "turn-1")
            .expect("read turn")
            .expect("turn exists");

        assert_eq!(session_a.messages[0].content, "session-a");
        assert_eq!(session_b.messages[0].content, "session-b");
    }

    #[test]
    fn reads_turn_bundles_and_searches_messages() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ArchiveStore::new(tmp.path()).expect("archive");

        store
            .upsert_session(
                "session-1",
                Some("Demo"),
                tmp.path(),
                "openai",
                "gpt-test",
                1,
                2,
            )
            .expect("session");
        store
            .upsert_turn("session-1", "turn-1", 1, 10)
            .expect("turn");
        store
            .append_message(
                "session-1",
                "turn-1",
                "assistant",
                "Adjusted the dialogue UI and collapsed tool cards.",
                None,
                Some("{\"kind\":\"assistant\"}"),
            )
            .expect("message");
        store
            .upsert_tool_call(
                "tool-1",
                "session-1",
                "turn-1",
                "read_file",
                "sequential",
                None,
                None,
                None,
                "done",
                Some("{\"path\":\"src/ui/chat.tsx\"}"),
                Some("file contents"),
            )
            .expect("tool");
        store
            .append_event(
                "session-1",
                Some("turn-1"),
                "assistant_message",
                "Assistant replied",
                "Layout updated",
            )
            .expect("event");

        let bundle = store
            .read_turn("session-1", "turn-1")
            .expect("bundle")
            .expect("turn exists");
        assert_eq!(bundle.messages.len(), 1);
        assert_eq!(bundle.tool_calls.len(), 1);
        assert_eq!(bundle.events.len(), 1);
        assert_eq!(bundle.tool_calls[0].tool_name, "read_file");

        let hits = store
            .search_messages("tool cards", Some("session-1"), 5)
            .expect("search");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.to_lowercase().contains("tool cards"));

        let summaries = store
            .list_turn_summaries("session-1", 5)
            .expect("summaries");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].message_count, 1);
        assert_eq!(summaries[0].tool_call_count, 1);
    }

    #[test]
    fn backfills_stored_sessions_into_archive_idempotently() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ArchiveStore::new(tmp.path()).expect("archive");
        let mut session = StoredSession::new("session-a".to_string(), "gpt-test".to_string());
        session.created_at_unix = 100;
        session.updated_at_unix = 200;
        session.title = Some("Dialogue UI Layout".to_string());
        session.history.push(ChatMessage::user(
            "Keep user messages as bubbles and collapse tool cards.",
        ));
        session.history.push(ChatMessage {
            role: "assistant".to_string(),
            content: Some(serde_json::Value::String(
                "I'll inspect the current chat layout.".to_string(),
            )),
            tool_calls: Some(vec![ToolCall {
                id: "tool-1".to_string(),
                kind: "function".to_string(),
                function: ToolFunctionCall {
                    name: "read_file".to_string(),
                    arguments: "{\"path\":\"src/ui/chat.tsx\"}".to_string(),
                },
            }]),
            tool_call_id: None,
        });
        session.history.push(ChatMessage::tool(
            "tool-1",
            "tool output: flat assistant layout",
        ));
        session.history.push(ChatMessage {
            role: "assistant".to_string(),
            content: Some(serde_json::Value::String(
                "Adjusted the assistant layout and kept user bubbles.".to_string(),
            )),
            tool_calls: None,
            tool_call_id: None,
        });
        session
            .record_user_timeline_entry("Keep user messages as bubbles and collapse tool cards.");
        session.record_tool_timeline_entry(
            "tool-1",
            "read_file",
            "tool output: flat assistant layout",
            StoredToolPhase::Done,
            Some("sequential"),
            None,
            None,
            None,
        );
        session.record_approval_timeline_entry(
            "approval-1",
            "terminal",
            "needs permission",
            "rm -rf /tmp/example",
            Some("sequential"),
            None,
            None,
            None,
        );
        session.record_assistant_timeline_entry(
            "Adjusted the assistant layout and kept user bubbles.",
        );

        assert!(
            !store
                .session_has_content(&session.session_id)
                .expect("inspect content")
        );
        let imported = store
            .backfill_sessions(&[session.clone()], tmp.path(), "openai")
            .expect("backfill");
        assert_eq!(imported, 1);
        assert!(
            store
                .session_has_content(&session.session_id)
                .expect("inspect content")
        );

        let bundle = store
            .read_turn("session-a", "turn-1")
            .expect("read turn")
            .expect("turn exists");
        assert_eq!(bundle.messages.len(), 4);
        assert_eq!(bundle.tool_calls.len(), 2);
        assert_eq!(bundle.approvals.len(), 1);
        assert!(
            bundle
                .events
                .iter()
                .any(|event| event.event_type == "assistant_message")
        );
        assert!(
            bundle
                .events
                .iter()
                .any(|event| event.event_type == "approval_required")
        );

        let tool = store
            .read_tool_call("tool-1")
            .expect("read tool")
            .expect("tool exists");
        assert_eq!(tool.tool_name, "read_file");
        assert!(
            tool.arguments_raw
                .as_deref()
                .unwrap_or_default()
                .contains("chat.tsx")
        );
        assert!(
            tool.output_raw
                .as_deref()
                .unwrap_or_default()
                .contains("flat assistant layout")
        );

        let imported_again = store
            .backfill_sessions(&[session], tmp.path(), "openai")
            .expect("backfill again");
        assert_eq!(imported_again, 0);

        let conn = rusqlite::Connection::open(store.path()).expect("open");
        let messages: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = 'session-a'",
                [],
                |row| row.get(0),
            )
            .expect("count messages");
        assert_eq!(messages, 4);
    }
}
