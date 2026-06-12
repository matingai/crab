use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};

use crate::bridge::{
    AgentBridge, BridgeEventEnvelope, BridgeEventSink, BridgeRunResult, BridgeSkillDetail,
    BridgeSkillSummary, RunAgentRequest, RunCronJobRequest, SessionCommandRequest,
    SimpleSessionResponse,
};

pub const TAURI_AGENT_EVENT: &str = "hermes://agent/event";
pub const TAURI_AGENT_DONE_EVENT: &str = "hermes://agent/done";
pub const TAURI_AGENT_CLEARED_EVENT: &str = "hermes://agent/cleared";

pub trait TauriEmitter: Send + Sync {
    fn emit_json(&self, event_name: &str, payload: Value) -> Result<()>;
}

pub struct TauriEventBridge<E> {
    emitter: E,
}

impl<E> TauriEventBridge<E>
where
    E: TauriEmitter,
{
    pub fn new(emitter: E) -> Self {
        Self { emitter }
    }

    pub async fn run_agent(&self, request: RunAgentRequest) -> Result<BridgeRunResult> {
        let session_hint = request.session_id.clone();
        let mut sink = TauriEventSink::new(&self.emitter, session_hint);
        let result = AgentBridge::run_with_event_sink(request, &mut sink).await?;
        if let Some(error) = sink.take_error() {
            return Err(anyhow!(error));
        }
        if result.status == "completed" {
            self.emit(
                TAURI_AGENT_DONE_EVENT,
                &TauriRunCompletedPayload {
                    session_id: result.session_id.clone(),
                    response: result.response.clone(),
                },
            )?;
            self.emit(&tauri_session_done_event_name(&result.session_id), &result)?;
        }
        Ok(result)
    }

    pub async fn resume_approval(
        &self,
        request: SessionCommandRequest,
        approval_id: String,
    ) -> Result<BridgeRunResult> {
        let session_hint = Some(request.session_id.clone());
        let mut sink = TauriEventSink::new(&self.emitter, session_hint);
        let result =
            AgentBridge::resume_approval_with_event_sink(request, approval_id, &mut sink).await?;
        if let Some(error) = sink.take_error() {
            return Err(anyhow!(error));
        }
        if result.status == "completed" {
            self.emit(
                TAURI_AGENT_DONE_EVENT,
                &TauriRunCompletedPayload {
                    session_id: result.session_id.clone(),
                    response: result.response.clone(),
                },
            )?;
            self.emit(&tauri_session_done_event_name(&result.session_id), &result)?;
        }
        Ok(result)
    }

    pub async fn run_cron_job(&self, request: RunCronJobRequest) -> Result<BridgeRunResult> {
        let mut sink = TauriEventSink::new(&self.emitter, None);
        let result = AgentBridge::run_cron_job_with_event_sink(request, &mut sink).await?;
        if let Some(error) = sink.take_error() {
            return Err(anyhow!(error));
        }
        if result.status == "completed" {
            self.emit(
                TAURI_AGENT_DONE_EVENT,
                &TauriRunCompletedPayload {
                    session_id: result.session_id.clone(),
                    response: result.response.clone(),
                },
            )?;
            self.emit(&tauri_session_done_event_name(&result.session_id), &result)?;
        }
        Ok(result)
    }

    pub fn clear_session(&self, request: SessionCommandRequest) -> Result<SimpleSessionResponse> {
        let response = AgentBridge::clear_session(request)?;
        self.emit(TAURI_AGENT_CLEARED_EVENT, &response)?;
        Ok(response)
    }

    pub fn list_skills(&self, data_dir: std::path::PathBuf) -> Result<Vec<BridgeSkillSummary>> {
        AgentBridge::list_skills(data_dir)
    }

    pub fn view_skill(
        &self,
        data_dir: std::path::PathBuf,
        name: String,
        category: Option<String>,
        file_path: Option<String>,
    ) -> Result<BridgeSkillDetail> {
        AgentBridge::view_skill(data_dir, name, category, file_path)
    }

    fn emit<T: Serialize>(&self, event_name: &str, payload: &T) -> Result<()> {
        let value = serde_json::to_value(payload).context("failed to serialize tauri payload")?;
        self.emitter.emit_json(event_name, value)
    }
}

