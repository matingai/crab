use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::agent::Agent;
use crate::approval::ApprovalRequest;
use crate::config::{
    AppConfig, env_flag, experience_context_enabled, meta_pattern_context_enabled,
    resolve_auxiliary_model,
};
use crate::cron::{
    CronJobDefinition, CronJobSummary, delete_cron_job_definition, finalize_run_record,
    find_cron_job, load_cron_job_summaries, new_running_record, save_cron_job_definition,
    save_run_record,
};
use crate::delegate_runs::{
    DelegateRunRecord, finalize_record as finalize_delegate_record,
    list_records as list_delegate_run_records, load_record as load_delegate_run_record,
    new_record as new_delegate_record, save_record as save_delegate_record,
};
use crate::events::{AgentEvent, EventHandler};
use crate::extensions::ExtensionsOverview;
use crate::mcp::{McpServerInspection, inspect_server};
use crate::providers::{
    ProviderResolutionRequest, ProviderSummary, load_provider_summaries, resolve_runtime_provider,
};
use crate::runtime_control::request_stop;
use crate::runtime_profile::RuntimeProfile;
use crate::session::{SessionSearchHit, SessionStore, SessionTimelineEntry, StoredSession};
use crate::shared_config::{SharedAgentConfig, load_shared_agent_config, save_shared_agent_config};
use crate::skills::{SkillLinkedFile, SkillStore, SkillView};
use crate::smart_model_routing::load_smart_model_routing;
use crate::todo::{TodoItem, TodoStore};
use crate::types::ChatMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentRequest {
    pub prompt: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub aux_provider: Option<String>,
    pub aux_model: Option<String>,
    pub aux_base_url: Option<String>,
    pub aux_api_key: Option<String>,
    pub workspace_root: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub session_id: Option<String>,
    pub max_iterations: Option<usize>,
    pub system_prompt_override: Option<String>,
    pub enable_shell_tool: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveProviderStatusRequest {
    pub data_dir: PathBuf,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeRunResult {
    pub session_id: String,
    pub response: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeProviderRuntimeStatus {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub model: String,
    pub base_url: String,
    pub api_mode: String,
    pub auth_source: Option<String>,
    pub auth_required: bool,
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedProviderConfigRequest {
    pub data_dir: PathBuf,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub aux_model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeEventEnvelope {
    pub seq: usize,
    pub event_type: String,
    pub emitted_at_unix_ms: u128,
    pub event: AgentEvent,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunAgentResponse {
    pub session_id: String,
    pub response: String,
    pub events: Vec<BridgeEventEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCommandRequest {
    pub workspace_root: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub session_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub aux_provider: Option<String>,
    pub aux_model: Option<String>,
    pub aux_base_url: Option<String>,
    pub aux_api_key: Option<String>,
    pub max_iterations: Option<usize>,
    pub system_prompt_override: Option<String>,
    pub enable_shell_tool: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCronJobRequest {
    pub workspace_root: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub job_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub aux_provider: Option<String>,
    pub aux_model: Option<String>,
    pub aux_base_url: Option<String>,
    pub aux_api_key: Option<String>,
    pub max_iterations: Option<usize>,
    pub system_prompt_override: Option<String>,
    pub enable_shell_tool: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimpleSessionResponse {
    pub session_id: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryDelegateRunRequest {
    pub workspace_root: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub run_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub aux_provider: Option<String>,
    pub aux_model: Option<String>,
    pub aux_base_url: Option<String>,
    pub aux_api_key: Option<String>,
    pub max_iterations: Option<usize>,
    pub system_prompt_override: Option<String>,
    pub enable_shell_tool: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCronJobRequest {
    pub previous_id: Option<String>,
    pub id: String,
    pub schedule: String,
    pub prompt: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeSkillSummary {
    pub category: String,
    pub name: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub task_kinds: Vec<String>,
    pub requires_tools: Vec<String>,
    pub requires_shell: bool,
    pub updated_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeSkillFile {
    pub path: String,
    pub size_bytes: u64,
    pub file_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeSkillDetail {
    pub category: String,
    pub name: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub task_kinds: Vec<String>,
    pub requires_tools: Vec<String>,
    pub requires_shell: bool,
    pub updated_at_unix: Option<u64>,
    pub file_path: String,
    pub file_type: String,
    pub content: String,
    pub is_binary: bool,
    pub linked_files: std::collections::BTreeMap<String, Vec<BridgeSkillFile>>,
    pub required_environment_variables: Vec<BridgeSkillEnvRequirement>,
    pub missing_required_environment_variables: Vec<String>,
    pub required_commands: Vec<String>,
    pub missing_required_commands: Vec<String>,
    pub config_requirements: Vec<BridgeSkillConfigRequirement>,
    pub setup_needed: bool,
    pub readiness_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeSkillEnvRequirement {
    pub name: String,
    pub prompt: String,
    pub help: Option<String>,
    pub required_for: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeSkillConfigRequirement {
    pub key: String,
    pub description: String,
    pub prompt: Option<String>,
    pub default_value: Option<String>,
    pub resolved_value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeSessionSummary {
    pub session_id: String,
    pub title: Option<String>,
    pub model: String,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    pub message_count: usize,
    pub user_turns: usize,
    pub assistant_turns: usize,
    pub last_user_message: Option<String>,
    pub last_assistant_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeSessionDetail {
    pub summary: BridgeSessionSummary,
    pub history: Vec<ChatMessage>,
    pub timeline: Vec<SessionTimelineEntry>,
    pub todos: Vec<TodoItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeSessionSearchResult {
    pub summary: BridgeSessionSummary,
    pub score: usize,
    pub match_count: usize,
    pub matched_messages: usize,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeDelegateRun {
    pub id: String,
    pub parent_session_id: String,
    pub parent_delegate_run_id: Option<String>,
    pub root_delegate_run_id: String,
    pub session_id: String,
    pub prompt: String,
    pub prompt_preview: String,
    pub status: String,
    pub result_preview: String,
    pub max_iterations: usize,
    pub attempt: usize,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

pub trait BridgeEventSink: Send {
    fn push(&mut self, event: BridgeEventEnvelope);
}

#[derive(Default)]
pub struct RecordingBridgeEventSink {
    events: Vec<BridgeEventEnvelope>,
}

impl RecordingBridgeEventSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_events(self) -> Vec<BridgeEventEnvelope> {
        self.events
    }

    pub fn events(&self) -> &[BridgeEventEnvelope] {
        &self.events
    }
}

impl BridgeEventSink for RecordingBridgeEventSink {
    fn push(&mut self, event: BridgeEventEnvelope) {
        self.events.push(event);
    }
}

static RUNNING_SESSIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

struct SessionRunGuard {
    session_id: Option<String>,
}

impl SessionRunGuard {
    fn acquire(session_id: &str) -> Result<Self> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Ok(Self { session_id: None });
        }

        let mut running = RUNNING_SESSIONS
            .lock()
            .map_err(|error| anyhow::anyhow!("failed to lock session run registry: {error}"))?;
        if running.contains(session_id) {
            anyhow::bail!(
                "会话 {} 正在执行中，请等待完成或先停止当前任务。",
                session_id
            );
        }
        running.insert(session_id.to_string());

        Ok(Self {
            session_id: Some(session_id.to_string()),
        })
    }
}

impl Drop for SessionRunGuard {
    fn drop(&mut self) {
        let Some(session_id) = self.session_id.as_deref() else {
            return;
        };
        if let Ok(mut running) = RUNNING_SESSIONS.lock() {
            running.remove(session_id);
        }
    }
}

struct BridgeEventForwarder<'a> {
    sink: &'a mut dyn BridgeEventSink,
    next_seq: usize,
}

impl<'a> BridgeEventForwarder<'a> {
    fn new(sink: &'a mut dyn BridgeEventSink) -> Self {
        Self { sink, next_seq: 0 }
    }
}

impl EventHandler for BridgeEventForwarder<'_> {
    fn on_event(&mut self, event: AgentEvent) {
        self.next_seq += 1;
        self.sink.push(BridgeEventEnvelope {
            seq: self.next_seq,
            event_type: event.event_type().to_string(),
            emitted_at_unix_ms: unix_now_ms(),
            event,
        });
    }
}

pub struct AgentBridge;

impl AgentBridge {
    pub async fn run(request: RunAgentRequest) -> Result<RunAgentResponse> {
        let mut sink = RecordingBridgeEventSink::new();
        let result = Self::run_with_event_sink(request, &mut sink).await?;
        Ok(RunAgentResponse {
            session_id: result.session_id,
            response: result.response,
            events: sink.into_events(),
        })
    }

    pub async fn run_with_event_sink(
        request: RunAgentRequest,
        sink: &mut dyn BridgeEventSink,
    ) -> Result<BridgeRunResult> {
        let config = build_run_config(&request)?;
        let mut agent = Agent::new(config)?;
        let _session_guard = SessionRunGuard::acquire(agent.session_id())?;
        let mut handler = BridgeEventForwarder::new(sink);
        let response = agent
            .run_prompt_with_handler(&request.prompt, &mut handler)
            .await?;
        Ok(BridgeRunResult {
            session_id: agent.session_id().to_string(),
            status: run_status_for_response(&response).to_string(),
            response,
        })
    }

    pub async fn resume_approval_with_event_sink(
        request: SessionCommandRequest,
        approval_id: String,
        sink: &mut dyn BridgeEventSink,
    ) -> Result<BridgeRunResult> {
        let config = build_session_config(&request)?;
        let mut agent = Agent::new(config)?;
        let _session_guard = SessionRunGuard::acquire(agent.session_id())?;
        let mut handler = BridgeEventForwarder::new(sink);
        let response = agent
            .resume_pending_approval_with_handler(&approval_id, &mut handler)
            .await?;
        Ok(BridgeRunResult {
            session_id: agent.session_id().to_string(),
            status: run_status_for_response(&response).to_string(),
            response,
        })
    }

    pub async fn run_cron_job_with_event_sink(
        request: RunCronJobRequest,
        sink: &mut dyn BridgeEventSink,
    ) -> Result<BridgeRunResult> {
        let config = build_cron_run_config(&request)?;
        let data_dir = config.data_dir.clone();
        let job = find_cron_job(&config.data_dir, &request.job_id)?;
        crate::cron::ensure_job_enabled(&job)?;

        let mut agent = Agent::new(config)?;
        let session_id = agent.session_id().to_string();
        let _session_guard = SessionRunGuard::acquire(&session_id)?;
        let mut record = new_running_record(&job.id, &session_id);
        save_run_record(&data_dir, &record)?;

        let prompt = format!(
            "Cron job `{}` scheduled as `{}` triggered this run.\n\n{}",
            job.id, job.schedule, job.prompt
        );
        let mut handler = BridgeEventForwarder::new(sink);
        let response = match agent.run_prompt_with_handler(&prompt, &mut handler).await {
            Ok(response) => {
                let status = run_status_for_response(&response);
                finalize_run_record(&mut record, status, truncated_preview(&response));
                save_run_record(&data_dir, &record)?;
                response
            }
            Err(error) => {
                finalize_run_record(&mut record, "failed", truncated_preview(&error.to_string()));
                save_run_record(&data_dir, &record)?;
                return Err(error);
            }
        };

        Ok(BridgeRunResult {
            session_id,
            status: run_status_for_response(&response).to_string(),
            response,
        })
    }

    pub fn clear_session(request: SessionCommandRequest) -> Result<SimpleSessionResponse> {
        let config = build_session_config(&request)?;
        let mut agent = Agent::new(config)?;
        agent.clear_history()?;
        Ok(SimpleSessionResponse {
            session_id: request.session_id,
            ok: true,
        })
    }

    pub fn stop_session(data_dir: PathBuf, session_id: String) -> Result<SimpleSessionResponse> {
        request_stop(&data_dir, &session_id)?;
        Ok(SimpleSessionResponse {
            session_id,
            ok: true,
        })
    }

    pub fn list_approvals(data_dir: PathBuf) -> Result<Vec<ApprovalRequest>> {
        crate::approval::list_requests(&data_dir)
    }

    pub fn resolve_approval(
        data_dir: PathBuf,
        approval_id: String,
        approved: bool,
    ) -> Result<ApprovalRequest> {
        crate::approval::resolve_request(&data_dir, &approval_id, approved)
    }

    pub async fn resume_approval(
        request: SessionCommandRequest,
        approval_id: String,
    ) -> Result<RunAgentResponse> {
        let mut sink = RecordingBridgeEventSink::new();
        let result = Self::resume_approval_with_event_sink(request, approval_id, &mut sink).await?;
        Ok(RunAgentResponse {
            session_id: result.session_id,
            response: result.response,
            events: sink.into_events(),
        })
    }

    pub async fn run_cron_job(request: RunCronJobRequest) -> Result<RunAgentResponse> {
        let mut sink = RecordingBridgeEventSink::new();
        let result = Self::run_cron_job_with_event_sink(request, &mut sink).await?;
        Ok(RunAgentResponse {
            session_id: result.session_id,
            response: result.response,
            events: sink.into_events(),
        })
    }

    pub fn list_skills(data_dir: PathBuf) -> Result<Vec<BridgeSkillSummary>> {
        let store = SkillStore::new_with_platform(data_dir, Some("desktop"))?;
        Ok(store.list()?.into_iter().map(build_skill_summary).collect())
    }

    pub fn view_skill(
        data_dir: PathBuf,
        name: String,
        category: Option<String>,
        file_path: Option<String>,
    ) -> Result<BridgeSkillDetail> {
        let store = SkillStore::new_with_platform(data_dir, Some("desktop"))?;
        let skill = store.view_with_file(&name, category.as_deref(), file_path.as_deref())?;
        Ok(build_skill_detail(skill))
    }

    pub fn extensions_overview(data_dir: PathBuf) -> Result<ExtensionsOverview> {
        crate::extensions::load_extensions_overview(&data_dir)
    }

    pub fn list_providers(data_dir: PathBuf) -> Result<Vec<ProviderSummary>> {
        load_provider_summaries(&data_dir)
    }

    pub fn resolve_provider_status(
        request: ResolveProviderStatusRequest,
    ) -> Result<BridgeProviderRuntimeStatus> {
        let resolved = resolve_runtime_provider(
            &request.data_dir,
            ProviderResolutionRequest {
                provider: request.provider,
                model: request.model,
                base_url: request.base_url,
                api_key: request.api_key,
            },
        )?;
        let auth_required = !is_local_endpoint(&resolved.base_url);
        let ready = resolved.auth_source.is_some() || !auth_required;
        Ok(BridgeProviderRuntimeStatus {
            id: resolved.id,
            label: resolved.label,
            kind: resolved.kind,
            model: resolved.model,
            base_url: resolved.base_url,
            api_mode: resolved.api_mode.as_str().to_string(),
            auth_source: resolved.auth_source,
            auth_required,
            ready,
        })
    }

    pub fn load_shared_provider_config(data_dir: PathBuf) -> Result<SharedAgentConfig> {
        load_shared_agent_config(&data_dir)
    }

    pub fn save_shared_provider_config(
        request: SharedProviderConfigRequest,
    ) -> Result<SharedAgentConfig> {
        let config = SharedAgentConfig {
            configured: false,
            provider: request.provider,
            model: request.model,
            base_url: request.base_url,
            api_key: request.api_key,
            aux_model: request.aux_model,
        };
        save_shared_agent_config(&request.data_dir, &config)
    }

    pub fn save_cron_job(data_dir: PathBuf, request: SaveCronJobRequest) -> Result<CronJobSummary> {
        let job = CronJobDefinition {
            id: request.id.trim().to_string(),
            schedule: request.schedule.trim().to_string(),
            prompt: request.prompt.trim().to_string(),
            enabled: request.enabled,
        };
        if job.id.is_empty() {
            anyhow::bail!("cron job id is required");
        }
        if job.schedule.is_empty() {
            anyhow::bail!("cron schedule is required");
        }
        if job.prompt.is_empty() {
            anyhow::bail!("cron prompt is required");
        }

        let validation_job = CronJobDefinition {
            enabled: true,
            ..job.clone()
        };
        crate::cron::next_run_at(&validation_job, None, unix_now_ms() as u64 / 1000)?;
        save_cron_job_definition(&data_dir, request.previous_id.as_deref(), &job)?;
        load_cron_job_summaries(&data_dir)?
            .into_iter()
            .find(|item| item.id == job.id)
            .ok_or_else(|| anyhow::anyhow!("saved cron job `{}` not found", job.id))
    }

    pub fn delete_cron_job(data_dir: PathBuf, job_id: String) -> Result<SimpleSessionResponse> {
        delete_cron_job_definition(&data_dir, job_id.trim())?;
        Ok(SimpleSessionResponse {
            session_id: job_id,
            ok: true,
        })
    }

    pub async fn inspect_mcp_server(
        data_dir: PathBuf,
        server_name: String,
    ) -> Result<McpServerInspection> {
        inspect_server(&data_dir, &server_name).await
    }

    pub fn list_sessions(data_dir: PathBuf) -> Result<Vec<BridgeSessionSummary>> {
        let store = SessionStore::new(data_dir)?;
        Ok(store
            .list()?
            .into_iter()
            .map(build_session_summary)
            .collect())
    }

    pub fn load_session(
        data_dir: PathBuf,
        session_id: String,
    ) -> Result<Option<BridgeSessionDetail>> {
        let store = SessionStore::new(data_dir.clone())?;
        let todo_store = TodoStore::new(data_dir)?;
        let Some(session) = store.load(&session_id)? else {
            return Ok(None);
        };
        let summary = build_session_summary(session.clone());
        Ok(Some(BridgeSessionDetail {
            summary,
            history: session.history,
            timeline: session.timeline,
            todos: todo_store.load(&session_id)?,
        }))
    }

    pub fn search_sessions(
        data_dir: PathBuf,
        query: String,
        limit: Option<usize>,
    ) -> Result<Vec<BridgeSessionSearchResult>> {
        let store = SessionStore::new(data_dir)?;
        Ok(store
            .search(&query, limit.unwrap_or(10))?
            .into_iter()
            .map(build_session_search_result)
            .collect())
    }

    pub fn list_delegate_runs(
        data_dir: PathBuf,
        parent_session_id: Option<String>,
    ) -> Result<Vec<BridgeDelegateRun>> {
        Ok(
            list_delegate_run_records(&data_dir, parent_session_id.as_deref())?
                .into_iter()
                .map(build_delegate_run)
                .collect(),
        )
    }

    pub fn cancel_delegate_run(data_dir: PathBuf, run_id: String) -> Result<BridgeDelegateRun> {
        let mut record = load_delegate_run_record(&data_dir, &run_id)?
            .ok_or_else(|| anyhow::anyhow!("delegate run `{run_id}` not found"))?;
        request_stop(&data_dir, &record.session_id)?;
        finalize_delegate_record(
            &mut record,
            "cancel_requested",
            "cancel requested by operator",
        );
        save_delegate_record(&data_dir, &record)?;
        Ok(build_delegate_run(record))
    }

    pub async fn retry_delegate_run_with_event_sink(
        request: RetryDelegateRunRequest,
        sink: &mut dyn BridgeEventSink,
    ) -> Result<BridgeDelegateRun> {
        let data_dir = request
            .data_dir
            .clone()
            .unwrap_or_else(|| request.workspace_root.join(".hermes-agent-rs"));
        let previous = load_delegate_run_record(&data_dir, &request.run_id)?
            .ok_or_else(|| anyhow::anyhow!("delegate run `{}` not found", request.run_id))?;
        let session_id = format!(
            "{}.delegate.{}",
            previous.parent_session_id,
            Uuid::new_v4().simple()
        );
        let mut record = new_delegate_record(
            &previous.parent_session_id,
            previous.parent_delegate_run_id.as_deref(),
            &session_id,
            &previous.prompt,
            request.max_iterations.unwrap_or(previous.max_iterations),
            previous.attempt + 1,
            Some(&previous.root_delegate_run_id),
        );
        record.worker_task = previous.worker_task.clone();
        save_delegate_record(&data_dir, &record)?;

        let mut agent = Agent::new(build_retry_delegate_config(&request, &session_id)?)?;
        let _session_guard = SessionRunGuard::acquire(agent.session_id())?;
        agent.set_delegate_run_id(Some(record.id.clone()));
        let mut handler = BridgeEventForwarder::new(sink);
        match agent
            .run_prompt_with_handler(&record.prompt, &mut handler)
            .await
        {
            Ok(response) => {
                let status = if response == "approval_pending" {
                    "awaiting_approval"
                } else {
                    "completed"
                };
                finalize_delegate_record(&mut record, status, &response);
                save_delegate_record(&data_dir, &record)?;
                Ok(build_delegate_run(record))
            }
            Err(error) => {
                let status = if error.to_string().contains("stop requested") {
                    "canceled"
                } else {
                    "failed"
                };
                finalize_delegate_record(&mut record, status, &error.to_string());
                save_delegate_record(&data_dir, &record)?;
                Err(error)
            }
        }
    }

    pub async fn retry_delegate_run(request: RetryDelegateRunRequest) -> Result<BridgeDelegateRun> {
        let mut sink = RecordingBridgeEventSink::new();
        Self::retry_delegate_run_with_event_sink(request, &mut sink).await
    }
}

fn build_skill_summary(skill: crate::skills::SkillSummary) -> BridgeSkillSummary {
    BridgeSkillSummary {
        category: skill.category,
        name: skill.name,
        description: skill.description,
        keywords: skill.keywords,
        task_kinds: skill.activation.task_kinds,
        requires_tools: skill.activation.requires_tools,
        requires_shell: skill.activation.requires_shell,
        updated_at_unix: skill.updated_at_unix,
    }
}

fn build_delegate_run(record: DelegateRunRecord) -> BridgeDelegateRun {
    BridgeDelegateRun {
        id: record.id,
        parent_session_id: record.parent_session_id,
        parent_delegate_run_id: record.parent_delegate_run_id,
        root_delegate_run_id: record.root_delegate_run_id,
        session_id: record.session_id,
        prompt: record.prompt,
        prompt_preview: record.prompt_preview,
        status: record.status,
        result_preview: record.result_preview,
        max_iterations: record.max_iterations,
        attempt: record.attempt,
        created_at_unix: record.created_at_unix,
        updated_at_unix: record.updated_at_unix,
    }
}

fn build_skill_detail(skill: SkillView) -> BridgeSkillDetail {
    BridgeSkillDetail {
        category: skill.summary.category,
        name: skill.summary.name,
        description: skill.summary.description,
        keywords: skill.summary.keywords,
        task_kinds: skill.summary.activation.task_kinds,
        requires_tools: skill.summary.activation.requires_tools,
        requires_shell: skill.summary.activation.requires_shell,
        updated_at_unix: skill.summary.updated_at_unix,
        file_path: skill.file_path,
        file_type: skill.file_type,
        content: skill.content,
        is_binary: skill.is_binary,
        linked_files: skill
            .linked_files
            .into_iter()
            .map(|(group, files)| {
                (
                    group,
                    files.into_iter().map(build_skill_file).collect::<Vec<_>>(),
                )
            })
            .collect(),
        required_environment_variables: skill
            .readiness
            .required_environment_variables
            .iter()
            .map(|item| BridgeSkillEnvRequirement {
                name: item.name.clone(),
                prompt: item.prompt.clone(),
                help: item.help.clone(),
                required_for: item.required_for.clone(),
            })
            .collect(),
        missing_required_environment_variables: skill
            .readiness
            .missing_required_environment_variables
            .clone(),
        required_commands: skill.readiness.required_commands.clone(),
        missing_required_commands: skill.readiness.missing_required_commands.clone(),
        config_requirements: skill
            .readiness
            .config_requirements
            .iter()
            .map(|item| BridgeSkillConfigRequirement {
                key: item.key.clone(),
                description: item.description.clone(),
                prompt: item.prompt.clone(),
                default_value: item.default_value.clone(),
                resolved_value: item.resolved_value.clone(),
            })
            .collect(),
        setup_needed: skill.readiness.setup_needed,
        readiness_status: skill.readiness.readiness_status.clone(),
    }
}

fn build_skill_file(file: SkillLinkedFile) -> BridgeSkillFile {
    BridgeSkillFile {
        path: file.path,
        size_bytes: file.size_bytes,
        file_type: file.file_type,
    }
}

fn build_run_config(request: &RunAgentRequest) -> Result<AppConfig> {
    let data_dir = request
        .data_dir
        .clone()
        .unwrap_or_else(|| request.workspace_root.join(".hermes-agent-rs"));
    let provider = resolve_primary_provider(
        &data_dir,
        request.provider.clone(),
        request.model.clone(),
        request.base_url.clone(),
        request.api_key.clone(),
    );
    let auxiliary_model = resolve_auxiliary_model(
        &data_dir,
        &provider,
        request.aux_provider.clone(),
        request.aux_model.clone(),
        request.aux_base_url.clone(),
        request.aux_api_key.clone(),
    )
    .unwrap_or(None);
    let smart_model_routing = load_smart_model_routing(&data_dir).unwrap_or(None);
    let runtime_profile = RuntimeProfile::resolve(&data_dir, &request.workspace_root)?;

    Ok(AppConfig {
        provider_id: provider.id.clone(),
        provider_label: provider.label.clone(),
        provider_kind: provider.kind.clone(),
        model: provider.model.clone(),
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        api_mode: provider.api_mode,
        skill_platform: "desktop".to_string(),
        workspace_root: request.workspace_root.clone(),
        data_dir,
        session_id: request.session_id.clone(),
        max_iterations: request.max_iterations.unwrap_or(12),
        system_prompt_override: request.system_prompt_override.clone(),
        tool_allowlist: None,
        enable_shell_tool: request.enable_shell_tool,
        debug_context: env_flag("HERMES_RS_DEBUG_CONTEXT"),
        enable_solve_trace_context: env_flag("HERMES_RS_ENABLE_SOLVE_TRACE_CONTEXT"),
        enable_meta_pattern_context: meta_pattern_context_enabled(),
        enable_experience_context: experience_context_enabled(),
        auxiliary_model,
        smart_model_routing,
        runtime_profile,
    })
}

fn build_session_config(request: &SessionCommandRequest) -> Result<AppConfig> {
    let data_dir = request
        .data_dir
        .clone()
        .unwrap_or_else(|| request.workspace_root.join(".hermes-agent-rs"));
    let provider = resolve_primary_provider(
        &data_dir,
        request.provider.clone(),
        request.model.clone(),
        request.base_url.clone(),
        request.api_key.clone(),
    );
    let auxiliary_model = resolve_auxiliary_model(
        &data_dir,
        &provider,
        request.aux_provider.clone(),
        request.aux_model.clone(),
        request.aux_base_url.clone(),
        request.aux_api_key.clone(),
    )
    .unwrap_or(None);
    let smart_model_routing = load_smart_model_routing(&data_dir).unwrap_or(None);
    let runtime_profile = RuntimeProfile::resolve(&data_dir, &request.workspace_root)?;

    Ok(AppConfig {
        provider_id: provider.id.clone(),
        provider_label: provider.label.clone(),
        provider_kind: provider.kind.clone(),
        model: provider.model.clone(),
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        api_mode: provider.api_mode,
        skill_platform: "desktop".to_string(),
        workspace_root: request.workspace_root.clone(),
        data_dir,
        session_id: Some(request.session_id.clone()),
        max_iterations: request.max_iterations.unwrap_or(12),
        system_prompt_override: request.system_prompt_override.clone(),
        tool_allowlist: None,
        enable_shell_tool: request.enable_shell_tool,
        debug_context: env_flag("HERMES_RS_DEBUG_CONTEXT"),
        enable_solve_trace_context: env_flag("HERMES_RS_ENABLE_SOLVE_TRACE_CONTEXT"),
        enable_meta_pattern_context: meta_pattern_context_enabled(),
        enable_experience_context: experience_context_enabled(),
        auxiliary_model,
        smart_model_routing,
        runtime_profile,
    })
}

fn build_cron_run_config(request: &RunCronJobRequest) -> Result<AppConfig> {
    let data_dir = request
        .data_dir
        .clone()
        .unwrap_or_else(|| request.workspace_root.join(".hermes-agent-rs"));
    let provider = resolve_primary_provider(
        &data_dir,
        request.provider.clone(),
        request.model.clone(),
        request.base_url.clone(),
        request.api_key.clone(),
    );
    let auxiliary_model = resolve_auxiliary_model(
        &data_dir,
        &provider,
        request.aux_provider.clone(),
        request.aux_model.clone(),
        request.aux_base_url.clone(),
        request.aux_api_key.clone(),
    )
    .unwrap_or(None);
    let smart_model_routing = load_smart_model_routing(&data_dir).unwrap_or(None);
    let runtime_profile = RuntimeProfile::resolve(&data_dir, &request.workspace_root)?;

    Ok(AppConfig {
        provider_id: provider.id.clone(),
        provider_label: provider.label.clone(),
        provider_kind: provider.kind.clone(),
        model: provider.model.clone(),
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        api_mode: provider.api_mode,
        skill_platform: "desktop".to_string(),
        workspace_root: request.workspace_root.clone(),
        data_dir,
        session_id: Some(format!("cron.{}.{}", request.job_id, unix_now_ms())),
        max_iterations: request.max_iterations.unwrap_or(12),
        system_prompt_override: request.system_prompt_override.clone(),
        tool_allowlist: None,
        enable_shell_tool: request.enable_shell_tool,
        debug_context: env_flag("HERMES_RS_DEBUG_CONTEXT"),
        enable_solve_trace_context: env_flag("HERMES_RS_ENABLE_SOLVE_TRACE_CONTEXT"),
        enable_meta_pattern_context: meta_pattern_context_enabled(),
        enable_experience_context: experience_context_enabled(),
        auxiliary_model,
        smart_model_routing,
        runtime_profile,
    })
}

fn build_retry_delegate_config(
    request: &RetryDelegateRunRequest,
    session_id: &str,
) -> Result<AppConfig> {
    let data_dir = request
        .data_dir
        .clone()
        .unwrap_or_else(|| request.workspace_root.join(".hermes-agent-rs"));
    let provider = resolve_primary_provider(
        &data_dir,
        request.provider.clone(),
        request.model.clone(),
        request.base_url.clone(),
        request.api_key.clone(),
    );
    let auxiliary_model = resolve_auxiliary_model(
        &data_dir,
        &provider,
        request.aux_provider.clone(),
        request.aux_model.clone(),
        request.aux_base_url.clone(),
        request.aux_api_key.clone(),
    )
    .unwrap_or(None);
    let smart_model_routing = load_smart_model_routing(&data_dir).unwrap_or(None);
    let runtime_profile = RuntimeProfile::resolve(&data_dir, &request.workspace_root)?;

    Ok(AppConfig {
        provider_id: provider.id.clone(),
        provider_label: provider.label.clone(),
        provider_kind: provider.kind.clone(),
        model: provider.model.clone(),
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        api_mode: provider.api_mode,
        skill_platform: "desktop".to_string(),
        workspace_root: request.workspace_root.clone(),
        data_dir,
        session_id: Some(session_id.to_string()),
        max_iterations: request.max_iterations.unwrap_or(12),
        system_prompt_override: request.system_prompt_override.clone(),
        tool_allowlist: previous_worker_tool_allowlist(
            &request.data_dir,
            &request.workspace_root,
            &request.run_id,
        ),
        enable_shell_tool: request.enable_shell_tool,
        debug_context: env_flag("HERMES_RS_DEBUG_CONTEXT"),
        enable_solve_trace_context: env_flag("HERMES_RS_ENABLE_SOLVE_TRACE_CONTEXT"),
        enable_meta_pattern_context: meta_pattern_context_enabled(),
        enable_experience_context: experience_context_enabled(),
        auxiliary_model,
        smart_model_routing,
        runtime_profile,
    })
}

fn previous_worker_tool_allowlist(
    data_dir: &Option<PathBuf>,
    workspace_root: &std::path::Path,
    run_id: &str,
) -> Option<Vec<String>> {
    let resolved_data_dir = data_dir
        .clone()
        .unwrap_or_else(|| workspace_root.join(".hermes-agent-rs"));
    load_delegate_run_record(&resolved_data_dir, run_id)
        .ok()
        .flatten()
        .and_then(|record| record.worker_task.map(|task| task.allowed_tools))
        .filter(|items| !items.is_empty())
}

fn resolve_primary_provider(
    data_dir: &std::path::Path,
    provider: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> crate::providers::ResolvedProviderConfig {
    resolve_runtime_provider(
        data_dir,
        ProviderResolutionRequest {
            provider,
            model,
            base_url,
            api_key,
        },
    )
    .unwrap_or_else(|_| {
        crate::providers::resolve_runtime_provider(data_dir, ProviderResolutionRequest::default())
            .expect("resolve default provider")
    })
}

fn run_status_for_response(response: &str) -> &'static str {
    if response == "approval_pending" {
        "awaiting_approval"
    } else {
        "completed"
    }
}

fn truncated_preview(value: &str) -> String {
    if value.chars().count() <= 240 {
        return value.to_string();
    }
    value.chars().take(240).collect::<String>() + "..."
}

fn unix_now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn is_local_endpoint(base_url: &str) -> bool {
    let normalized = base_url.trim().to_lowercase();
    normalized.contains("localhost")
        || normalized.contains("127.0.0.1")
        || normalized.contains("0.0.0.0")
        || normalized.contains("[::1]")
}

fn build_session_summary(session: StoredSession) -> BridgeSessionSummary {
    let user_turns = session
        .history
        .iter()
        .filter(|message| message.role == "user")
        .count();
    let assistant_turns = session
        .history
        .iter()
        .filter(|message| message.role == "assistant")
        .count();
    let last_user_message = session
        .history
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content_text());
    let last_assistant_message = session
        .history
        .iter()
        .rev()
        .find(|message| message.role == "assistant")
        .map(|message| message.content_text());
    let title = session.title.or_else(|| {
        session
            .history
            .iter()
            .find(|message| message.role == "user")
            .map(|message| derive_session_title(&message.content_text()))
    });

    BridgeSessionSummary {
        session_id: session.session_id,
        title,
        model: session.model,
        created_at_unix: session.created_at_unix,
        updated_at_unix: session.updated_at_unix,
        message_count: session.history.len(),
        user_turns,
        assistant_turns,
        last_user_message,
        last_assistant_message,
    }
}

fn derive_session_title(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 48 {
        return normalized;
    }
    let mut title = normalized.chars().take(47).collect::<String>();
    title.push('…');
    title
}

fn build_session_search_result(hit: SessionSearchHit) -> BridgeSessionSearchResult {
    BridgeSessionSearchResult {
        summary: build_session_summary(hit.session),
        score: hit.score,
        match_count: hit.match_count,
        matched_messages: hit.matched_messages,
        snippet: hit.snippet,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentBridge, BridgeEventEnvelope, BridgeEventSink, RecordingBridgeEventSink,
        ResolveProviderStatusRequest, RunAgentRequest, RunCronJobRequest, SessionCommandRequest,
    };
    use crate::approval::{ApprovalStatus, list_requests, load_pending_approval, resolve_request};
    use crate::events::AgentEvent;
    use crate::session::{SessionStore, SessionTimelineEntry, StoredSession, StoredToolPhase};
    use crate::skills::{SkillActivation, SkillStore};
    use crate::todo::{TodoItem, TodoStore};
    use std::sync::mpsc;

    struct CountingSink {
        count: usize,
        last: Option<BridgeEventEnvelope>,
    }

    impl BridgeEventSink for CountingSink {
        fn push(&mut self, event: BridgeEventEnvelope) {
            self.count += 1;
            self.last = Some(event);
        }
    }

    struct BlockingSink {
        started_tx: Option<mpsc::Sender<()>>,
        release_rx: mpsc::Receiver<()>,
    }

    impl BridgeEventSink for BlockingSink {
        fn push(&mut self, _event: BridgeEventEnvelope) {
            if let Some(started_tx) = self.started_tx.take() {
                let _ = started_tx.send(());
                let _ = self.release_rx.recv();
            }
        }
    }

    #[test]
    fn recording_sink_keeps_event_order() {
        let mut sink = RecordingBridgeEventSink::new();
        sink.push(BridgeEventEnvelope {
            seq: 1,
            event_type: "turn_started".to_string(),
            emitted_at_unix_ms: 1,
            event: AgentEvent::TurnStarted {
                session_id: "demo".to_string(),
                turn_id: "turn-1".to_string(),
                user_input_preview: "hello".to_string(),
                input_chars: 5,
                resumed: false,
            },
        });
        sink.push(BridgeEventEnvelope {
            seq: 2,
            event_type: "assistant_message".to_string(),
            emitted_at_unix_ms: 2,
            event: AgentEvent::AssistantMessage {
                session_id: "demo".to_string(),
                content: "world".to_string(),
            },
        });

        assert_eq!(sink.events().len(), 2);
        assert_eq!(sink.events()[0].seq, 1);
        assert_eq!(sink.events()[1].event_type, "assistant_message");
    }

    #[test]
    fn custom_sink_receives_bridge_events() {
        let mut sink = CountingSink {
            count: 0,
            last: None,
        };
        sink.push(BridgeEventEnvelope {
            seq: 7,
            event_type: "nudge".to_string(),
            emitted_at_unix_ms: 42,
            event: AgentEvent::Nudge {
                session_id: "demo".to_string(),
                kind: "skills".to_string(),
                message: "save it".to_string(),
            },
        });

        assert_eq!(sink.count, 1);
        assert_eq!(sink.last.expect("last").seq, 7);
    }

    #[test]
    fn bridge_lists_skills() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            "skills:\n  include_bundled: false\n",
        )
        .expect("write config");
        let store = SkillStore::new(tmp.path()).expect("store");
        store
            .save_with_metadata(
                "coding",
                "rust-review",
                "Review Rust code carefully.",
                &["rust".to_string(), "review".to_string()],
                &SkillActivation {
                    task_kinds: vec!["analysis".to_string()],
                    requires_tools: vec!["read_file".to_string()],
                    requires_shell: false,
                },
                "# Rust Review",
            )
            .expect("save");

        let skills = AgentBridge::list_skills(tmp.path().to_path_buf()).expect("skills");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "rust-review");
        assert_eq!(skills[0].task_kinds, vec!["analysis".to_string()]);
        assert_eq!(skills[0].requires_tools, vec!["read_file".to_string()]);
        assert!(!skills[0].requires_shell);
    }

    #[test]
    fn bridge_clears_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let response = AgentBridge::clear_session(SessionCommandRequest {
            workspace_root: tmp.path().to_path_buf(),
            data_dir: Some(tmp.path().join(".agent")),
            session_id: "demo".to_string(),
            provider: None,
            model: None,
            base_url: None,
            api_key: None,
            aux_provider: None,
            aux_model: None,
            aux_base_url: None,
            aux_api_key: None,
            max_iterations: None,
            system_prompt_override: None,
            enable_shell_tool: false,
        })
        .expect("clear");

        assert!(response.ok);
        assert_eq!(response.session_id, "demo");
    }

    #[test]
    fn bridge_lists_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path()).expect("store");
        let mut session = StoredSession::new("demo".to_string(), "gpt-4.1-mini".to_string());
        session
            .history
            .push(crate::types::ChatMessage::user("hello world"));
        session
            .history
            .push(crate::types::ChatMessage::system("ignore"));
        session.history.push(crate::types::ChatMessage {
            role: "assistant".to_string(),
            content: Some(serde_json::Value::String("hi".to_string())),
            tool_calls: None,
            tool_call_id: None,
        });
        store.save(&session).expect("save");

        let sessions = AgentBridge::list_sessions(tmp.path().to_path_buf()).expect("sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "demo");
        assert_eq!(sessions[0].title.as_deref(), Some("hello world"));
        assert_eq!(sessions[0].user_turns, 1);
        assert_eq!(sessions[0].assistant_turns, 1);
        assert_eq!(
            sessions[0].last_user_message.as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn bridge_loads_session_detail() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path()).expect("store");
        let todo_store = TodoStore::new(tmp.path()).expect("todo store");
        let mut session = StoredSession::new("demo".to_string(), "gpt-4.1-mini".to_string());
        session
            .history
            .push(crate::types::ChatMessage::user("hello"));
        store.save(&session).expect("save");
        todo_store
            .save(
                "demo",
                &[TodoItem::new("1", "Investigate issue", "in_progress")],
            )
            .expect("save todos");

        let loaded = AgentBridge::load_session(tmp.path().to_path_buf(), "demo".to_string())
            .expect("load")
            .expect("session");
        assert_eq!(loaded.summary.session_id, "demo");
        assert_eq!(loaded.summary.title.as_deref(), Some("hello"));
        assert_eq!(loaded.history.len(), 1);
        assert_eq!(loaded.todos.len(), 1);
        assert_eq!(loaded.todos[0].id, "1");
    }

    #[test]
    fn bridge_searches_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SessionStore::new(tmp.path()).expect("store");

        let mut session = StoredSession::new("debug-rust".to_string(), "gpt-4.1-mini".to_string());
        session.title = Some("Rust parser debugging".to_string());
        session.history.push(crate::types::ChatMessage::user(
            "Track down borrow checker failure",
        ));
        store.save(&session).expect("save");

        let results = AgentBridge::search_sessions(
            tmp.path().to_path_buf(),
            "borrow checker".to_string(),
            Some(5),
        )
        .expect("search");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].summary.session_id, "debug-rust");
        assert!(results[0].score > 0);
        assert!(results[0].snippet.contains("borrow"));
        assert_eq!(results[0].matched_messages, 1);
    }

    #[test]
    fn bridge_resolves_local_provider_status_without_auth() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let status = AgentBridge::resolve_provider_status(ResolveProviderStatusRequest {
            data_dir: tmp.path().to_path_buf(),
            provider: None,
            model: Some("qwen-coder".to_string()),
            base_url: Some("http://127.0.0.1:1234/v1".to_string()),
            api_key: None,
        })
        .expect("status");

        assert_eq!(status.id, "direct");
        assert!(!status.auth_required);
        assert!(status.ready);
    }

    #[test]
    fn bridge_requests_stop() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let response =
            AgentBridge::stop_session(tmp.path().to_path_buf(), "demo".to_string()).expect("stop");
        assert!(response.ok);
        assert_eq!(response.session_id, "demo");
    }

    #[tokio::test]
    async fn bridge_runs_cron_job_and_persists_status() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join(".data")).expect("mkdir");
        std::fs::write(
            tmp.path().join(".data").join("config.yaml"),
            r#"cron:
  jobs:
    - id: nightly-audit
      schedule: "0 2 * * *"
      prompt: "Audit the workspace."
"#,
        )
        .expect("write config");

        let result = AgentBridge::run_cron_job_with_event_sink(
            RunCronJobRequest {
                workspace_root: tmp.path().to_path_buf(),
                data_dir: Some(tmp.path().join(".data")),
                job_id: "nightly-audit".to_string(),
                provider: None,
                model: Some("test-model".to_string()),
                base_url: Some("mock://final-response".to_string()),
                api_key: None,
                aux_provider: None,
                aux_model: None,
                aux_base_url: None,
                aux_api_key: None,
                max_iterations: Some(4),
                system_prompt_override: None,
                enable_shell_tool: false,
            },
            &mut RecordingBridgeEventSink::new(),
        )
        .await
        .expect("run cron");

        assert_eq!(result.status, "completed");
        assert_eq!(result.response, "mock final response");

        let overview = crate::extensions::load_extensions_overview(&tmp.path().join(".data"))
            .expect("overview");
        assert_eq!(overview.cron_jobs.len(), 1);
        assert_eq!(overview.cron_jobs[0].id, "nightly-audit");
        assert_eq!(
            overview.cron_jobs[0].last_status.as_deref(),
            Some("completed")
        );
        assert!(overview.cron_jobs[0].last_session_id.is_some());
    }

    #[tokio::test]
    async fn bridge_inspects_mock_mcp_server() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            r#"mcp:
  servers:
    - name: docs
      command: __mock_mcp_server__
"#,
        )
        .expect("write config");

        let inspection =
            AgentBridge::inspect_mcp_server(tmp.path().to_path_buf(), "docs".to_string())
                .await
                .expect("inspection");
        assert_eq!(inspection.server.name, "docs");
        assert_eq!(inspection.tools.len(), 2);
        assert_eq!(inspection.tools[0].name, "search_docs");
    }

    #[tokio::test]
    async fn bridge_resumes_approved_command_after_approval() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("hello.txt"), "hello").expect("write");
        let data_dir = tmp.path().join(".data");
        let base_url = "mock://terminal-approval".to_string();
        let session_id = "approval-allowed".to_string();

        let mut first_sink = RecordingBridgeEventSink::new();
        let first = AgentBridge::run_with_event_sink(
            RunAgentRequest {
                prompt: "delete hello.txt".to_string(),
                provider: None,
                model: Some("test-model".to_string()),
                base_url: Some(base_url.clone()),
                api_key: None,
                workspace_root: tmp.path().to_path_buf(),
                data_dir: Some(data_dir.clone()),
                session_id: Some(session_id.clone()),
                max_iterations: Some(4),
                system_prompt_override: None,
                aux_provider: None,
                aux_model: None,
                aux_base_url: None,
                aux_api_key: None,
                enable_shell_tool: true,
            },
            &mut first_sink,
        )
        .await
        .expect("initial run");

        assert_eq!(first.status, "awaiting_approval");
        assert_eq!(first.response, "approval_pending");
        assert!(
            first_sink
                .events()
                .iter()
                .any(|item| item.event_type == "approval_required")
        );
        assert!(
            !first_sink
                .events()
                .iter()
                .any(|item| item.event_type == "tool_call_finished")
        );

        let approvals = list_requests(&data_dir).expect("approvals");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].status, ApprovalStatus::Pending);
        let approval_id = approvals[0].id.clone();
        assert!(
            load_pending_approval(&data_dir, &approval_id)
                .expect("pending")
                .is_some()
        );

        resolve_request(&data_dir, &approval_id, true).expect("approve");

        let mut resumed_sink = RecordingBridgeEventSink::new();
        let resumed = AgentBridge::resume_approval_with_event_sink(
            SessionCommandRequest {
                workspace_root: tmp.path().to_path_buf(),
                data_dir: Some(data_dir.clone()),
                session_id: session_id.clone(),
                provider: None,
                model: Some("test-model".to_string()),
                base_url: Some(base_url),
                api_key: None,
                aux_provider: None,
                aux_model: None,
                aux_base_url: None,
                aux_api_key: None,
                max_iterations: Some(4),
                system_prompt_override: None,
                enable_shell_tool: true,
            },
            approval_id.clone(),
            &mut resumed_sink,
        )
        .await
        .expect("resume");

        assert_eq!(resumed.status, "completed");
        assert_eq!(resumed.response, "command completed");
        assert!(!tmp.path().join("hello.txt").exists());
        assert!(
            load_pending_approval(&data_dir, &approval_id)
                .expect("pending removed")
                .is_none()
        );
        assert!(
            resumed_sink
                .events()
                .iter()
                .any(|item| item.event_type == "tool_call_finished")
        );
        assert!(
            resumed_sink
                .events()
                .iter()
                .any(|item| item.event_type == "assistant_message")
        );
    }

    #[tokio::test]
    async fn bridge_resumes_denied_command_without_running_shell() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("hello.txt"), "hello").expect("write");
        let data_dir = tmp.path().join(".data");
        let base_url = "mock://terminal-approval".to_string();
        let session_id = "approval-denied".to_string();

        let first = AgentBridge::run_with_event_sink(
            RunAgentRequest {
                prompt: "delete hello.txt".to_string(),
                provider: None,
                model: Some("test-model".to_string()),
                base_url: Some(base_url.clone()),
                api_key: None,
                workspace_root: tmp.path().to_path_buf(),
                data_dir: Some(data_dir.clone()),
                session_id: Some(session_id.clone()),
                max_iterations: Some(4),
                system_prompt_override: None,
                aux_provider: None,
                aux_model: None,
                aux_base_url: None,
                aux_api_key: None,
                enable_shell_tool: true,
            },
            &mut RecordingBridgeEventSink::new(),
        )
        .await
        .expect("initial run");
        assert_eq!(first.status, "awaiting_approval");

        let approval_id = list_requests(&data_dir)
            .expect("approvals")
            .into_iter()
            .next()
            .expect("approval")
            .id;
        resolve_request(&data_dir, &approval_id, false).expect("deny");

        let mut resumed_sink = RecordingBridgeEventSink::new();
        let resumed = AgentBridge::resume_approval_with_event_sink(
            SessionCommandRequest {
                workspace_root: tmp.path().to_path_buf(),
                data_dir: Some(data_dir.clone()),
                session_id: session_id.clone(),
                provider: None,
                model: Some("test-model".to_string()),
                base_url: Some(base_url),
                api_key: None,
                aux_provider: None,
                aux_model: None,
                aux_base_url: None,
                aux_api_key: None,
                max_iterations: Some(4),
                system_prompt_override: None,
                enable_shell_tool: true,
            },
            approval_id.clone(),
            &mut resumed_sink,
        )
        .await
        .expect("resume");

        assert_eq!(resumed.status, "completed");
        assert_eq!(resumed.response, "approval denied acknowledged");
        assert!(tmp.path().join("hello.txt").exists());
        assert!(
            load_pending_approval(&data_dir, &approval_id)
                .expect("pending removed")
                .is_none()
        );
        assert!(
            resumed_sink.events().iter().any(|item| matches!(
                &item.event,
                AgentEvent::ApprovalResolved {
                    approval_id: resolved_approval_id,
                    status,
                    approved,
                    ..
                } if resolved_approval_id == &approval_id && status == "denied" && !approved
            )),
            "denied approval should emit a resolved approval event"
        );
        assert!(
            resumed_sink.events().iter().any(|item| matches!(
                &item.event,
                AgentEvent::ToolCallFinished { status, .. } if status == "error"
            )),
            "denied approval should finish the resumed tool call with error status"
        );
        let stored = SessionStore::new(data_dir.clone())
            .expect("store")
            .load(&session_id)
            .expect("load session")
            .expect("session");
        assert!(stored.timeline.iter().any(|entry| matches!(
            entry,
            SessionTimelineEntry::Tool {
                phase: StoredToolPhase::Error,
                ..
            }
        )));
    }

    #[test]
    fn bridge_blocks_duplicate_run_for_same_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let request = RunAgentRequest {
            prompt: "say hello".to_string(),
            provider: None,
            model: Some("test-model".to_string()),
            base_url: Some("mock://final-response".to_string()),
            api_key: None,
            aux_provider: None,
            aux_model: None,
            aux_base_url: None,
            aux_api_key: None,
            workspace_root: tmp.path().to_path_buf(),
            data_dir: Some(tmp.path().join(".data")),
            session_id: Some("duplicate-run".to_string()),
            max_iterations: Some(4),
            system_prompt_override: None,
            enable_shell_tool: false,
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let first_request = request.clone();
        let first_run = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            let mut sink = BlockingSink {
                started_tx: Some(started_tx),
                release_rx,
            };
            runtime.block_on(AgentBridge::run_with_event_sink(first_request, &mut sink))
        });

        started_rx.recv().expect("first run started");

        let duplicate_error = {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            let mut sink = RecordingBridgeEventSink::new();
            runtime
                .block_on(AgentBridge::run_with_event_sink(request, &mut sink))
                .expect_err("duplicate run blocked")
        };
        assert!(duplicate_error.to_string().contains("正在执行中"));

        release_tx.send(()).expect("release first run");
        let first_result = first_run.join().expect("join").expect("first run result");
        assert_eq!(first_result.status, "completed");
    }

    #[test]
    fn bridge_blocks_duplicate_resume_for_same_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("hello.txt"), "hello").expect("write");
        let data_dir = tmp.path().join(".data");
        let session_id = "duplicate-resume".to_string();
        let approval_id = {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            let mut sink = RecordingBridgeEventSink::new();
            runtime
                .block_on(AgentBridge::run_with_event_sink(
                    RunAgentRequest {
                        prompt: "delete hello.txt".to_string(),
                        provider: None,
                        model: Some("test-model".to_string()),
                        base_url: Some("mock://terminal-approval".to_string()),
                        api_key: None,
                        aux_provider: None,
                        aux_model: None,
                        aux_base_url: None,
                        aux_api_key: None,
                        workspace_root: tmp.path().to_path_buf(),
                        data_dir: Some(data_dir.clone()),
                        session_id: Some(session_id.clone()),
                        max_iterations: Some(4),
                        system_prompt_override: None,
                        enable_shell_tool: true,
                    },
                    &mut sink,
                ))
                .expect("initial run");

            let approval_id = list_requests(&data_dir)
                .expect("approvals")
                .into_iter()
                .next()
                .expect("approval")
                .id;
            resolve_request(&data_dir, &approval_id, true).expect("approve");
            approval_id
        };

        let request = SessionCommandRequest {
            workspace_root: tmp.path().to_path_buf(),
            data_dir: Some(data_dir.clone()),
            session_id: session_id.clone(),
            provider: None,
            model: Some("test-model".to_string()),
            base_url: Some("mock://terminal-approval".to_string()),
            api_key: None,
            aux_provider: None,
            aux_model: None,
            aux_base_url: None,
            aux_api_key: None,
            max_iterations: Some(4),
            system_prompt_override: None,
            enable_shell_tool: true,
        };
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let first_request = request.clone();
        let first_approval_id = approval_id.clone();
        let first_resume = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            let mut sink = BlockingSink {
                started_tx: Some(started_tx),
                release_rx,
            };
            runtime.block_on(AgentBridge::resume_approval_with_event_sink(
                first_request,
                first_approval_id,
                &mut sink,
            ))
        });

        started_rx.recv().expect("first resume started");

        let duplicate_error = {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            let mut sink = RecordingBridgeEventSink::new();
            runtime
                .block_on(AgentBridge::resume_approval_with_event_sink(
                    request,
                    approval_id,
                    &mut sink,
                ))
                .expect_err("duplicate resume blocked")
        };
        assert!(duplicate_error.to_string().contains("正在执行中"));

        release_tx.send(()).expect("release first resume");
        let resumed = first_resume.join().expect("join").expect("resume result");
        assert_eq!(resumed.status, "completed");
        assert_eq!(resumed.response, "command completed");
        assert!(!tmp.path().join("hello.txt").exists());
    }
}