struct TauriEventSink<'a, E> {
    emitter: &'a E,
    session_hint: Option<String>,
    first_error: Option<String>,
}

impl<'a, E> TauriEventSink<'a, E>
where
    E: TauriEmitter,
{
    fn new(emitter: &'a E, session_hint: Option<String>) -> Self {
        Self {
            emitter,
            session_hint,
            first_error: None,
        }
    }

    fn take_error(&mut self) -> Option<String> {
        self.first_error.take()
    }
}

impl<E> BridgeEventSink for TauriEventSink<'_, E>
where
    E: TauriEmitter,
{
    fn push(&mut self, event: BridgeEventEnvelope) {
        if let Err(error) = self.emitter.emit_json(
            TAURI_AGENT_EVENT,
            serde_json::to_value(&event).unwrap_or(Value::Null),
        ) {
            if self.first_error.is_none() {
                self.first_error = Some(error.to_string());
            }
        }
        let session_id = event_session_id(&event).or_else(|| self.session_hint.clone());
        if let Some(session_id) = session_id {
            if let Err(error) = self.emitter.emit_json(
                &tauri_session_event_name(&session_id),
                serde_json::to_value(&event).unwrap_or(Value::Null),
            ) {
                if self.first_error.is_none() {
                    self.first_error = Some(error.to_string());
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct TauriRunCompletedPayload {
    session_id: String,
    response: String,
}

pub fn tauri_session_event_name(session_id: &str) -> String {
    format!("{TAURI_AGENT_EVENT}/{session_id}")
}

pub fn tauri_session_done_event_name(session_id: &str) -> String {
    format!("{TAURI_AGENT_DONE_EVENT}/{session_id}")
}

fn event_session_id(event: &BridgeEventEnvelope) -> Option<String> {
    match &event.event {
        crate::events::AgentEvent::SessionReady { session_id, .. }
        | crate::events::AgentEvent::TurnStarted { session_id, .. }
        | crate::events::AgentEvent::TurnFinished { session_id, .. }
        | crate::events::AgentEvent::TurnInterrupted { session_id, .. }
        | crate::events::AgentEvent::IterationStarted { session_id, .. }
        | crate::events::AgentEvent::AssistantDelta { session_id, .. }
        | crate::events::AgentEvent::SkillMatched { session_id, .. }
        | crate::events::AgentEvent::GoalStateUpdated { session_id, .. }
        | crate::events::AgentEvent::TodoStateUpdated { session_id, .. }
        | crate::events::AgentEvent::SolveTraceUpdated { session_id, .. }
        | crate::events::AgentEvent::ContextPrepared { session_id, .. }
        | crate::events::AgentEvent::ContextCompacted { session_id, .. }
        | crate::events::AgentEvent::ModelRecovery { session_id, .. }
        | crate::events::AgentEvent::ModelRequestStarted { session_id, .. }
        | crate::events::AgentEvent::ModelRequestFinished { session_id, .. }
        | crate::events::AgentEvent::BackgroundModelRequestStarted { session_id, .. }
        | crate::events::AgentEvent::BackgroundModelRequestFinished { session_id, .. }
        | crate::events::AgentEvent::SkillLifecycleSuggested { session_id, .. }
        | crate::events::AgentEvent::ToolBatchStarted { session_id, .. }
        | crate::events::AgentEvent::ToolBatchProgress { session_id, .. }
        | crate::events::AgentEvent::ToolBatchFinished { session_id, .. }
        | crate::events::AgentEvent::ToolCallStarted { session_id, .. }
        | crate::events::AgentEvent::ToolCallDelta { session_id, .. }
        | crate::events::AgentEvent::ToolCallFinished { session_id, .. }
        | crate::events::AgentEvent::ApprovalRequired { session_id, .. }
        | crate::events::AgentEvent::ApprovalResolved { session_id, .. }
        | crate::events::AgentEvent::AssistantMessage { session_id, .. }
        | crate::events::AgentEvent::SessionSaved { session_id, .. }
        | crate::events::AgentEvent::Nudge { session_id, .. }
        | crate::events::AgentEvent::Error { session_id, .. } => Some(session_id.clone()),
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecordingTauriEmitter {
    events: Arc<Mutex<Vec<(String, Value)>>>,
}

impl RecordingTauriEmitter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<(String, Value)> {
        self.events.lock().expect("events").clone()
    }
}

impl TauriEmitter for RecordingTauriEmitter {
    fn emit_json(&self, event_name: &str, payload: Value) -> Result<()> {
        self.events
            .lock()
            .expect("events")
            .push((event_name.to_string(), payload));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RecordingTauriEmitter, TAURI_AGENT_EVENT, TauriEmitter, TauriEventBridge,
        tauri_session_done_event_name, tauri_session_event_name,
    };
    use crate::bridge::BridgeEventEnvelope;
    use crate::events::AgentEvent;

    #[test]
    fn session_event_name_is_stable() {
        assert_eq!(
            tauri_session_event_name("abc"),
            "hermes://agent/event/abc".to_string()
        );
        assert_eq!(
            tauri_session_done_event_name("abc"),
            "hermes://agent/done/abc".to_string()
        );
    }

    #[test]
    fn recording_emitter_captures_payloads() {
        let emitter = RecordingTauriEmitter::new();
        emitter
            .emit_json(TAURI_AGENT_EVENT, serde_json::json!({ "ok": true }))
            .expect("emit");
        let events = emitter.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, TAURI_AGENT_EVENT);
        assert_eq!(events[0].1["ok"], true);
    }

    #[test]
    fn tauri_bridge_lists_skills_through_core_bridge() {
        let emitter = RecordingTauriEmitter::new();
        let bridge = TauriEventBridge::new(emitter);
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            "skills:\n  include_bundled: false\n",
        )
        .expect("write config");
        let skills = bridge
            .list_skills(tmp.path().to_path_buf())
            .expect("skills");
        assert!(skills.is_empty());
    }

    #[test]
    fn tauri_bridge_views_skill_through_core_bridge() {
        let emitter = RecordingTauriEmitter::new();
        let bridge = TauriEventBridge::new(emitter);
        let tmp = tempfile::tempdir().expect("tempdir");
        let skills_dir = tmp.path().join("skills").join("coding").join("rust-review");
        std::fs::create_dir_all(skills_dir.join("references")).expect("mkdir refs");
        std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: rust-review\ndescription: Review rust\n---\n\n# Rust Review\n",
        )
        .expect("write skill");
        std::fs::write(
            skills_dir.join("references").join("api.md"),
            "# API\n\nReference content\n",
        )
        .expect("write ref");

        let skill = bridge
            .view_skill(
                tmp.path().to_path_buf(),
                "rust-review".to_string(),
                Some("coding".to_string()),
                Some("references/api.md".to_string()),
            )
            .expect("skill");
        assert_eq!(skill.file_path, "references/api.md");
        assert!(skill.content.contains("Reference content"));
    }

    #[test]
    fn event_payload_can_be_emitted_on_both_channels() {
        let emitter = RecordingTauriEmitter::new();
        let event = BridgeEventEnvelope {
            seq: 1,
            event_type: "session_ready".to_string(),
            emitted_at_unix_ms: 1,
            event: AgentEvent::SessionReady {
                session_id: "demo".to_string(),
                resumed: false,
            },
        };

        emitter
            .emit_json(
                TAURI_AGENT_EVENT,
                serde_json::to_value(&event).expect("json"),
            )
            .expect("emit");
        emitter
            .emit_json(
                &tauri_session_event_name("demo"),
                serde_json::to_value(&event).expect("json"),
            )
            .expect("emit");

        let events = emitter.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, TAURI_AGENT_EVENT);
        assert_eq!(events[1].0, "hermes://agent/event/demo");
    }

    #[test]
    fn batch_events_resolve_session_channel() {
        let event = BridgeEventEnvelope {
            seq: 1,
            event_type: "tool_batch_started".to_string(),
            emitted_at_unix_ms: 1,
            event: AgentEvent::ToolBatchStarted {
                session_id: "demo".to_string(),
                iteration: 2,
                batch_id: "parallel-2-call-1".to_string(),
                total_calls: 3,
            },
        };

        assert_eq!(super::event_session_id(&event), Some("demo".to_string()));
    }
}
