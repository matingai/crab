use anyhow::{Result, bail};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{info, warn};
use uuid::Uuid;

use crate::approval::{
    ApprovalStatus, get_request, load_pending_approval, remove_pending_approval,
    save_pending_approval,
};
use crate::archive_db::ArchiveStore;
use crate::browser_state::BrowserStateStore;
use crate::config::AppConfig;
use crate::context_compression::{ContextCompressor, estimate_messages_tokens};
use crate::context_limit_cache::save_context_length;
use crate::events::{AgentEvent, EventHandler, NoopEventHandler, RecordingEventHandler};
use crate::experience_store::{ExperienceStore, derive_experience_from_episode};
use crate::goal_state::{
    CognitionItem, EvidenceItem, GoalItem, GoalState, GoalStateStore, HotDataItem,
};
use crate::llm::{ApiMode, ModelResponse, OpenAiCompatClient, RequestOptions};
use crate::memory_cards::build_memory_snapshot;
use crate::meta_pattern_store::{MetaPatternState, MetaPatternStore, MetaPatternStrategyTemplate};
use crate::privacy::{redact_chat_message_secrets, redact_secrets};
use crate::prompts::build_system_prompt;
use crate::request_recovery::{
    RetryableErrorKind, classify_retryable_error, is_context_overflow_error, jittered_backoff_ms,
    parse_available_output_tokens_from_error, parse_context_limit_from_error, parse_retry_after_ms,
};
use crate::runtime_control::{clear_stop_request, stop_requested};
use crate::session::{SessionStore, StoredBatchStatus, StoredSession, StoredToolPhase};
use crate::skill_advisor::{SkillAdviceInput, suggest_skill_lifecycle};
use crate::smart_model_routing::resolve_turn_route;
use crate::solve_trace::{SolveDecision, SolveOutcome, SolveStep, SolveTraceStore};
use crate::subdir_hints::SubdirectoryHintTracker;
use crate::title_generation::{generate_title, should_generate_title};
use crate::todo::{TodoItem, TodoStore};
use crate::tools::{
    ToolContext, ToolRegistry, ToolRuntimeEvent, clear_tool_event_sender,
    register_tool_event_sender, truncated, with_tool_runtime_scope,
};
use crate::types::ChatMessage;
use crate::wiki_store::WikiStore;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep, timeout};

pub struct Agent {
    config: AppConfig,
    client: OpenAiCompatClient,
    auxiliary_client: Option<OpenAiCompatClient>,
    tools: ToolRegistry,
    tool_context: ToolContext,
    session_store: SessionStore,
    session: StoredSession,
    archive_store: ArchiveStore,
    browser_state_store: BrowserStateStore,
    todo_store: TodoStore,
    goal_state_store: GoalStateStore,
    experience_store: ExperienceStore,
    meta_pattern_store: MetaPatternStore,
    solve_trace_store: SolveTraceStore,
    wiki_store: WikiStore,
    subdir_hint_tracker: SubdirectoryHintTracker,
    context_compressor: ContextCompressor,
    ephemeral_max_output_tokens: Option<usize>,
    active_turn_id: Option<String>,
}

#[derive(Default)]
struct TurnProgress {
    turn_tool_calls: usize,
    skill_manage_used: bool,
    turn_tool_names: Vec<String>,
    turn_skill_matches: Vec<crate::skills::SkillMatch>,
}

#[derive(Default)]
struct LiveToolOutputState {
    stdout: String,
    stderr: String,
}

struct ContextInjection {
    label: &'static str,
    content: String,
    max_chars: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextPreviewInjection {
    pub label: String,
    pub content: String,
    pub original_chars: usize,
    pub max_chars: usize,
}

#[derive(Default)]
struct ContextInjectionStats {
    total_blocks: usize,
    kept_blocks: usize,
    original_chars: usize,
    final_chars: usize,
    clipped_labels: Vec<&'static str>,
    skipped_labels: Vec<&'static str>,
}

impl ContextInjectionStats {
    fn was_trimmed(&self) -> bool {
        !self.clipped_labels.is_empty() || !self.skipped_labels.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextPreviewStats {
    pub total_blocks: usize,
    pub kept_blocks: usize,
    pub original_chars: usize,
    pub final_chars: usize,
    pub clipped_labels: Vec<String>,
    pub skipped_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextPreview {
    pub session_id: String,
    pub user_input: String,
    pub projected_tokens: usize,
    pub request_budget_tokens: usize,
    pub tool_definition_count: usize,
    pub matched_skills: Vec<String>,
    pub messages: Vec<ChatMessage>,
    pub injections: Vec<ContextPreviewInjection>,
    pub injection_stats: ContextPreviewStats,
    pub events: Vec<AgentEvent>,
}

#[derive(Clone)]
struct TurnModelRuntime {
    client: OpenAiCompatClient,
    model: String,
    base_url: String,
    api_mode: ApiMode,
    routed_label: Option<String>,
    uses_primary: bool,
}

const APPROVAL_PENDING_RESPONSE: &str = "approval_pending";
const GOAL_STATE_COMPACTION_PROMPT: &str = "You are compressing an agent's goal-state working memory for the next action. Keep only the most decision-relevant information for the active goal. Do not infer new facts. Prefer explicit constraints, blockers, verified facts, recent tool observations, and the next actionable context. Return strict JSON with this shape: {\"cognition\":\"...\",\"hot_data\":\"...\",\"confidence\":0.0}. The `cognition` field should summarize the durable beliefs and open uncertainties. The `hot_data` field should summarize the freshest task-relevant runtime facts. Keep each field under 240 characters.";
const GOAL_STATE_RECONCILE_PROMPT: &str = "You are reconciling an agent's goal-state after a tool outcome. Return only justified updates supported by the current goal state and the tool result. Do not invent new facts. Prefer updating mission, phase, focus, reflection, goal status, confidence, and explicit cognition/risk/hot-data items when the tool outcome materially changes them. Each returned goal, cognition, or hot-data item may include `evidence` and `evidence_items`, where `evidence_items` is an array of objects with shape {\"source_type\":\"tool_output\",\"source\":\"...\",\"summary\":\"...\",\"relation\":\"supports\",\"observed_at_unix\":123}. Return strict JSON with this shape: {\"mission\":\"...\",\"phase\":\"act\",\"current_focus_goal_id\":\"...\",\"reflection\":\"...\",\"goals\":[{\"id\":\"...\",\"title\":\"...\",\"level\":\"current\",\"status\":\"blocked\",\"confidence\":0.5,\"parent_id\":\"...\",\"summary\":\"...\",\"evidence\":\"...\",\"evidence_items\":[...] }],\"cognition\":[{\"id\":\"...\",\"kind\":\"risk\",\"content\":\"...\",\"confidence\":0.5,\"evidence\":\"...\",\"evidence_items\":[...]}],\"hot_data\":[{\"id\":\"...\",\"content\":\"...\",\"confidence\":0.5,\"source\":\"...\",\"goal_id\":\"...\",\"expires_at_unix\":123,\"evidence_items\":[...]}]}. Every returned item must be complete enough to store directly. Use existing ids when updating existing items. Omit fields and items that should not change.";
const GOAL_STATE_TURN_RECONCILE_PROMPT: &str = "You are reconciling an agent's goal-state at the end of a turn. Use the final assistant response, the current turn transcript, and the existing goal-state to produce only justified updates. Do not invent new facts. Prefer deciding whether the focus goal remains in progress, is blocked, or has succeeded, whether the phase should change, and whether reflection should capture a strategy adjustment. Each returned goal, cognition, or hot-data item may include `evidence` and `evidence_items`, where `evidence_items` is an array of objects with shape {\"source_type\":\"assistant_response\",\"source\":\"...\",\"summary\":\"...\",\"relation\":\"supports\",\"observed_at_unix\":123}. Return strict JSON with this shape: {\"mission\":\"...\",\"phase\":\"verify\",\"current_focus_goal_id\":\"...\",\"reflection\":\"...\",\"goals\":[{\"id\":\"...\",\"title\":\"...\",\"level\":\"current\",\"status\":\"succeeded\",\"confidence\":0.5,\"parent_id\":\"...\",\"summary\":\"...\",\"evidence\":\"...\",\"evidence_items\":[...]}],\"cognition\":[{\"id\":\"...\",\"kind\":\"decision\",\"content\":\"...\",\"confidence\":0.5,\"evidence\":\"...\",\"evidence_items\":[...]}],\"hot_data\":[{\"id\":\"...\",\"content\":\"...\",\"confidence\":0.5,\"source\":\"...\",\"goal_id\":\"...\",\"expires_at_unix\":123,\"evidence_items\":[...]}]}. Use existing ids when updating existing items. Omit anything that should not change.";
const GOAL_STATE_EXECUTION_BRIEF_PROMPT: &str = "You are distilling an agent's current goal-state into a short execution brief for the foreground model. Do not invent facts. Prefer the active focus, the best immediate next step, the main blocker or risk, and one operating rule that keeps work aligned to the goal. Return strict JSON with this shape: {\"focus\":\"...\",\"next_step\":\"...\",\"watch\":\"...\",\"operating_rule\":\"...\",\"confidence\":0.0}. Keep each text field under 180 characters. Use an empty string when a field is not justified by the provided state.";
const GOAL_STATE_DELTA_PROMPT: &str = "You are proposing a small, justified state update for an agent's goal-state before the next foreground action. Use only the provided goal-state and latest user input. Do not invent facts. Return strict JSON with this shape: {\"mission\":\"...\",\"phase\":\"...\",\"current_focus_goal_id\":\"...\",\"reflection\":\"...\",\"rationale\":\"...\"}. `phase` must be one of understand/investigate/act/verify/finalize or empty. Leave fields empty when no update is justified. Keep each text field under 180 characters.";
const TOOL_RESULT_SUMMARY_PROMPT: &str = "You are compressing a tool result for goal-state maintenance. Extract only the decision-relevant signal that a goal-state reconcile step needs. Do not invent facts. Return strict JSON with this shape: {\"summary\":\"...\",\"key_evidence\":[\"...\"],\"candidate_hot_data\":[\"...\"],\"candidate_risks\":[\"...\"]}. Keep `summary` under 180 characters. Keep each list item under 120 characters. Omit empty lists.";
const GOAL_STATE_COMPACTION_ITEM_LIMIT: usize = 12;
const GOAL_STATE_COMPACTION_INPUT_CHARS: usize = 6_000;
const GOAL_STATE_COMPACTION_OUTPUT_CHARS: usize = 240;
const GOAL_STATE_COMPACTION_TIMEOUT_SECS: u64 = 4;
const GOAL_STATE_EXECUTION_BRIEF_INPUT_CHARS: usize = 4_000;
const GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS: usize = 180;
const GOAL_STATE_EXECUTION_BRIEF_TIMEOUT_SECS: u64 = 4;
const GOAL_STATE_DELTA_INPUT_CHARS: usize = 4_000;
const GOAL_STATE_DELTA_OUTPUT_CHARS: usize = 180;
const GOAL_STATE_DELTA_TIMEOUT_SECS: u64 = 4;
const GOAL_STATE_RECONCILE_INPUT_CHARS: usize = 8_000;
const GOAL_STATE_RECONCILE_TIMEOUT_SECS: u64 = 4;
const GOAL_STATE_TURN_RECONCILE_INPUT_CHARS: usize = 9_000;
const GOAL_STATE_TURN_RECONCILE_TIMEOUT_SECS: u64 = 4;
const TOOL_RESULT_SUMMARY_INPUT_CHARS: usize = 6_000;
const TOOL_RESULT_SUMMARY_TIMEOUT_SECS: u64 = 4;
const META_PATTERN_SUMMARY_PROMPT: &str = "You are refining aggregated agent meta-patterns into concise reusable strategy templates. Given structured pattern data, rewrite each pattern into a short `model_summary`, 2-4 `match_hints`, and a `strategy_template` object. Do not invent facts beyond the provided pattern. Return strict JSON with this shape: {\"patterns\":[{\"id\":\"...\",\"model_summary\":\"...\",\"match_hints\":[\"...\"],\"strategy_template\":{\"applicable_when\":[\"...\"],\"preferred_actions\":[\"...\"],\"avoid\":[\"...\"],\"escalate_when\":[\"...\"]},\"confidence\":0.0}]}. Keep summaries under 180 characters and each template item under 90 characters.";
const META_PATTERN_SUMMARY_TIMEOUT_SECS: u64 = 4;
const CONTEXT_INJECTION_TOTAL_BUDGET_CHARS: usize = 12_000;
const REDUCED_CONTEXT_INJECTION_TOTAL_BUDGET_CHARS: usize = 4_000;
const CONTEXT_INJECTION_MIN_BLOCK_CHARS: usize = 180;
const PROMPT_HISTORY_MAX_USER_TURNS: usize = 5;
const MEMORY_SNAPSHOT_BUDGET_CHARS: usize = 2_400;
const EXECUTION_BRIEF_BUDGET_CHARS: usize = 700;
const STATE_DELTA_BUDGET_CHARS: usize = 700;
const GOAL_STATE_BUDGET_CHARS: usize = 1_800;
const SOLVE_TRACE_BUDGET_CHARS: usize = 1_800;
const META_PATTERN_BUDGET_CHARS: usize = 2_200;
const EXPERIENCE_BUDGET_CHARS: usize = 1_600;
const TODO_BUDGET_CHARS: usize = 1_200;
const PLUGIN_CONTEXT_BUDGET_CHARS: usize = 1_500;
const REQUEST_CONTEXT_BUDGET_NUMERATOR: usize = 4;
const REQUEST_CONTEXT_BUDGET_DENOMINATOR: usize = 5;

#[derive(Debug, Deserialize)]
struct GoalStateCompactionResponse {
    cognition: String,
    hot_data: String,
    #[serde(default)]
    confidence: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
struct GoalStateExecutionBriefResponse {
    #[serde(default)]
    focus: String,
    #[serde(default)]
    next_step: String,
    #[serde(default)]
    watch: String,
    #[serde(default)]
    operating_rule: String,
    #[serde(default)]
    confidence: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
struct GoalStateDeltaResponse {
    #[serde(default)]
    mission: String,
    #[serde(default)]
    phase: String,
    #[serde(default)]
    current_focus_goal_id: String,
    #[serde(default)]
    reflection: String,
    #[serde(default)]
    rationale: String,
}

#[derive(Debug, Default, Deserialize)]
struct GoalStateReconcileResponse {
    #[serde(default)]
    mission: Option<String>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    current_focus_goal_id: Option<String>,
    #[serde(default)]
    reflection: Option<String>,
    #[serde(default)]
    goals: Vec<GoalStateGoalPatch>,
    #[serde(default)]
    cognition: Vec<GoalStateCognitionPatch>,
    #[serde(default)]
    hot_data: Vec<GoalStateHotDataPatch>,
}

#[derive(Debug, Default, Deserialize)]
struct ToolResultSummaryResponse {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    key_evidence: Vec<String>,
    #[serde(default)]
    candidate_hot_data: Vec<String>,
    #[serde(default)]
    candidate_risks: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct GoalStateGoalPatch {
    id: String,
    title: Option<String>,
    level: Option<String>,
    status: Option<String>,
    confidence: Option<f32>,
    parent_id: Option<String>,
    summary: Option<String>,
    evidence: Option<String>,
    evidence_items: Option<Vec<GoalStateEvidencePatch>>,
}

#[derive(Debug, Default, Deserialize)]
struct GoalStateCognitionPatch {
    id: String,
    kind: Option<String>,
    content: Option<String>,
    confidence: Option<f32>,
    evidence: Option<String>,
    evidence_items: Option<Vec<GoalStateEvidencePatch>>,
}

#[derive(Debug, Default, Deserialize)]
struct GoalStateHotDataPatch {
    id: String,
    content: Option<String>,
    confidence: Option<f32>,
    source: Option<String>,
    goal_id: Option<String>,
    expires_at_unix: Option<u64>,
    evidence_items: Option<Vec<GoalStateEvidencePatch>>,
}

#[derive(Debug, Default, Deserialize)]
struct GoalStateEvidencePatch {
    source_type: Option<String>,
    source: Option<String>,
    summary: Option<String>,
    relation: Option<String>,
    observed_at_unix: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct MetaPatternSummaryResponse {
    #[serde(default)]
    patterns: Vec<MetaPatternSummaryPatch>,
}

#[derive(Debug, Default, Deserialize)]
struct MetaPatternSummaryPatch {
    id: String,
    #[serde(default)]
    model_summary: Option<String>,
    #[serde(default)]
    match_hints: Vec<String>,
    #[serde(default)]
    strategy_template: Option<MetaPatternStrategyTemplatePatch>,
    #[serde(default)]
    confidence: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
struct MetaPatternStrategyTemplatePatch {
    #[serde(default)]
    applicable_when: Vec<String>,
    #[serde(default)]
    preferred_actions: Vec<String>,
    #[serde(default)]
    avoid: Vec<String>,
    #[serde(default)]
    escalate_when: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DelegateWorkerToolResult {
    status: Option<String>,
    objective: Option<String>,
    focus_goal_id: Option<String>,
    worker_result: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DelegateWorkerResultPayload {
    summary: Option<String>,
    #[serde(default)]
    key_evidence: Vec<String>,
    #[serde(default)]
    candidate_beliefs: Vec<String>,
    #[serde(default)]
    candidate_risks: Vec<String>,
    #[serde(default)]
    step_updates: Vec<DelegateWorkerStepUpdate>,
    #[serde(default)]
    recommended_next_actions: Vec<String>,
    #[serde(default)]
    raw_refs: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DelegateWorkerStepUpdate {
    step_id: Option<String>,
    status: Option<String>,
    summary: Option<String>,
}

#[derive(Debug)]
struct DelegateWorkerObservation {
    tool_status: Option<String>,
    objective: String,
    focus_goal_id: Option<String>,
    payload: DelegateWorkerResultPayload,
}

impl Agent {
    pub fn new(config: AppConfig) -> Result<Self> {
        let client = OpenAiCompatClient::new(
            config.base_url.clone(),
            config.api_key.clone(),
            config.api_mode,
        )?;
        let auxiliary_client = config
            .auxiliary_model
            .as_ref()
            .map(|auxiliary| {
                OpenAiCompatClient::new(
                    auxiliary.base_url.clone(),
                    auxiliary.api_key.clone(),
                    auxiliary.api_mode,
                )
            })
            .transpose()?;
        let tools = ToolRegistry::hermes_default_with_allowlist(
            &config.data_dir,
            config.tool_allowlist.as_deref(),
        );
        let workspace_root = config.workspace_root.clone();
        let context_compressor = ContextCompressor::for_runtime_with_data_dir(
            &config.data_dir,
            &config.model,
            &config.provider_kind,
            &config.base_url,
        );
        let archive_store = ArchiveStore::new(config.data_dir.clone())?;
        let session_store = SessionStore::new(config.data_dir.clone())?;
        let browser_state_store = BrowserStateStore::new(config.data_dir.clone())?;
        let todo_store = TodoStore::new(config.data_dir.clone())?;
        let goal_state_store = GoalStateStore::new(config.data_dir.clone())?;
        let experience_store = ExperienceStore::new(config.data_dir.clone())?;
        let meta_pattern_store = MetaPatternStore::new(config.data_dir.clone())?;
        let solve_trace_store = SolveTraceStore::new(config.data_dir.clone())?;
        let wiki_store = WikiStore::new(config.data_dir.clone())?;
        let session = match config.session_id.clone() {
            Some(session_id) => session_store
                .load(&session_id)?
                .unwrap_or_else(|| StoredSession::new(session_id, config.model.clone())),
            None => StoredSession::new(Uuid::new_v4().to_string(), config.model.clone()),
        };
        let tool_context = ToolContext {
            workspace_root: workspace_root.clone(),
            data_dir: config.data_dir.clone(),
            shell_enabled: config.enable_shell_tool,
            skill_platform: config.skill_platform.clone(),
            provider_id: config.provider_id.clone(),
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            api_key: config.api_key.clone(),
            max_iterations: config.max_iterations,
            current_session_id: session.session_id.clone(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        };
        archive_store.upsert_session(
            &session.session_id,
            session.title.as_deref(),
            &config.workspace_root,
            &config.provider_id,
            &config.model,
            session.created_at_unix,
            session.updated_at_unix,
        )?;
        match session_store.list() {
            Ok(sessions) => match archive_store.backfill_sessions(
                &sessions,
                &config.workspace_root,
                &config.provider_id,
            ) {
                Ok(imported) if imported > 0 => {
                    info!("backfilled {imported} stored sessions into archive.db");
                }
                Ok(_) => {}
                Err(error) => {
                    warn!("failed to backfill stored sessions into archive.db: {error:#}");
                }
            },
            Err(error) => {
                warn!("failed to list stored sessions for archive backfill: {error:#}");
            }
        }

        Ok(Self {
            config,
            client,
            auxiliary_client,
            tools,
            tool_context,
            session_store,
            session,
            archive_store,
            browser_state_store,
            todo_store,
            goal_state_store,
            experience_store,
            meta_pattern_store,
            solve_trace_store,
            wiki_store,
            subdir_hint_tracker: SubdirectoryHintTracker::new(workspace_root),
            context_compressor,
            ephemeral_max_output_tokens: None,
            active_turn_id: None,
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session.session_id
    }

    pub fn set_delegate_depth(&mut self, depth: usize) {
        self.tool_context.delegate_depth = depth;
    }

    pub fn set_delegate_run_id(&mut self, delegate_run_id: Option<String>) {
        self.tool_context.current_delegate_run_id = delegate_run_id;
    }

    pub fn clear_history(&mut self) -> Result<()> {
        self.session.history.clear();
        self.session.clear_timeline();
        self.active_turn_id = None;
        self.browser_state_store.clear(&self.session.session_id)?;
        self.todo_store.clear(&self.session.session_id)?;
        self.goal_state_store.clear(&self.session.session_id)?;
        self.experience_store.clear(&self.session.session_id)?;
        self.meta_pattern_store.clear(&self.session.session_id)?;
        self.solve_trace_store.clear(&self.session.session_id)?;
        self.persist_session().map(|_| ())
    }

    fn active_archive_turn_id(&self) -> String {
        self.active_turn_id
            .clone()
            .unwrap_or_else(|| self.session.current_turn_id())
    }

    fn upsert_archive_session(&self) {
        if let Err(error) = self.archive_store.upsert_session(
            &self.session.session_id,
            self.session.title.as_deref(),
            &self.config.workspace_root,
            &self.config.provider_id,
            &self.config.model,
            self.session.created_at_unix,
            self.session.updated_at_unix,
        ) {
            warn!(
                "failed to persist archive session {}: {error:#}",
                self.session.session_id
            );
        }
    }

    fn sync_wiki_views(&self) {
        let todos = match self.todo_store.load(&self.session.session_id) {
            Ok(items) => items,
            Err(error) => {
                warn!(
                    "failed to load todos for wiki sync in session {}: {error:#}",
                    self.session.session_id
                );
                Vec::new()
            }
        };
        if let Err(error) = self.wiki_store.write_session_page(
            &self.session,
            &todos,
            &self.config.workspace_root,
            &self.config.provider_id,
        ) {
            warn!(
                "failed to write session wiki page for {}: {error:#}",
                self.session.session_id
            );
        }

        match self.session_store.list() {
            Ok(sessions) => {
                if let Err(error) = self.wiki_store.write_topic_pages(&sessions) {
                    warn!("failed to write topic wiki pages: {error:#}");
                }
                if let Err(error) = self
                    .wiki_store
                    .write_user_timeline(&sessions, Some(&self.session.session_id))
                {
                    warn!("failed to write user timeline wiki page: {error:#}");
                }
            }
            Err(error) => {
                warn!("failed to list sessions for wiki timeline sync: {error:#}");
            }
        }
    }

    fn seed_goal_state_from_user_input(&self, user_input: &str) -> Result<()> {
        let mut state = self.goal_state_store.load(&self.session.session_id)?;
        if state.goals.is_empty() {
            state.goals.push(GoalItem {
                id: "goal-current".to_string(),
                title: inline_clip(user_input, 120),
                level: "current".to_string(),
                status: "in_progress".to_string(),
                confidence: 0.65,
                parent_id: None,
                summary: inline_clip(user_input, 200),
                evidence: "Seeded from the latest user request.".to_string(),
                evidence_items: vec![evidence_item(
                    "user_input",
                    "current_turn",
                    "Seeded from the latest user request.",
                    "supports",
                )],
                updated_at_unix: unix_now_secs(),
            });
            state.current_focus_goal_id = Some("goal-current".to_string());
        }

        merge_cognition(
            &mut state.cognition,
            CognitionItem {
                id: "latest-user-request".to_string(),
                kind: "fact".to_string(),
                content: user_input.trim().to_string(),
                confidence: 1.0,
                evidence: "Explicit user input in the current turn.".to_string(),
                evidence_items: vec![evidence_item(
                    "user_input",
                    "current_turn",
                    "Explicit user input in the current turn.",
                    "supports",
                )],
                updated_at_unix: unix_now_secs(),
            },
        );

        merge_hot_data(
            &mut state.hot_data,
            HotDataItem {
                id: "latest-user-request".to_string(),
                content: inline_clip(user_input, 220),
                confidence: 1.0,
                source: "user".to_string(),
                goal_id: state.current_focus_goal_id.clone(),
                expires_at_unix: None,
                evidence_items: vec![evidence_item(
                    "user_input",
                    "current_turn",
                    "The latest user request should stay hot for immediate recall.",
                    "supports",
                )],
                updated_at_unix: unix_now_secs(),
            },
        );

        let _ = self
            .goal_state_store
            .replace(&self.session.session_id, state)?;
        Ok(())
    }

    fn start_solve_episode(&self, user_input: &str) {
        let turn_id = self.active_archive_turn_id();
        let focus = self
            .goal_state_store
            .load(&self.session.session_id)
            .ok()
            .and_then(|state| {
                select_focus_goal(&state).map(|goal| {
                    (
                        goal.id.clone(),
                        goal.title.clone(),
                        if goal.summary.trim().is_empty() {
                            None
                        } else {
                            Some(goal.summary.clone())
                        },
                    )
                })
            });
        let goal = focus
            .as_ref()
            .map(|(_, title, _)| title.clone())
            .unwrap_or_else(|| inline_clip(user_input, 120));
        let _ = self.solve_trace_store.start_episode(
            &self.session.session_id,
            &turn_id,
            &turn_id,
            &goal,
            user_input,
            focus.as_ref().map(|(id, _, _)| id.as_str()),
            focus.as_ref().map(|(_, title, _)| title.as_str()),
        );
        if let Some((_, _, Some(summary))) = focus {
            let _ = self.solve_trace_store.append_supplements(
                &self.session.session_id,
                &turn_id,
                &[format!(
                    "Initial focus summary: {}",
                    inline_clip(&summary, 180)
                )],
            );
        }
    }

    fn sync_goal_runtime_state(&self) {
        let Ok(mut state) = self.goal_state_store.load(&self.session.session_id) else {
            return;
        };
        if state.goals.is_empty() && state.cognition.is_empty() && state.hot_data.is_empty() {
            return;
        }

        let Ok(mut todos) = self.todo_store.load(&self.session.session_id) else {
            return;
        };
        sync_todos_from_goal_state(&state, &mut todos);
        if todos.is_empty() {
            let _ = self.todo_store.clear(&self.session.session_id);
        } else {
            let _ = self.todo_store.save(&self.session.session_id, &todos);
        }

        if let Some(summary) = summarize_active_todos_for_goal_loop(&todos) {
            merge_hot_data(
                &mut state.hot_data,
                HotDataItem {
                    id: "active-todo-summary".to_string(),
                    content: summary,
                    confidence: 0.9,
                    source: "todo_store".to_string(),
                    goal_id: state.current_focus_goal_id.clone(),
                    expires_at_unix: None,
                    evidence_items: vec![evidence_item(
                        "system_state",
                        "todo_store",
                        "Derived from active execution todos in the current session.",
                        "context",
                    )],
                    updated_at_unix: unix_now_secs(),
                },
            );
        }

        let _ = self
            .goal_state_store
            .replace(&self.session.session_id, state);
    }

    fn update_goal_state_from_tool_outcome(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        result: &str,
        execution_mode: &str,
        status: &str,
    ) {
        let Ok(mut state) = self.goal_state_store.load(&self.session.session_id) else {
            return;
        };

        let summary = summarize_tool_result(tool_name, result, status);
        let confidence = tool_observation_confidence(result, status);
        let evidence_summary = format!(
            "Observed via tool `{}` in {} mode with status `{}`.",
            tool_name, execution_mode, status
        );
        merge_cognition(
            &mut state.cognition,
            CognitionItem {
                id: format!("tool-observation:{tool_call_id}"),
                kind: classify_tool_cognition_kind(result, status).to_string(),
                content: summary.clone(),
                confidence,
                evidence: evidence_summary.clone(),
                evidence_items: vec![evidence_item(
                    "tool_output",
                    &format!("{execution_mode}:{tool_name}"),
                    &evidence_summary,
                    if status == "error" {
                        "conflicts"
                    } else {
                        "supports"
                    },
                )],
                updated_at_unix: unix_now_secs(),
            },
        );
        merge_hot_data(
            &mut state.hot_data,
            HotDataItem {
                id: format!("tool-hot:{tool_call_id}"),
                content: summary,
                confidence,
                source: format!("tool:{tool_name}"),
                goal_id: state.current_focus_goal_id.clone(),
                expires_at_unix: Some(unix_now_secs() + 6 * 3600),
                evidence_items: vec![evidence_item(
                    "tool_output",
                    &format!("{execution_mode}:{tool_name}"),
                    &evidence_summary,
                    if status == "error" {
                        "conflicts"
                    } else {
                        "supports"
                    },
                )],
                updated_at_unix: unix_now_secs(),
            },
        );
        if tool_name == "delegate_to_worker" {
            apply_delegate_worker_observation(
                &mut state,
                tool_call_id,
                result,
                execution_mode,
                status,
            );
        }

        let _ = self
            .goal_state_store
            .replace(&self.session.session_id, state);
        if tool_name == "delegate_to_worker" {
            self.update_delegate_worker_todos_from_tool_outcome(tool_call_id, result);
            self.record_delegate_worker_solve_trace(tool_call_id, result, status);
        } else {
            self.record_tool_solve_trace(tool_call_id, tool_name, result, status);
        }
    }

    async fn reconcile_goal_state_after_tool_outcome(
        &self,
        handler: &mut dyn EventHandler,
        tool_call_id: &str,
        tool_name: &str,
        result: &str,
        execution_mode: &str,
        status: &str,
    ) -> Option<String> {
        let Ok(mut state) = self.goal_state_store.load(&self.session.session_id) else {
            return None;
        };
        if state.goals.is_empty() && state.cognition.is_empty() && state.hot_data.is_empty() {
            return None;
        }
        if !should_reconcile_goal_state_after_tool(tool_name, result, status) {
            return None;
        }

        let used_summary = should_summarize_tool_result_for_reconcile(tool_name, result);
        let reconcile_result = if used_summary {
            self.summarize_tool_result_for_goal_reconcile(
                handler,
                tool_name,
                result,
                execution_mode,
                status,
            )
            .await
            .unwrap_or_else(|| result.to_string())
        } else {
            result.to_string()
        };

        let serialized = serialize_goal_state_reconcile_input(
            &state,
            tool_call_id,
            tool_name,
            &reconcile_result,
            execution_mode,
            status,
        );
        if serialized.is_empty() {
            return None;
        }

        let messages = vec![
            ChatMessage::system(GOAL_STATE_RECONCILE_PROMPT),
            ChatMessage::user(serialized),
        ];
        let Some(message) = self
            .request_background_message(
                handler,
                "goal_state_reconcile",
                messages,
                Duration::from_secs(GOAL_STATE_RECONCILE_TIMEOUT_SECS),
            )
            .await
        else {
            return used_summary.then_some(reconcile_result);
        };
        let Some(patch) = parse_goal_state_reconcile_response(&message.content_text()) else {
            return used_summary.then_some(reconcile_result);
        };

        apply_goal_state_reconcile_patch(&mut state, patch);
        let _ = self
            .goal_state_store
            .replace(&self.session.session_id, state);
        used_summary.then_some(reconcile_result)
    }

    async fn summarize_tool_result_for_goal_reconcile(
        &self,
        handler: &mut dyn EventHandler,
        tool_name: &str,
        result: &str,
        execution_mode: &str,
        status: &str,
    ) -> Option<String> {
        let messages = vec![
            ChatMessage::system(TOOL_RESULT_SUMMARY_PROMPT),
            ChatMessage::user(serialize_tool_result_summary_input(
                tool_name,
                result,
                execution_mode,
                status,
            )),
        ];
        let message = self
            .request_background_message(
                handler,
                "tool_result_summary",
                messages,
                Duration::from_secs(TOOL_RESULT_SUMMARY_TIMEOUT_SECS),
            )
            .await?;
        let summary = parse_tool_result_summary_response(&message.content_text())?;
        render_tool_result_summary_for_reconcile(&summary, tool_name, status)
    }

    fn update_delegate_worker_todos_from_tool_outcome(&self, tool_call_id: &str, result: &str) {
        let Some(observation) = parse_delegate_worker_observation(result) else {
            return;
        };
        if observation.payload.step_updates.is_empty() {
            return;
        }

        let Ok(mut todos) = self.todo_store.load(&self.session.session_id) else {
            return;
        };
        sync_todos_from_delegate_worker(tool_call_id, &observation, &mut todos);
        let _ = self.todo_store.save(&self.session.session_id, &todos);
    }

    fn record_turn_solve_outcome(&self, assistant_response: &str) {
        let episode_id = self.active_archive_turn_id();
        let focus = self
            .goal_state_store
            .load(&self.session.session_id)
            .ok()
            .and_then(|state| {
                select_focus_goal(&state).map(|goal| {
                    (
                        goal.status.clone(),
                        goal.title.clone(),
                        goal.summary.clone(),
                    )
                })
            });
        let outcome = SolveOutcome {
            status: focus
                .as_ref()
                .map(|(status, _, _)| map_goal_status_to_episode_status(status).to_string())
                .unwrap_or_else(|| "completed".to_string()),
            summary: if let Some((_, _, summary)) = &focus {
                if !summary.trim().is_empty() {
                    inline_clip(summary, 220)
                } else {
                    inline_clip(assistant_response, 220)
                }
            } else {
                inline_clip(assistant_response, 220)
            },
            next_focus: focus.as_ref().map(|(_, title, _)| title.clone()),
            created_at_unix: unix_now_secs(),
        };
        let _ = self
            .solve_trace_store
            .set_outcome(&self.session.session_id, &episode_id, outcome);
        self.update_experience_from_episode(&episode_id);
    }

    fn update_experience_from_episode(&self, episode_id: &str) {
        let Ok(trace) = self.solve_trace_store.load(&self.session.session_id) else {
            return;
        };
        let Some(episode) = trace.episodes.iter().find(|item| item.id == episode_id) else {
            return;
        };
        let Some(record) = derive_experience_from_episode(episode) else {
            return;
        };
        if self
            .experience_store
            .upsert_record(&self.session.session_id, record)
            .is_ok()
        {
            self.rebuild_meta_patterns();
        }
    }

    fn rebuild_meta_patterns(&self) {
        let Ok(experience_state) = self.experience_store.load(&self.session.session_id) else {
            return;
        };
        let _ = self
            .meta_pattern_store
            .rebuild_from_experience_state(&self.session.session_id, &experience_state);
    }

    async fn refresh_meta_patterns_with_model(&self, handler: &mut dyn EventHandler) {
        let Ok(state) = self.meta_pattern_store.load(&self.session.session_id) else {
            return;
        };
        if state.patterns.is_empty() {
            return;
        }

        let serialized = serialize_meta_patterns_for_summary(&state);
        if serialized.is_empty() {
            return;
        }

        let messages = vec![
            ChatMessage::system(META_PATTERN_SUMMARY_PROMPT),
            ChatMessage::user(serialized),
        ];
        let Some(message) = self
            .request_background_message(
                handler,
                "meta_pattern_summary",
                messages,
                Duration::from_secs(META_PATTERN_SUMMARY_TIMEOUT_SECS),
            )
            .await
        else {
            return;
        };
        let Some(response) = parse_meta_pattern_summary_response(&message.content_text()) else {
            return;
        };

        for patch in response.patterns {
            if patch.id.trim().is_empty() {
                continue;
            }
            let _ = self.meta_pattern_store.update_pattern(
                &self.session.session_id,
                &patch.id,
                patch.model_summary,
                patch.match_hints,
                patch
                    .strategy_template
                    .map(|template| MetaPatternStrategyTemplate {
                        applicable_when: template.applicable_when,
                        preferred_actions: template.preferred_actions,
                        avoid: template.avoid,
                        escalate_when: template.escalate_when,
                    }),
                patch.confidence,
            );
        }
    }

    fn record_tool_solve_trace(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        result: &str,
        status: &str,
    ) {
        let episode_id = self.active_archive_turn_id();
        let step = SolveStep {
            id: format!("tool:{tool_call_id}"),
            kind: "tool".to_string(),
            trigger: format!("Tool `{tool_name}` executed during solve loop"),
            action: format!("Run tool `{tool_name}`"),
            observation: summarize_tool_result(tool_name, result, status),
            evidence_refs: vec![tool_call_id.to_string()],
            status: map_tool_status_to_solve_step_status(status).to_string(),
            created_at_unix: unix_now_secs(),
        };
        let _ = self
            .solve_trace_store
            .append_step(&self.session.session_id, &episode_id, step);
    }

    fn record_delegate_worker_solve_trace(&self, tool_call_id: &str, result: &str, status: &str) {
        let Some(observation) = parse_delegate_worker_observation(result) else {
            self.record_tool_solve_trace(tool_call_id, "delegate_to_worker", result, status);
            return;
        };
        let episode_id = self.active_archive_turn_id();
        let step_status = map_tool_status_to_solve_step_status(
            observation.tool_status.as_deref().unwrap_or(status),
        );
        let summary = observation
            .payload
            .summary
            .clone()
            .unwrap_or_else(|| observation.objective.clone());
        let step = SolveStep {
            id: format!("delegate:{tool_call_id}"),
            kind: "delegate_worker".to_string(),
            trigger: format!(
                "Delegated worker assigned objective `{}`",
                observation.objective
            ),
            action: format!("Delegate worker objective: {}", observation.objective),
            observation: inline_clip(&summary, 220),
            evidence_refs: observation
                .payload
                .raw_refs
                .iter()
                .take(3)
                .cloned()
                .collect(),
            status: step_status.to_string(),
            created_at_unix: unix_now_secs(),
        };
        let _ = self
            .solve_trace_store
            .append_step(&self.session.session_id, &episode_id, step);

        let mut supplements = observation
            .payload
            .candidate_beliefs
            .iter()
            .take(2)
            .map(|item| format!("Belief: {}", inline_clip(item, 180)))
            .collect::<Vec<_>>();
        supplements.extend(
            observation
                .payload
                .candidate_risks
                .iter()
                .take(2)
                .map(|item| format!("Risk: {}", inline_clip(item, 180))),
        );
        if !supplements.is_empty() {
            let _ = self.solve_trace_store.append_supplements(
                &self.session.session_id,
                &episode_id,
                &supplements,
            );
        }

        if !observation.payload.recommended_next_actions.is_empty() {
            let decision = SolveDecision {
                id: format!("delegate:{tool_call_id}:next"),
                question: format!("What should we do next for `{}`?", observation.objective),
                chosen: observation
                    .payload
                    .recommended_next_actions
                    .iter()
                    .take(3)
                    .map(|item| inline_clip(item, 96))
                    .collect::<Vec<_>>()
                    .join("; "),
                rationale: observation
                    .payload
                    .key_evidence
                    .iter()
                    .chain(observation.payload.candidate_beliefs.iter())
                    .take(3)
                    .map(|item| inline_clip(item, 160))
                    .collect(),
                created_at_unix: unix_now_secs(),
            };
            let _ = self.solve_trace_store.append_decision(
                &self.session.session_id,
                &episode_id,
                decision,
            );
        }
    }

    async fn reconcile_goal_state_after_turn(
        &self,
        handler: &mut dyn EventHandler,
        user_input: &str,
        assistant_response: &str,
    ) {
        let Ok(mut state) = self.goal_state_store.load(&self.session.session_id) else {
            return;
        };
        if state.goals.is_empty() && state.cognition.is_empty() && state.hot_data.is_empty() {
            return;
        }

        let serialized = serialize_goal_state_turn_reconcile_input(
            &state,
            &self.session.history,
            user_input,
            assistant_response,
        );
        if serialized.is_empty() {
            return;
        }

        let messages = vec![
            ChatMessage::system(GOAL_STATE_TURN_RECONCILE_PROMPT),
            ChatMessage::user(serialized),
        ];
        let Some(message) = self
            .request_background_message(
                handler,
                "goal_state_turn_reconcile",
                messages,
                Duration::from_secs(GOAL_STATE_TURN_RECONCILE_TIMEOUT_SECS),
            )
            .await
        else {
            return;
        };
        let Some(patch) = parse_goal_state_reconcile_response(&message.content_text()) else {
            return;
        };

        apply_goal_state_reconcile_patch(&mut state, patch);
        let _ = self
            .goal_state_store
            .replace(&self.session.session_id, state);
    }

    async fn build_execution_brief_context_block(
        &self,
        handler: &mut dyn EventHandler,
        user_input: &str,
    ) -> Option<String> {
        let Ok(state) = self.goal_state_store.load(&self.session.session_id) else {
            return None;
        };
        if state.goals.is_empty() && state.cognition.is_empty() && state.hot_data.is_empty() {
            return None;
        }

        if self.config.auxiliary_model.is_none() {
            return derive_execution_brief_from_goal_state(&state)
                .map(|brief| render_goal_execution_brief_block(&brief));
        }

        let serialized = serialize_goal_state_for_execution_brief(&state, user_input);
        if serialized.is_empty() {
            return derive_execution_brief_from_goal_state(&state)
                .map(|brief| render_goal_execution_brief_block(&brief));
        }

        let messages = vec![
            ChatMessage::system(GOAL_STATE_EXECUTION_BRIEF_PROMPT),
            ChatMessage::user(serialized),
        ];
        let brief = self
            .request_background_message(
                handler,
                "goal_state_execution_brief",
                messages,
                Duration::from_secs(GOAL_STATE_EXECUTION_BRIEF_TIMEOUT_SECS),
            )
            .await
            .and_then(|message| parse_goal_state_execution_brief_response(&message.content_text()))
            .or_else(|| derive_execution_brief_from_goal_state(&state))?;
        Some(render_goal_execution_brief_block(&brief))
    }

    async fn build_goal_state_delta_context_block(
        &self,
        handler: &mut dyn EventHandler,
        user_input: &str,
    ) -> Option<String> {
        self.config.auxiliary_model.as_ref()?;
        let Ok(state) = self.goal_state_store.load(&self.session.session_id) else {
            return None;
        };
        if state.goals.is_empty() && state.cognition.is_empty() && state.hot_data.is_empty() {
            return None;
        }

        let serialized = serialize_goal_state_for_delta(&state, user_input);
        if serialized.is_empty() {
            return None;
        }

        let messages = vec![
            ChatMessage::system(GOAL_STATE_DELTA_PROMPT),
            ChatMessage::user(serialized),
        ];
        let delta = self
            .request_background_message(
                handler,
                "goal_state_delta",
                messages,
                Duration::from_secs(GOAL_STATE_DELTA_TIMEOUT_SECS),
            )
            .await
            .and_then(|message| parse_goal_state_delta_response(&message.content_text()))?;
        Some(render_goal_state_delta_block(&delta))
    }

    async fn build_goal_state_context_block(
        &self,
        handler: &mut dyn EventHandler,
        allow_compaction: bool,
    ) -> Option<String> {
        let Ok(state) = self.goal_state_store.load(&self.session.session_id) else {
            return None;
        };
        if state.goals.is_empty() && state.cognition.is_empty() && state.hot_data.is_empty() {
            return None;
        }

        if !allow_compaction || !needs_goal_state_compaction(&state) {
            return self
                .goal_state_store
                .build_brief_context_block(&self.session.session_id)
                .ok()
                .flatten();
        }

        match self.compact_goal_state_for_prompt(&state, handler).await {
            Ok(Some(compaction)) => Some(render_compacted_goal_state_block(&state, &compaction)),
            Ok(None) | Err(_) => self
                .goal_state_store
                .build_brief_context_block(&self.session.session_id)
                .ok()
                .flatten(),
        }
    }

    async fn compact_goal_state_for_prompt(
        &self,
        state: &GoalState,
        handler: &mut dyn EventHandler,
    ) -> Result<Option<GoalStateCompactionResponse>> {
        let serialized = serialize_goal_state_for_compaction(state);
        if serialized.is_empty() {
            return Ok(None);
        }

        let messages = vec![
            ChatMessage::system(GOAL_STATE_COMPACTION_PROMPT),
            ChatMessage::user(serialized),
        ];
        let Some(message) = self
            .request_background_message(
                handler,
                "goal_state_compaction",
                messages,
                Duration::from_secs(GOAL_STATE_COMPACTION_TIMEOUT_SECS),
            )
            .await
        else {
            return Ok(None);
        };
        Ok(parse_goal_state_compaction_response(
            &message.content_text(),
        ))
    }

    async fn request_background_message(
        &self,
        handler: &mut dyn EventHandler,
        purpose: &str,
        messages: Vec<ChatMessage>,
        timeout_duration: Duration,
    ) -> Option<ChatMessage> {
        let client = self.background_client();
        let model = self.background_model();
        self.persist_debug_context_snapshot(
            "background_request",
            None,
            Some(purpose),
            Some(&model),
            None,
            &messages,
            None,
            None,
            0,
        );
        handler.on_event(AgentEvent::BackgroundModelRequestStarted {
            session_id: self.session.session_id.clone(),
            purpose: purpose.to_string(),
            model: model.clone(),
            message_count: messages.len(),
        });

        let response = timeout(timeout_duration, client.respond(&model, &messages, &[])).await;
        match response {
            Ok(Ok(response)) => {
                let message = response.message;
                handler.on_event(AgentEvent::BackgroundModelRequestFinished {
                    session_id: self.session.session_id.clone(),
                    purpose: purpose.to_string(),
                    model,
                    status: "ok".to_string(),
                    content_preview: truncated(message.content_text(), 240),
                });
                Some(message)
            }
            Ok(Err(error)) => {
                handler.on_event(AgentEvent::BackgroundModelRequestFinished {
                    session_id: self.session.session_id.clone(),
                    purpose: purpose.to_string(),
                    model,
                    status: "error".to_string(),
                    content_preview: truncated(error.to_string(), 240),
                });
                None
            }
            Err(_) => {
                handler.on_event(AgentEvent::BackgroundModelRequestFinished {
                    session_id: self.session.session_id.clone(),
                    purpose: purpose.to_string(),
                    model,
                    status: "timeout".to_string(),
                    content_preview: String::new(),
                });
                None
            }
        }
    }

    fn upsert_archive_turn(&self, turn_id: &str) {
        let turn_index = turn_id
            .strip_prefix("turn-")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        if let Err(error) = self.archive_store.upsert_turn(
            &self.session.session_id,
            turn_id,
            turn_index,
            unix_now_ms(),
        ) {
            warn!(
                "failed to persist archive turn {} for session {}: {error:#}",
                turn_id, self.session.session_id
            );
        }
    }

    fn append_archive_message(
        &self,
        role: &str,
        content: &str,
        tool_call_id: Option<&str>,
        raw_json: Option<&str>,
    ) {
        let turn_id = self.active_archive_turn_id();
        self.upsert_archive_turn(&turn_id);
        let redacted_content = redact_secrets(content);
        let redacted_raw_json = raw_json.map(redact_secrets);
        if let Err(error) = self.archive_store.append_message(
            &self.session.session_id,
            &turn_id,
            role,
            &redacted_content,
            tool_call_id,
            redacted_raw_json.as_deref(),
        ) {
            warn!(
                "failed to append archive message for session {} turn {}: {error:#}",
                self.session.session_id, turn_id
            );
        }
    }

    fn append_archive_event(&self, event_type: &str, title: &str, summary: &str) {
        let turn_id = self.active_turn_id.as_deref();
        let redacted_summary = redact_secrets(summary);
        if let Err(error) = self.archive_store.append_event(
            &self.session.session_id,
            turn_id,
            event_type,
            title,
            &redacted_summary,
        ) {
            warn!(
                "failed to append archive event {} for session {}: {error:#}",
                event_type, self.session.session_id
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn upsert_archive_tool_call(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        execution_mode: &str,
        batch_id: Option<&str>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
        phase: &str,
        arguments_raw: Option<&str>,
        output_raw: Option<&str>,
    ) {
        let turn_id = self.active_archive_turn_id();
        self.upsert_archive_turn(&turn_id);
        let redacted_arguments = arguments_raw.map(redact_secrets);
        let redacted_output = output_raw.map(redact_secrets);
        if let Err(error) = self.archive_store.upsert_tool_call(
            tool_call_id,
            &self.session.session_id,
            &turn_id,
            tool_name,
            execution_mode,
            batch_id,
            batch_index,
            batch_total,
            phase,
            redacted_arguments.as_deref(),
            redacted_output.as_deref(),
        ) {
            warn!(
                "failed to upsert archive tool call {} for session {}: {error:#}",
                tool_call_id, self.session.session_id
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn upsert_archive_approval(
        &self,
        approval_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        reason: &str,
        command: &str,
        execution_mode: &str,
        batch_id: Option<&str>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    ) {
        let turn_id = self.active_archive_turn_id();
        self.upsert_archive_turn(&turn_id);
        if let Err(error) = self.archive_store.upsert_approval(
            approval_id,
            &self.session.session_id,
            &turn_id,
            tool_call_id,
            tool_name,
            reason,
            command,
            execution_mode,
            batch_id,
            batch_index,
            batch_total,
        ) {
            warn!(
                "failed to upsert archive approval {} for session {}: {error:#}",
                approval_id, self.session.session_id
            );
        }
    }

    pub async fn run_prompt(&mut self, user_input: &str) -> Result<String> {
        let mut handler = NoopEventHandler;
        self.run_prompt_with_handler(user_input, &mut handler).await
    }

    pub async fn debug_context_preview(&mut self, user_input: &str) -> Result<ContextPreview> {
        let original_session = self.session.clone();
        let original_active_turn_id = self.active_turn_id.clone();
        let goal_state_path = self
            .goal_state_store
            .root()
            .join("goal_state")
            .join(format!("{}.json", self.session.session_id));
        let goal_state_existed = goal_state_path.exists();
        let original_goal_state = goal_state_existed
            .then(|| self.goal_state_store.load(&self.session.session_id))
            .transpose()?;

        let preview = async {
            let mut handler = RecordingEventHandler::new();
            self.session.history.push(ChatMessage::user(user_input));
            let turn_id = self.session.record_user_timeline_entry(user_input);
            self.active_turn_id = Some(turn_id);
            self.seed_goal_state_from_user_input(user_input)?;

            let mut progress = TurnProgress::default();
            let system_prompt = build_system_prompt(&self.config);
            let (injections, matched_skill_labels) = self
                .collect_turn_context_injections(&mut handler, 1, user_input, &mut progress)
                .await;
            let (messages, injection_stats, projected_tokens) = self
                .prepare_turn_messages_with_budget(&mut handler, system_prompt, &injections)
                .await?;

            Ok::<ContextPreview, anyhow::Error>(ContextPreview {
                session_id: self.session.session_id.clone(),
                user_input: user_input.to_string(),
                projected_tokens,
                request_budget_tokens: self.request_context_budget_tokens(),
                tool_definition_count: self.tools.primary_model_definitions().len(),
                matched_skills: matched_skill_labels,
                messages,
                injections: injections
                    .iter()
                    .map(|item| ContextPreviewInjection {
                        label: item.label.to_string(),
                        content: item.content.clone(),
                        original_chars: item.content.chars().count(),
                        max_chars: item.max_chars,
                    })
                    .collect(),
                injection_stats: ContextPreviewStats {
                    total_blocks: injection_stats.total_blocks,
                    kept_blocks: injection_stats.kept_blocks,
                    original_chars: injection_stats.original_chars,
                    final_chars: injection_stats.final_chars,
                    clipped_labels: injection_stats
                        .clipped_labels
                        .iter()
                        .map(|label| (*label).to_string())
                        .collect(),
                    skipped_labels: injection_stats
                        .skipped_labels
                        .iter()
                        .map(|label| (*label).to_string())
                        .collect(),
                },
                events: handler.into_events(),
            })
        }
        .await;

        self.session = original_session;
        self.active_turn_id = original_active_turn_id;
        if let Some(state) = original_goal_state {
            self.goal_state_store
                .replace(&self.session.session_id, state)?;
        } else if !goal_state_existed {
            self.goal_state_store.clear(&self.session.session_id)?;
        }

        preview
    }

    pub async fn run_prompt_with_handler(
        &mut self,
        user_input: &str,
        handler: &mut dyn EventHandler,
    ) -> Result<String> {
        clear_stop_request(&self.config.data_dir, &self.session.session_id)?;
        let new_session = self.session.history.is_empty();
        handler.on_event(AgentEvent::SessionReady {
            session_id: self.session.session_id.clone(),
            resumed: !new_session,
        });
        if new_session {
            let session_id = self.session.session_id.clone();
            let provider_id = self.config.provider_id.clone();
            let model = self.config.model.clone();
            let payload = serde_json::json!({
                "event": "on_session_start",
                "session_id": session_id,
                "provider": provider_id,
                "model": model,
                "workspace_root": self.config.workspace_root.display().to_string(),
            });
            let _ = self.tools.run_hooks("on_session_start", &payload).await;
        }
        handler.on_event(AgentEvent::TurnStarted {
            session_id: self.session.session_id.clone(),
            user_input: user_input.to_string(),
        });

        let repaired_tool_calls = repair_dangling_tool_calls(&mut self.session.history);
        if repaired_tool_calls > 0 {
            warn!(
                "repaired {repaired_tool_calls} dangling tool call(s) before starting a new turn in session {}",
                self.session.session_id
            );
        }

        self.session.history.push(ChatMessage::user(user_input));
        let turn_id = self.session.record_user_timeline_entry(user_input);
        self.active_turn_id = Some(turn_id);
        self.seed_goal_state_from_user_input(user_input)?;
        self.start_solve_episode(user_input);
        self.upsert_archive_turn(&self.active_archive_turn_id());
        self.append_archive_message("user", user_input, None, None);
        self.append_archive_event(
            "turn_started",
            "User turn started",
            &truncated(user_input, 240),
        );
        self.persist_session_with_handler(handler)?;

        self.continue_active_turn(user_input, handler, TurnProgress::default())
            .await
    }

    pub async fn resume_pending_approval_with_handler(
        &mut self,
        approval_id: &str,
        handler: &mut dyn EventHandler,
    ) -> Result<String> {
        clear_stop_request(&self.config.data_dir, &self.session.session_id)?;
        handler.on_event(AgentEvent::SessionReady {
            session_id: self.session.session_id.clone(),
            resumed: true,
        });

        let pending = load_pending_approval(&self.config.data_dir, approval_id)?
            .ok_or_else(|| anyhow::anyhow!("pending approval `{approval_id}` not found"))?;
        if pending.session_id != self.session.session_id {
            bail!(
                "approval `{approval_id}` belongs to session `{}`, not `{}`",
                pending.session_id,
                self.session.session_id
            );
        }

        let approval = get_request(&self.config.data_dir, approval_id)?;
        self.session.record_tool_timeline_entry(
            pending.tool_call_id.clone(),
            pending.tool_name.clone(),
            truncated(pending.raw_arguments.clone(), 300),
            StoredToolPhase::Running,
            Some(&pending.execution_mode),
            pending.batch_id.as_deref(),
            pending.batch_index,
            pending.batch_total,
        );
        self.active_turn_id = Some(self.session.current_turn_id());
        self.upsert_archive_tool_call(
            &pending.tool_call_id,
            &pending.tool_name,
            &pending.execution_mode,
            pending.batch_id.as_deref(),
            pending.batch_index,
            pending.batch_total,
            "running",
            Some(&pending.raw_arguments),
            None,
        );
        let _ = self.persist_session();
        handler.on_event(AgentEvent::ToolCallStarted {
            session_id: self.session.session_id.clone(),
            iteration: 0,
            tool_call_id: pending.tool_call_id.clone(),
            tool_name: pending.tool_name.clone(),
            arguments_preview: redacted_truncated(pending.raw_arguments.clone(), 300),
            execution_mode: pending.execution_mode.clone(),
            batch_id: pending.batch_id.clone(),
            batch_index: pending.batch_index,
            batch_total: pending.batch_total,
        });
        let tool_started_at = Instant::now();
        let result = match approval.status {
            ApprovalStatus::Approved => {
                self.call_tool_with_live_updates(
                    handler,
                    0,
                    &pending.tool_call_id,
                    &pending.tool_name,
                    &pending.raw_arguments,
                    &pending.execution_mode,
                    pending.batch_id.as_deref(),
                    pending.batch_index,
                    pending.batch_total,
                )
                .await
            }
            ApprovalStatus::Denied => format!(
                "approval denied for command `{}`: {}",
                pending.command, approval.reason
            ),
            ApprovalStatus::Pending => {
                bail!("approval `{approval_id}` is still pending");
            }
            ApprovalStatus::Consumed => {
                bail!("approval `{approval_id}` was already consumed");
            }
        };
        let duration_ms = elapsed_ms(tool_started_at);
        let result = match parse_tool_args(&pending.raw_arguments) {
            Some(args) => match self
                .subdir_hint_tracker
                .check_tool_call(&pending.tool_name, &args)
            {
                Some(hint) => format!("{result}\n\n{hint}"),
                None => result,
            },
            None => result,
        };
        let result = redact_secrets(result);
        let tool_status = classify_tool_result_status(&result);
        self.session.record_tool_timeline_entry(
            pending.tool_call_id.clone(),
            pending.tool_name.clone(),
            result.clone(),
            tool_status_to_phase(tool_status),
            Some(&pending.execution_mode),
            pending.batch_id.as_deref(),
            pending.batch_index,
            pending.batch_total,
        );
        self.upsert_archive_tool_call(
            &pending.tool_call_id,
            &pending.tool_name,
            &pending.execution_mode,
            pending.batch_id.as_deref(),
            pending.batch_index,
            pending.batch_total,
            tool_status,
            Some(&pending.raw_arguments),
            Some(&result),
        );
        self.update_goal_state_from_tool_outcome(
            &pending.tool_call_id,
            &pending.tool_name,
            &result,
            &pending.execution_mode,
            tool_status,
        );
        let reconcile_summary = self
            .reconcile_goal_state_after_tool_outcome(
                handler,
                &pending.tool_call_id,
                &pending.tool_name,
                &result,
                &pending.execution_mode,
                tool_status,
            )
            .await;
        handler.on_event(AgentEvent::ToolCallFinished {
            session_id: self.session.session_id.clone(),
            iteration: 0,
            tool_call_id: pending.tool_call_id.clone(),
            tool_name: pending.tool_name.clone(),
            status: tool_status.to_string(),
            duration_ms,
            output_preview: redacted_truncated(result.clone(), 500),
            execution_mode: pending.execution_mode.clone(),
            batch_id: pending.batch_id.clone(),
            batch_index: pending.batch_index,
            batch_total: pending.batch_total,
        });
        self.append_archive_message("tool", &result, Some(&pending.tool_call_id), None);
        self.session.history.push(ChatMessage::tool(
            pending.tool_call_id,
            summarize_tool_result_for_history(
                &pending.tool_name,
                &result,
                tool_status,
                reconcile_summary.as_deref(),
            ),
        ));
        self.persist_session_with_handler(handler)?;
        remove_pending_approval(&self.config.data_dir, approval_id)?;

        let user_input = self.latest_user_input()?;
        self.continue_active_turn(&user_input, handler, TurnProgress::default())
            .await
    }

    async fn continue_active_turn(
        &mut self,
        user_input: &str,
        handler: &mut dyn EventHandler,
        mut progress: TurnProgress,
    ) -> Result<String> {
        let tool_definitions = self.tools.primary_model_definitions();

        for iteration in 1..=self.config.max_iterations {
            self.check_stop_requested(handler)?;
            info!("starting agent iteration {iteration}");
            handler.on_event(AgentEvent::IterationStarted {
                session_id: self.session.session_id.clone(),
                iteration,
                max_iterations: self.config.max_iterations,
            });
            if should_run_context_compression_before_iteration(iteration) {
                let background_client = self.background_client();
                let background_model = self.background_model();
                match self
                    .context_compressor
                    .maybe_compress_session(
                        &mut self.session,
                        &background_client,
                        &background_model,
                    )
                    .await
                {
                    Ok(Some(outcome)) => {
                        self.persist_session_with_handler(handler)?;
                        self.emit_context_compaction_nudge(handler, outcome, None);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        info!("context compression skipped after error: {error:#}");
                    }
                }
            }

            let mut system_prompt = build_system_prompt(&self.config);
            if progress.turn_tool_calls >= 5 && !progress.skill_manage_used {
                system_prompt.push_str(
                    "\n\n# Skill reminder\nIf this workflow is reusable, save it as a skill with `skill_manage` before finishing.",
                );
            }

            let (injections, matched_skill_labels) = self
                .collect_turn_context_injections(handler, iteration, user_input, &mut progress)
                .await;
            let (messages, injection_stats, projected_tokens) = self
                .prepare_turn_messages_with_budget(handler, system_prompt, &injections)
                .await?;
            self.persist_debug_context_snapshot(
                "main_request_preflight",
                Some(iteration),
                None,
                None,
                Some(projected_tokens),
                &messages,
                Some(&injections),
                Some(&injection_stats),
                tool_definitions.len(),
            );
            if injection_stats.was_trimmed() {
                handler.on_event(AgentEvent::Nudge {
                    session_id: self.session.session_id.clone(),
                    kind: "context".to_string(),
                    message: render_context_budget_nudge(&injection_stats),
                });
            }
            if !matched_skill_labels.is_empty() {
                handler.on_event(AgentEvent::SkillMatched {
                    session_id: self.session.session_id.clone(),
                    skills: matched_skill_labels,
                });
            }
            self.emit_request_context_pressure_nudge(handler, projected_tokens);

            let assistant = match self
                .request_assistant_message(
                    handler,
                    iteration,
                    user_input,
                    messages,
                    &tool_definitions,
                )
                .await
            {
                Ok(message) => message,
                Err(error) => {
                    handler.on_event(AgentEvent::Error {
                        session_id: self.session.session_id.clone(),
                        message: error.to_string(),
                    });
                    return Err(error);
                }
            };

            let tool_calls = assistant.tool_calls.clone().unwrap_or_default();
            let sanitized_assistant = redact_chat_message_secrets(assistant);
            let final_text = sanitized_assistant.content_text();
            let assistant_raw_json = serde_json::to_string(&sanitized_assistant).ok();

            self.session.history.push(sanitized_assistant);
            self.append_archive_message(
                "assistant",
                &final_text,
                None,
                assistant_raw_json.as_deref(),
            );
            if tool_calls.is_empty() {
                self.session
                    .record_assistant_timeline_entry(final_text.clone());
                self.reconcile_goal_state_after_turn(handler, user_input, &final_text)
                    .await;
                self.record_turn_solve_outcome(&final_text);
                self.refresh_meta_patterns_with_model(handler).await;
            }
            self.persist_session_with_handler(handler)?;

            if tool_calls.is_empty() {
                self.maybe_generate_session_title(handler, user_input, &final_text)
                    .await;
                self.emit_skill_lifecycle_suggestion(
                    handler,
                    user_input,
                    progress.turn_skill_matches.clone(),
                    progress.turn_tool_names.clone(),
                    progress.skill_manage_used,
                );
                self.emit_skill_nudge_if_needed(
                    handler,
                    progress.turn_tool_calls,
                    progress.skill_manage_used,
                    user_input,
                );
                handler.on_event(AgentEvent::AssistantMessage {
                    session_id: self.session.session_id.clone(),
                    content: final_text.clone(),
                });
                self.append_archive_event(
                    "assistant_message",
                    "Assistant replied",
                    &truncated(final_text.clone(), 320),
                );
                let post_content = final_text.clone();
                let payload = serde_json::json!({
                    "event": "post_llm_call",
                    "session_id": self.session.session_id.clone(),
                    "provider": self.config.provider_id.clone(),
                    "model": self.config.model.clone(),
                    "content": post_content.clone(),
                });
                let _ = self.tools.run_hooks("post_llm_call", &payload).await;
                let end_payload = serde_json::json!({
                    "event": "on_session_end",
                    "session_id": self.session.session_id.clone(),
                    "provider": self.config.provider_id.clone(),
                    "model": self.config.model.clone(),
                    "content": post_content,
                });
                let _ = self.tools.run_hooks("on_session_end", &end_payload).await;
                return Ok(end_payload
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string());
            }

            if should_parallelize_tool_batch(&tool_calls, &self.tool_context) {
                if let Some(result) = self
                    .execute_tool_calls_parallel(tool_calls, iteration, handler, &mut progress)
                    .await?
                {
                    return Ok(result);
                }
                continue;
            }

            for call in tool_calls {
                self.check_stop_requested(handler)?;
                progress.turn_tool_calls += 1;
                progress.turn_tool_names.push(call.function.name.clone());
                if call.function.name == "skill_manage" {
                    progress.skill_manage_used = true;
                }
                info!("executing tool {}", call.function.name);
                self.session.record_tool_timeline_entry(
                    call.id.clone(),
                    call.function.name.clone(),
                    truncated(call.function.arguments.clone(), 300),
                    StoredToolPhase::Running,
                    Some("sequential"),
                    None,
                    None,
                    None,
                );
                self.upsert_archive_tool_call(
                    &call.id,
                    &call.function.name,
                    "sequential",
                    None,
                    None,
                    None,
                    "running",
                    Some(&call.function.arguments),
                    None,
                );
                handler.on_event(AgentEvent::ToolCallStarted {
                    session_id: self.session.session_id.clone(),
                    iteration,
                    tool_call_id: call.id.clone(),
                    tool_name: call.function.name.clone(),
                    arguments_preview: redacted_truncated(call.function.arguments.clone(), 300),
                    execution_mode: "sequential".to_string(),
                    batch_id: None,
                    batch_index: None,
                    batch_total: None,
                });
                let tool_started_at = Instant::now();
                let result = self
                    .call_tool_with_live_updates(
                        handler,
                        iteration,
                        &call.id,
                        &call.function.name,
                        &call.function.arguments,
                        "sequential",
                        None,
                        None,
                        None,
                    )
                    .await;
                let duration_ms = elapsed_ms(tool_started_at);
                let result = match parse_tool_args(&call.function.arguments) {
                    Some(args) => {
                        match self
                            .subdir_hint_tracker
                            .check_tool_call(&call.function.name, &args)
                        {
                            Some(hint) => format!("{result}\n\n{hint}"),
                            None => result,
                        }
                    }
                    None => result,
                };
                let result = redact_secrets(result);
                if let Some(approval) = parse_approval_required(&result) {
                    self.session.record_tool_timeline_entry(
                        call.id.clone(),
                        call.function.name.clone(),
                        approval.reason.clone(),
                        StoredToolPhase::Approval,
                        Some("sequential"),
                        None,
                        None,
                        None,
                    );
                    self.session.record_approval_timeline_entry(
                        approval.approval_id.clone(),
                        call.function.name.clone(),
                        approval.reason.clone(),
                        approval.command.clone(),
                        Some("sequential"),
                        None,
                        None,
                        None,
                    );
                    save_pending_approval(
                        &self.config.data_dir,
                        &approval.approval_id,
                        &self.session.session_id,
                        &call.id,
                        &call.function.name,
                        "sequential",
                        None,
                        None,
                        None,
                        &call.function.arguments,
                        &approval.command,
                    )?;
                    self.upsert_archive_tool_call(
                        &call.id,
                        &call.function.name,
                        "sequential",
                        None,
                        None,
                        None,
                        "approval",
                        Some(&call.function.arguments),
                        Some(&approval.reason),
                    );
                    self.upsert_archive_approval(
                        &approval.approval_id,
                        &call.id,
                        &call.function.name,
                        &approval.reason,
                        &approval.command,
                        "sequential",
                        None,
                        None,
                        None,
                    );
                    let approval_id = approval.approval_id.clone();
                    let approval_reason = approval.reason.clone();
                    let approval_command = approval.command.clone();
                    handler.on_event(AgentEvent::ApprovalRequired {
                        session_id: self.session.session_id.clone(),
                        tool_call_id: call.id.clone(),
                        tool_name: call.function.name.clone(),
                        approval_id,
                        reason: approval_reason.clone(),
                        command: approval_command,
                        execution_mode: "sequential".to_string(),
                        batch_id: None,
                        batch_index: None,
                        batch_total: None,
                    });
                    self.append_archive_event(
                        "approval_required",
                        "Tool approval required",
                        &truncated(approval_reason, 240),
                    );
                    self.update_goal_state_from_tool_outcome(
                        &call.id,
                        &call.function.name,
                        &approval.reason,
                        "sequential",
                        "approval_required",
                    );
                    self.reconcile_goal_state_after_tool_outcome(
                        handler,
                        &call.id,
                        &call.function.name,
                        &approval.reason,
                        "sequential",
                        "approval_required",
                    )
                    .await;
                    self.persist_session_with_handler(handler)?;
                    return Ok(APPROVAL_PENDING_RESPONSE.to_string());
                }
                let tool_status = classify_tool_result_status(&result);
                self.session.record_tool_timeline_entry(
                    call.id.clone(),
                    call.function.name.clone(),
                    result.clone(),
                    tool_status_to_phase(tool_status),
                    Some("sequential"),
                    None,
                    None,
                    None,
                );
                self.upsert_archive_tool_call(
                    &call.id,
                    &call.function.name,
                    "sequential",
                    None,
                    None,
                    None,
                    tool_status,
                    Some(&call.function.arguments),
                    Some(&result),
                );
                self.update_goal_state_from_tool_outcome(
                    &call.id,
                    &call.function.name,
                    &result,
                    "sequential",
                    tool_status,
                );
                let reconcile_summary = self
                    .reconcile_goal_state_after_tool_outcome(
                        handler,
                        &call.id,
                        &call.function.name,
                        &result,
                        "sequential",
                        tool_status,
                    )
                    .await;
                let tool_name = call.function.name.clone();
                handler.on_event(AgentEvent::ToolCallFinished {
                    session_id: self.session.session_id.clone(),
                    iteration,
                    tool_call_id: call.id.clone(),
                    tool_name: tool_name.clone(),
                    status: tool_status.to_string(),
                    duration_ms,
                    output_preview: redacted_truncated(result.clone(), 500),
                    execution_mode: "sequential".to_string(),
                    batch_id: None,
                    batch_index: None,
                    batch_total: None,
                });
                self.append_archive_message("tool", &result, Some(&call.id), None);
                self.session.history.push(ChatMessage::tool(
                    call.id,
                    summarize_tool_result_for_history(
                        &tool_name,
                        &result,
                        tool_status,
                        reconcile_summary.as_deref(),
                    ),
                ));
                self.persist_session_with_handler(handler)?;
            }
        }

        self.finish_after_tool_iteration_limit(user_input, handler, progress)
            .await
    }

    async fn collect_turn_context_injections(
        &mut self,
        handler: &mut dyn EventHandler,
        iteration: usize,
        user_input: &str,
        progress: &mut TurnProgress,
    ) -> (Vec<ContextInjection>, Vec<String>) {
        let mut injections = Vec::new();
        if let Ok(memory_snapshot) =
            build_memory_snapshot(&self.config.data_dir, &self.session, user_input)
        {
            push_context_injection(
                &mut injections,
                "memory_snapshot",
                memory_snapshot,
                MEMORY_SNAPSHOT_BUDGET_CHARS,
            );
        }
        if let Some(execution_brief_block) = self
            .build_execution_brief_context_block(handler, user_input)
            .await
        {
            push_context_injection(
                &mut injections,
                "execution_brief",
                execution_brief_block,
                EXECUTION_BRIEF_BUDGET_CHARS,
            );
        }
        if let Some(state_delta_block) = self
            .build_goal_state_delta_context_block(handler, user_input)
            .await
        {
            push_context_injection(
                &mut injections,
                "state_delta",
                state_delta_block,
                STATE_DELTA_BUDGET_CHARS,
            );
        }
        if let Some(goal_state_block) = self
            .build_goal_state_context_block(handler, iteration > 1)
            .await
        {
            push_context_injection(
                &mut injections,
                "goal_state",
                goal_state_block,
                GOAL_STATE_BUDGET_CHARS,
            );
        }
        if self.config.enable_solve_trace_context {
            if let Ok(Some(solve_trace_block)) = self.solve_trace_store.build_context_block(
                &self.session.session_id,
                user_input,
                self.active_turn_id.as_deref(),
            ) {
                push_context_injection(
                    &mut injections,
                    "solve_trace",
                    solve_trace_block,
                    SOLVE_TRACE_BUDGET_CHARS,
                );
            }
        }
        if self.config.enable_meta_pattern_context {
            if let Ok(Some(meta_pattern_block)) = self
                .meta_pattern_store
                .build_context_block(&self.session.session_id, user_input)
            {
                push_context_injection(
                    &mut injections,
                    "meta_pattern",
                    meta_pattern_block,
                    META_PATTERN_BUDGET_CHARS,
                );
            }
        }
        if self.config.enable_experience_context {
            if let Ok(Some(experience_block)) = self.experience_store.build_context_block(
                &self.session.session_id,
                user_input,
                self.active_turn_id.as_deref(),
            ) {
                push_context_injection(
                    &mut injections,
                    "experience",
                    experience_block,
                    EXPERIENCE_BUDGET_CHARS,
                );
            }
        }
        if let Ok(Some(todo_block)) = self
            .todo_store
            .build_context_block(&self.session.session_id)
        {
            push_context_injection(&mut injections, "todo", todo_block, TODO_BUDGET_CHARS);
        }
        progress.turn_skill_matches.clear();
        let matched_skill_labels = Vec::new();
        let plugin_context = self
            .tools
            .run_hooks(
                "pre_llm_call",
                &serde_json::json!({
                    "event": "pre_llm_call",
                    "session_id": self.session.session_id.clone(),
                    "iteration": iteration,
                    "user_input": user_input,
                    "provider": self.config.provider_id.clone(),
                    "model": self.config.model.clone(),
                }),
            )
            .await;
        if !plugin_context.is_empty() {
            push_context_injection(
                &mut injections,
                "plugin_context",
                plugin_context
                    .into_iter()
                    .map(|item| format!("# Plugin Context\n{item}"))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                PLUGIN_CONTEXT_BUDGET_CHARS,
            );
        }

        (injections, matched_skill_labels)
    }

    async fn prepare_turn_messages_with_budget(
        &mut self,
        handler: &mut dyn EventHandler,
        system_prompt: String,
        injections: &[ContextInjection],
    ) -> Result<(Vec<ChatMessage>, ContextInjectionStats, usize)> {
        let request_budget_tokens = self.request_context_budget_tokens();
        let mut injection_budget_chars = CONTEXT_INJECTION_TOTAL_BUDGET_CHARS;
        let mut attempted_preflight_compression = false;

        loop {
            let (messages, injection_stats) = assemble_turn_messages(
                &system_prompt,
                &self.session.history,
                injections,
                injection_budget_chars,
            );
            let projected_tokens = estimate_messages_tokens(&messages);
            if projected_tokens <= request_budget_tokens {
                return Ok((messages, injection_stats, projected_tokens));
            }

            if !attempted_preflight_compression {
                let background_client = self.background_client();
                let background_model = self.background_model();
                match self
                    .context_compressor
                    .force_compress_session(
                        &mut self.session,
                        &background_client,
                        &background_model,
                    )
                    .await
                {
                    Ok(Some(outcome)) => {
                        self.persist_session_with_handler(handler)?;
                        self.emit_context_compaction_nudge(
                            handler,
                            outcome,
                            Some("发送前预算检查触发了上下文压缩"),
                        );
                        attempted_preflight_compression = true;
                        continue;
                    }
                    Ok(None) => {
                        attempted_preflight_compression = true;
                    }
                    Err(error) => {
                        info!("preflight context compression skipped after error: {error:#}");
                        attempted_preflight_compression = true;
                    }
                }
            }

            if injection_budget_chars > REDUCED_CONTEXT_INJECTION_TOTAL_BUDGET_CHARS {
                injection_budget_chars = REDUCED_CONTEXT_INJECTION_TOTAL_BUDGET_CHARS;
                handler.on_event(AgentEvent::Nudge {
                    session_id: self.session.session_id.clone(),
                    kind: "context".to_string(),
                    message: format!(
                        "预计请求上下文约 {} tokens，已将可选注入预算收紧到 {} chars。",
                        projected_tokens, injection_budget_chars
                    ),
                });
                continue;
            }

            return Ok((messages, injection_stats, projected_tokens));
        }
    }

    async fn finish_after_tool_iteration_limit(
        &mut self,
        user_input: &str,
        handler: &mut dyn EventHandler,
        mut progress: TurnProgress,
    ) -> Result<String> {
        handler.on_event(AgentEvent::Nudge {
            session_id: self.session.session_id.clone(),
            kind: "loop".to_string(),
            message: format!(
                "已达到 {} 轮工具调用上限，正在基于已有结果收尾。",
                self.config.max_iterations
            ),
        });

        let mut system_prompt = build_system_prompt(&self.config);
        system_prompt.push_str(
            "\n\n# Tool budget exhausted\nThe tool-calling budget for this turn is exhausted. Do not call more tools. Provide the best final answer using the available conversation and tool results. Be explicit about anything that remains unverified.",
        );
        let (injections, matched_skill_labels) = self
            .collect_turn_context_injections(
                handler,
                self.config.max_iterations + 1,
                user_input,
                &mut progress,
            )
            .await;
        let (messages, injection_stats, projected_tokens) = self
            .prepare_turn_messages_with_budget(handler, system_prompt, &injections)
            .await?;
        self.persist_debug_context_snapshot(
            "final_request_preflight",
            Some(self.config.max_iterations + 1),
            None,
            None,
            Some(projected_tokens),
            &messages,
            Some(&injections),
            Some(&injection_stats),
            0,
        );
        if injection_stats.was_trimmed() {
            handler.on_event(AgentEvent::Nudge {
                session_id: self.session.session_id.clone(),
                kind: "context".to_string(),
                message: render_context_budget_nudge(&injection_stats),
            });
        }
        if !matched_skill_labels.is_empty() {
            handler.on_event(AgentEvent::SkillMatched {
                session_id: self.session.session_id.clone(),
                skills: matched_skill_labels,
            });
        }
        self.emit_request_context_pressure_nudge(handler, projected_tokens);

        let assistant = match self
            .request_assistant_message(
                handler,
                self.config.max_iterations + 1,
                user_input,
                messages,
                &[],
            )
            .await
        {
            Ok(message) => message,
            Err(error) => {
                handler.on_event(AgentEvent::Error {
                    session_id: self.session.session_id.clone(),
                    message: format!("工具调用已达到上限，收尾回复也失败：{}", error),
                });
                return Err(error);
            }
        };

        let mut assistant_message = assistant;
        let mut final_text = assistant_message.content_text();
        if final_text.trim().is_empty() {
            final_text = format!(
                "已达到 {} 轮工具调用上限，但模型没有生成可展示的收尾回复。请继续发送下一步指令，我会基于当前工具结果接着处理。",
                self.config.max_iterations
            );
            assistant_message = ChatMessage::assistant(final_text.clone());
            self.session.set_latest_response_state(None, None, None);
        }

        let assistant_raw_json = serde_json::to_string(&assistant_message).ok();
        self.session.history.push(assistant_message);
        self.append_archive_message(
            "assistant",
            &final_text,
            None,
            assistant_raw_json.as_deref(),
        );
        self.session
            .record_assistant_timeline_entry(final_text.clone());
        self.reconcile_goal_state_after_turn(handler, user_input, &final_text)
            .await;
        self.record_turn_solve_outcome(&final_text);
        self.refresh_meta_patterns_with_model(handler).await;
        self.persist_session_with_handler(handler)?;
        self.maybe_generate_session_title(handler, user_input, &final_text)
            .await;
        self.emit_skill_lifecycle_suggestion(
            handler,
            user_input,
            progress.turn_skill_matches,
            progress.turn_tool_names,
            progress.skill_manage_used,
        );
        self.emit_skill_nudge_if_needed(
            handler,
            progress.turn_tool_calls,
            progress.skill_manage_used,
            user_input,
        );
        handler.on_event(AgentEvent::AssistantMessage {
            session_id: self.session.session_id.clone(),
            content: final_text.clone(),
        });
        self.append_archive_event(
            "assistant_message",
            "Assistant replied after tool budget",
            &truncated(final_text.clone(), 320),
        );
        let payload = serde_json::json!({
            "event": "post_llm_call",
            "session_id": self.session.session_id.clone(),
            "provider": self.config.provider_id.clone(),
            "model": self.config.model.clone(),
            "content": final_text.clone(),
        });
        let _ = self.tools.run_hooks("post_llm_call", &payload).await;
        let end_payload = serde_json::json!({
            "event": "on_session_end",
            "session_id": self.session.session_id.clone(),
            "provider": self.config.provider_id.clone(),
            "model": self.config.model.clone(),
            "content": final_text,
        });
        let _ = self.tools.run_hooks("on_session_end", &end_payload).await;

        Ok(end_payload
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    async fn call_tool_with_live_updates(
        &self,
        handler: &mut dyn EventHandler,
        iteration: usize,
        tool_call_id: &str,
        tool_name: &str,
        raw_arguments: &str,
        execution_mode: &str,
        batch_id: Option<&str>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    ) -> String {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let session_id = self.session.session_id.clone();
        let data_dir = self.config.data_dir.clone();
        register_tool_event_sender(&session_id, sender);

        let result = {
            let tool_future = self
                .tools
                .call(tool_name, raw_arguments, &self.tool_context);
            let scoped_future = with_tool_runtime_scope(tool_call_id.to_string(), tool_future);
            tokio::pin!(scoped_future);
            let mut live_output = LiveToolOutputState::default();

            loop {
                tokio::select! {
                    maybe_event = receiver.recv() => {
                        let Some(event) = maybe_event else {
                            continue;
                        };
                        Self::handle_runtime_tool_event(
                            handler,
                            &data_dir,
                            &session_id,
                            iteration,
                            tool_call_id,
                            tool_name,
                            execution_mode,
                            batch_id,
                            batch_index,
                            batch_total,
                            &mut live_output,
                            event,
                        );
                    }
                    result = &mut scoped_future => {
                        while let Ok(event) = receiver.try_recv() {
                            Self::handle_runtime_tool_event(
                                handler,
                                &data_dir,
                                &session_id,
                                iteration,
                                tool_call_id,
                                tool_name,
                                execution_mode,
                                batch_id,
                                batch_index,
                                batch_total,
                                &mut live_output,
                                event,
                            );
                        }
                        break result;
                    }
                }
            }
        };

        clear_tool_event_sender(&session_id);
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_runtime_tool_event(
        handler: &mut dyn EventHandler,
        data_dir: &Path,
        session_id: &str,
        iteration: usize,
        tool_call_id: &str,
        tool_name: &str,
        execution_mode: &str,
        batch_id: Option<&str>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
        live_output: &mut LiveToolOutputState,
        event: ToolRuntimeEvent,
    ) {
        match event {
            ToolRuntimeEvent::Stdout {
                tool_call_id: _,
                chunk,
            } => append_tail_capped(&mut live_output.stdout, &redact_secrets(chunk), 12_000),
            ToolRuntimeEvent::Stderr {
                tool_call_id: _,
                chunk,
            } => append_tail_capped(&mut live_output.stderr, &redact_secrets(chunk), 8_000),
        }

        let detail = live_output.snapshot();
        if detail.is_empty() {
            return;
        }

        let _ = persist_live_tool_snapshot(
            data_dir,
            session_id,
            tool_call_id,
            tool_name,
            &detail,
            execution_mode,
            batch_id,
            batch_index,
            batch_total,
        );
        handler.on_event(AgentEvent::ToolCallDelta {
            session_id: session_id.to_string(),
            iteration,
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            detail_preview: detail,
            execution_mode: execution_mode.to_string(),
            batch_id: batch_id.map(str::to_string),
            batch_index,
            batch_total,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_parallel_runtime_tool_event(
        handler: &mut dyn EventHandler,
        data_dir: &Path,
        session_id: &str,
        iteration: usize,
        execution_mode: &str,
        batch_id: Option<&str>,
        batch_total: Option<usize>,
        tool_runtime_index: &BTreeMap<String, (String, usize)>,
        live_outputs: &mut BTreeMap<String, LiveToolOutputState>,
        event: ToolRuntimeEvent,
    ) {
        let (tool_call_id, is_stderr, chunk) = match event {
            ToolRuntimeEvent::Stdout {
                tool_call_id,
                chunk,
            } => (tool_call_id, false, chunk),
            ToolRuntimeEvent::Stderr {
                tool_call_id,
                chunk,
            } => (tool_call_id, true, chunk),
        };

        let Some((tool_name, batch_index)) = tool_runtime_index.get(&tool_call_id) else {
            return;
        };

        let live_output = live_outputs.entry(tool_call_id.clone()).or_default();
        if is_stderr {
            append_tail_capped(&mut live_output.stderr, &redact_secrets(chunk), 8_000);
        } else {
            append_tail_capped(&mut live_output.stdout, &redact_secrets(chunk), 12_000);
        }

        let detail = live_output.snapshot();
        if detail.is_empty() {
            return;
        }

        let _ = persist_live_tool_snapshot(
            data_dir,
            session_id,
            &tool_call_id,
            tool_name,
            &detail,
            execution_mode,
            batch_id,
            Some(*batch_index),
            batch_total,
        );
        handler.on_event(AgentEvent::ToolCallDelta {
            session_id: session_id.to_string(),
            iteration,
            tool_call_id,
            tool_name: tool_name.clone(),
            detail_preview: detail,
            execution_mode: execution_mode.to_string(),
            batch_id: batch_id.map(str::to_string),
            batch_index: Some(*batch_index),
            batch_total,
        });
    }

    fn latest_user_input(&self) -> Result<String> {
        self.session
            .history
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .map(|message| message.content_text())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("no user message found for current session"))
    }

    async fn request_assistant_message(
        &mut self,
        handler: &mut dyn EventHandler,
        iteration: usize,
        user_input: &str,
        mut messages: Vec<ChatMessage>,
        tool_definitions: &[crate::types::ToolDefinition],
    ) -> Result<ChatMessage> {
        let mut recovery_attempts = 0usize;
        let mut transient_retry_attempts = 0usize;
        let mut runtime = self.select_turn_model_runtime(iteration, user_input, &messages)?;
        if let Some(label) = runtime.routed_label.clone() {
            handler.on_event(AgentEvent::Nudge {
                session_id: self.session.session_id.clone(),
                kind: "routing".to_string(),
                message: label,
            });
        }
        self.persist_debug_context_snapshot(
            "assistant_request",
            Some(iteration),
            None,
            Some(&runtime.model),
            Some(estimate_messages_tokens(&messages)),
            &messages,
            None,
            None,
            tool_definitions.len(),
        );

        loop {
            let mut request_options = self.take_request_options();
            request_options.previous_response_id =
                self.response_continuation_id_for_runtime(&runtime);
            let session_id = self.session.session_id.clone();
            handler.on_event(AgentEvent::ModelRequestStarted {
                session_id: session_id.clone(),
                iteration,
                model: runtime.model.clone(),
                message_count: messages.len(),
                tool_count: tool_definitions.len(),
            });
            let assistant_result = match self
                .runtime_client(&runtime)
                .respond_stream_with_options(
                    &runtime.model,
                    &messages,
                    tool_definitions,
                    request_options.clone(),
                    |delta| {
                        if !delta.is_empty() {
                            handler.on_event(AgentEvent::AssistantDelta {
                                session_id: session_id.clone(),
                                iteration,
                                delta: delta.to_string(),
                            });
                        }
                    },
                )
                .await
            {
                Ok(message) => Ok(message),
                Err(stream_error) => {
                    info!(
                        "streaming request failed, falling back to non-streaming: {stream_error:#}"
                    );
                    self.runtime_client(&runtime)
                        .respond_with_options(
                            &runtime.model,
                            &messages,
                            tool_definitions,
                            request_options,
                        )
                        .await
                }
            };

            match assistant_result {
                Ok(response) => {
                    self.remember_runtime_response(&runtime, &response);
                    let message = response.message;
                    handler.on_event(AgentEvent::ModelRequestFinished {
                        session_id: self.session.session_id.clone(),
                        iteration,
                        model: runtime.model.clone(),
                        tool_call_count: message.tool_calls.as_ref().map(Vec::len).unwrap_or(0),
                        content_preview: truncated(message.content_text(), 240),
                    });
                    return Ok(message);
                }
                Err(error) => {
                    if !runtime.uses_primary {
                        handler.on_event(AgentEvent::Nudge {
                            session_id: self.session.session_id.clone(),
                            kind: "routing".to_string(),
                            message: format!(
                                "smart route `{}` 失败，已自动回退到主模型 `{}`。",
                                runtime.model, self.config.model
                            ),
                        });
                        runtime = self.primary_turn_model_runtime();
                        transient_retry_attempts = 0;
                        continue;
                    }
                    if recovery_attempts >= 2
                        || !self
                            .recover_from_model_error(
                                handler,
                                &error,
                                &mut transient_retry_attempts,
                                &runtime,
                            )
                            .await?
                    {
                        return Err(error);
                    }
                    messages = self.rebuild_retry_messages(&messages, user_input);
                    self.persist_debug_context_snapshot(
                        "assistant_request_retry",
                        Some(iteration),
                        None,
                        Some(&runtime.model),
                        Some(estimate_messages_tokens(&messages)),
                        &messages,
                        None,
                        None,
                        tool_definitions.len(),
                    );
                    recovery_attempts += 1;
                }
            }
        }
    }

    async fn recover_from_model_error(
        &mut self,
        handler: &mut dyn EventHandler,
        error: &anyhow::Error,
        transient_retry_attempts: &mut usize,
        runtime: &TurnModelRuntime,
    ) -> Result<bool> {
        let error_text = error.to_string();

        if let Some(available_tokens) = parse_available_output_tokens_from_error(&error_text) {
            let safe_tokens = available_tokens.saturating_sub(64).max(1);
            if self.ephemeral_max_output_tokens == Some(safe_tokens) {
                return Ok(false);
            }
            self.ephemeral_max_output_tokens = Some(safe_tokens);
            handler.on_event(AgentEvent::Nudge {
                session_id: self.session.session_id.clone(),
                kind: "context".to_string(),
                message: format!(
                    "检测到输出上限不足，已自动将本轮输出上限下调到 {safe_tokens} tokens 并重试。"
                ),
            });
            return Ok(true);
        }

        if !is_context_overflow_error(&error_text) {
            if let Some(kind) = classify_retryable_error(&error_text) {
                if *transient_retry_attempts >= 2 {
                    return Ok(false);
                }
                *transient_retry_attempts += 1;
                let base_ms = match kind {
                    RetryableErrorKind::RateLimit | RetryableErrorKind::Overloaded => 1_200,
                    RetryableErrorKind::Server | RetryableErrorKind::Timeout => 350,
                };
                let max_ms = match kind {
                    RetryableErrorKind::RateLimit | RetryableErrorKind::Overloaded => 5_000,
                    RetryableErrorKind::Server | RetryableErrorKind::Timeout => 2_000,
                };
                let delay_ms = parse_retry_after_ms(&error_text)
                    .unwrap_or_else(|| {
                        jittered_backoff_ms(*transient_retry_attempts, base_ms, max_ms)
                    })
                    .min(max_ms);
                let reason = match kind {
                    RetryableErrorKind::RateLimit => "请求限流",
                    RetryableErrorKind::Overloaded => "服务过载",
                    RetryableErrorKind::Server => "服务异常",
                    RetryableErrorKind::Timeout => "网络超时",
                };
                handler.on_event(AgentEvent::Nudge {
                    session_id: self.session.session_id.clone(),
                    kind: "retry".to_string(),
                    message: format!(
                        "检测到{reason}，将在 {delay_ms} ms 后自动重试（第 {} 次）。",
                        transient_retry_attempts
                    ),
                });
                sleep(Duration::from_millis(delay_ms)).await;
                return Ok(true);
            }
            return Ok(false);
        }

        if let Some(limit) = parse_context_limit_from_error(&error_text) {
            let previous = self.context_compressor.context_length();
            self.context_compressor.apply_context_limit(limit);
            if self.context_compressor.context_length() < previous {
                let _ = save_context_length(
                    &self.config.data_dir,
                    &runtime.model,
                    &runtime.base_url,
                    self.context_compressor.context_length(),
                );
                handler.on_event(AgentEvent::Nudge {
                    session_id: self.session.session_id.clone(),
                    kind: "context".to_string(),
                    message: format!(
                        "检测到模型真实上下文上限约为 {limit} tokens，已自动收紧压缩预算并记住该值。"
                    ),
                });
            }
        }

        let background_client = self.background_client();
        let background_model = self.background_model();
        match self
            .context_compressor
            .force_compress_session(&mut self.session, &background_client, &background_model)
            .await
        {
            Ok(Some(outcome)) => {
                self.persist_session_with_handler(handler)?;
                self.emit_context_compaction_nudge(handler, outcome, Some("请求超限后已自动压缩"));
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(compression_error) => {
                info!("forced context compression failed after model error: {compression_error:#}");
                Ok(false)
            }
        }
    }

    fn take_request_options(&mut self) -> RequestOptions {
        RequestOptions {
            max_output_tokens: self.ephemeral_max_output_tokens.take(),
            previous_response_id: None,
        }
    }

    fn runtime_client<'a>(&'a self, runtime: &'a TurnModelRuntime) -> &'a OpenAiCompatClient {
        &runtime.client
    }

    fn runtime_response_key(&self, runtime: &TurnModelRuntime) -> Option<String> {
        (runtime.api_mode == ApiMode::Responses).then(|| {
            format!(
                "{}|{}",
                runtime.base_url.trim_end_matches('/'),
                runtime.model
            )
        })
    }

    fn response_continuation_id_for_runtime(&self, runtime: &TurnModelRuntime) -> Option<String> {
        if !supports_response_continuation(&runtime.base_url) {
            return None;
        }
        let runtime_key = self.runtime_response_key(runtime)?;
        if self.session.latest_response_runtime_key.as_deref() == Some(runtime_key.as_str()) {
            if self.session.latest_response_prefix_digest.as_deref()
                == continuation_prefix_digest(&self.session.history).as_deref()
            {
                return self.session.latest_response_id.clone();
            }
        }
        None
    }

    fn remember_runtime_response(&mut self, runtime: &TurnModelRuntime, response: &ModelResponse) {
        let runtime_key = self.runtime_response_key(runtime);
        let prefix_digest = continuation_prefix_digest(&self.session.history);
        self.session.set_latest_response_state(
            response.response_id.clone(),
            runtime_key,
            prefix_digest,
        );
    }

    fn primary_turn_model_runtime(&self) -> TurnModelRuntime {
        TurnModelRuntime {
            client: self.client.clone(),
            model: self.config.model.clone(),
            base_url: self.config.base_url.clone(),
            api_mode: self.config.api_mode,
            routed_label: None,
            uses_primary: true,
        }
    }

    fn request_context_budget_tokens(&self) -> usize {
        self.context_compressor
            .context_length()
            .saturating_mul(REQUEST_CONTEXT_BUDGET_NUMERATOR)
            / REQUEST_CONTEXT_BUDGET_DENOMINATOR
    }

    fn emit_request_context_pressure_nudge(
        &self,
        handler: &mut dyn EventHandler,
        projected_tokens: usize,
    ) {
        let budget_tokens = self.request_context_budget_tokens();
        if projected_tokens < budget_tokens.saturating_mul(9) / 10 {
            return;
        }

        let severity = if projected_tokens > budget_tokens {
            "已超过"
        } else {
            "接近"
        };
        handler.on_event(AgentEvent::Nudge {
            session_id: self.session.session_id.clone(),
            kind: "context".to_string(),
            message: format!(
                "本次请求预计占用约 {} tokens，{}发送预算 {} tokens。",
                projected_tokens, severity, budget_tokens
            ),
        });
    }

    fn persist_debug_context_snapshot(
        &self,
        phase: &str,
        iteration: Option<usize>,
        purpose: Option<&str>,
        model: Option<&str>,
        projected_tokens: Option<usize>,
        messages: &[ChatMessage],
        injections: Option<&[ContextInjection]>,
        injection_stats: Option<&ContextInjectionStats>,
        tool_count: usize,
    ) {
        if !self.config.debug_context {
            return;
        }

        let dir = self
            .config
            .data_dir
            .join("runtime")
            .join("context-debug")
            .join(&self.session.session_id);
        if let Err(error) = fs::create_dir_all(&dir) {
            warn!(
                "failed to create context debug dir {}: {error:#}",
                dir.display()
            );
            return;
        }

        let timestamp = unix_now_ms();
        let mut file_name = format!("{timestamp}-{phase}");
        if let Some(iteration) = iteration {
            file_name.push_str(&format!("-iter{iteration}"));
        }
        if let Some(purpose) = purpose {
            file_name.push_str(&format!("-{}", sanitize_debug_file_component(purpose)));
        }
        file_name.push_str(".json");
        let path = dir.join(file_name);

        let injections_json = injections.map(|items| {
            items
                .iter()
                .map(|item| {
                    json!({
                        "label": item.label,
                        "max_chars": item.max_chars,
                        "content_chars": item.content.chars().count(),
                        "preview": truncated(item.content.clone(), 240),
                    })
                })
                .collect::<Vec<_>>()
        });
        let injection_stats_json = injection_stats.map(|stats| {
            json!({
                "total_blocks": stats.total_blocks,
                "kept_blocks": stats.kept_blocks,
                "original_chars": stats.original_chars,
                "final_chars": stats.final_chars,
                "clipped_labels": stats.clipped_labels,
                "skipped_labels": stats.skipped_labels,
            })
        });
        let message_debug = messages
            .iter()
            .enumerate()
            .map(|(index, message)| {
                json!({
                    "index": index,
                    "role": message.role,
                    "chars": message.content_text().chars().count(),
                    "tool_call_id": message.tool_call_id,
                    "tool_call_count": message.tool_calls.as_ref().map(Vec::len).unwrap_or(0),
                    "content_preview": truncated(message.content_text(), 240),
                    "message": message,
                })
            })
            .collect::<Vec<_>>();
        let snapshot = json!({
            "session_id": self.session.session_id,
            "phase": phase,
            "iteration": iteration,
            "purpose": purpose,
            "model": model,
            "tool_count": tool_count,
            "projected_tokens": projected_tokens,
            "request_budget_tokens": self.request_context_budget_tokens(),
            "message_count": messages.len(),
            "messages": message_debug,
            "injections": injections_json,
            "injection_stats": injection_stats_json,
            "history_message_count": self.session.history.len(),
            "history_user_turn_limit": PROMPT_HISTORY_MAX_USER_TURNS,
        });

        let raw = match serde_json::to_string_pretty(&snapshot) {
            Ok(raw) => raw,
            Err(error) => {
                warn!("failed to serialize context debug snapshot: {error:#}");
                return;
            }
        };
        if let Err(error) = fs::write(&path, raw) {
            warn!(
                "failed to write context debug snapshot {}: {error:#}",
                path.display()
            );
        }
    }

    fn select_turn_model_runtime(
        &self,
        _iteration: usize,
        user_input: &str,
        messages: &[ChatMessage],
    ) -> Result<TurnModelRuntime> {
        if messages.iter().any(|message| {
            message.role == "tool"
                || message
                    .tool_calls
                    .as_ref()
                    .map(|tool_calls| !tool_calls.is_empty())
                    .unwrap_or(false)
        }) {
            return Ok(self.primary_turn_model_runtime());
        }

        let Some(route) = resolve_turn_route(
            &self.config.data_dir,
            user_input,
            self.config.smart_model_routing.as_ref(),
        )?
        else {
            return Ok(self.primary_turn_model_runtime());
        };

        let client = OpenAiCompatClient::new(
            route.runtime.base_url.clone(),
            route.runtime.api_key.clone(),
            route.runtime.api_mode,
        )?;
        Ok(TurnModelRuntime {
            client,
            model: route.runtime.model.clone(),
            base_url: route.runtime.base_url.clone(),
            api_mode: route.runtime.api_mode,
            routed_label: Some(format!(
                "simple turn 已切到 `{}` ({})。",
                route.runtime.model, route.runtime.id
            )),
            uses_primary: false,
        })
    }

    fn background_client(&self) -> OpenAiCompatClient {
        self.auxiliary_client
            .clone()
            .unwrap_or_else(|| self.client.clone())
    }

    fn background_model(&self) -> String {
        self.config
            .auxiliary_model
            .as_ref()
            .map(|auxiliary| auxiliary.model.clone())
            .unwrap_or_else(|| self.config.model.clone())
    }

    fn rebuild_retry_messages(
        &self,
        previous_messages: &[ChatMessage],
        user_input: &str,
    ) -> Vec<ChatMessage> {
        let system_prompt = previous_messages
            .first()
            .filter(|message| message.role == "system")
            .map(ChatMessage::content_text)
            .unwrap_or_else(|| build_system_prompt(&self.config));
        let injected_suffix = previous_messages
            .last()
            .filter(|message| message.role == "user")
            .and_then(|message| {
                message
                    .content_text()
                    .strip_prefix(user_input)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let injections = if injected_suffix.is_empty() {
            Vec::new()
        } else {
            vec![ContextInjection {
                label: "retry_context",
                content: injected_suffix.trim().to_string(),
                max_chars: injected_suffix.len().max(CONTEXT_INJECTION_MIN_BLOCK_CHARS),
            }]
        };
        let (rebuilt, _) = assemble_turn_messages(
            &system_prompt,
            &self.session.history,
            &injections,
            usize::MAX,
        );

        rebuilt
    }

    fn emit_context_compaction_nudge(
        &self,
        handler: &mut dyn EventHandler,
        outcome: crate::context_compression::ContextCompressionOutcome,
        prefix: Option<&str>,
    ) {
        let action = if outcome.used_summary {
            "长会话已压缩"
        } else {
            "旧上下文已整理"
        };
        let mut message = match prefix {
            Some(prefix) => format!(
                "{prefix}，{action}，历史消息 {} -> {}，估算上下文 {} -> {} tokens。",
                outcome.original_message_count,
                outcome.compressed_message_count,
                outcome.original_estimated_tokens,
                outcome.compressed_estimated_tokens
            ),
            None => format!(
                "{action}，历史消息 {} -> {}，估算上下文 {} -> {} tokens。",
                outcome.original_message_count,
                outcome.compressed_message_count,
                outcome.original_estimated_tokens,
                outcome.compressed_estimated_tokens
            ),
        };
        if outcome.pruned_tool_messages > 0 {
            message.push_str(&format!(
                " 已裁剪 {} 条旧工具输出。",
                outcome.pruned_tool_messages
            ));
        }
        handler.on_event(AgentEvent::Nudge {
            session_id: self.session.session_id.clone(),
            kind: "context".to_string(),
            message,
        });
    }

    fn check_stop_requested(&self, handler: &mut dyn EventHandler) -> Result<()> {
        if !stop_requested(&self.config.data_dir, &self.session.session_id) {
            return Ok(());
        }
        handler.on_event(AgentEvent::Error {
            session_id: self.session.session_id.clone(),
            message: "stop requested for current session".to_string(),
        });
        bail!("stop requested for current session");
    }

    fn emit_skill_nudge_if_needed(
        &self,
        handler: &mut dyn EventHandler,
        turn_tool_calls: usize,
        skill_manage_used: bool,
        user_input: &str,
    ) {
        if turn_tool_calls < 5 || skill_manage_used {
            return;
        }
        handler.on_event(AgentEvent::Nudge {
            session_id: self.session.session_id.clone(),
            kind: "skills".to_string(),
            message: format!(
                "This turn used {turn_tool_calls} tool calls. If the workflow is reusable, save it as a skill with `skill_manage`. User task: {}",
                truncated(user_input, 160)
            ),
        });
    }

    fn emit_skill_lifecycle_suggestion(
        &self,
        handler: &mut dyn EventHandler,
        user_input: &str,
        matched_skills: Vec<crate::skills::SkillMatch>,
        tool_names_used: Vec<String>,
        skill_manage_used: bool,
    ) {
        let Some(suggestion) = suggest_skill_lifecycle(&SkillAdviceInput {
            user_input: user_input.to_string(),
            matched_skills,
            tool_names_used,
            skill_manage_used,
            shell_enabled: self.tool_context.shell_enabled,
        }) else {
            return;
        };

        handler.on_event(AgentEvent::SkillLifecycleSuggested {
            session_id: self.session.session_id.clone(),
            action: suggestion.action.as_str().to_string(),
            category: suggestion.category,
            name: suggestion.name,
            description: suggestion.description,
            keywords: suggestion.keywords,
            task_kinds: suggestion.task_kinds,
            requires_tools: suggestion.requires_tools,
            requires_shell: suggestion.requires_shell,
            reason: suggestion.reason,
        });
    }

    async fn execute_tool_calls_parallel(
        &mut self,
        tool_calls: Vec<crate::types::ToolCall>,
        iteration: usize,
        handler: &mut dyn EventHandler,
        progress: &mut TurnProgress,
    ) -> Result<Option<String>> {
        self.check_stop_requested(handler)?;
        let batch_total = tool_calls.len();
        let batch_id = format!(
            "parallel-{iteration}-{}",
            tool_calls
                .first()
                .map(|call| call.id.as_str())
                .unwrap_or("batch")
        );
        let batch_started_at = Instant::now();

        handler.on_event(AgentEvent::ToolBatchStarted {
            session_id: self.session.session_id.clone(),
            iteration,
            batch_id: batch_id.clone(),
            total_calls: batch_total,
        });
        self.session.record_batch_timeline_entry(
            batch_id.clone(),
            iteration,
            batch_total,
            0,
            StoredBatchStatus::Running,
        );

        for (index, call) in tool_calls.iter().enumerate() {
            progress.turn_tool_calls += 1;
            progress.turn_tool_names.push(call.function.name.clone());
            if call.function.name == "skill_manage" {
                progress.skill_manage_used = true;
            }
            info!("executing tool {} (parallel batch)", call.function.name);
            self.session.record_tool_timeline_entry(
                call.id.clone(),
                call.function.name.clone(),
                truncated(call.function.arguments.clone(), 300),
                StoredToolPhase::Running,
                Some("parallel"),
                Some(&batch_id),
                Some(index + 1),
                Some(batch_total),
            );
            self.upsert_archive_tool_call(
                &call.id,
                &call.function.name,
                "parallel",
                Some(&batch_id),
                Some(index + 1),
                Some(batch_total),
                "running",
                Some(&call.function.arguments),
                None,
            );
            handler.on_event(AgentEvent::ToolCallStarted {
                session_id: self.session.session_id.clone(),
                iteration,
                tool_call_id: call.id.clone(),
                tool_name: call.function.name.clone(),
                arguments_preview: redacted_truncated(call.function.arguments.clone(), 300),
                execution_mode: "parallel".to_string(),
                batch_id: Some(batch_id.clone()),
                batch_index: Some(index + 1),
                batch_total: Some(batch_total),
            });
        }

        let session_id = self.session.session_id.clone();
        let data_dir = self.config.data_dir.clone();
        let tool_runtime_index = tool_calls
            .iter()
            .enumerate()
            .map(|(index, call)| (call.id.clone(), (call.function.name.clone(), index + 1)))
            .collect::<BTreeMap<_, _>>();
        let results = {
            let (sender, mut receiver) = mpsc::unbounded_channel();
            register_tool_event_sender(&session_id, sender);
            let tools = &self.tools;
            let tool_context = &self.tool_context;
            let futures = tool_calls
                .iter()
                .map(|call| {
                    let tool_started_at = Instant::now();
                    let tool_call_id = call.id.clone();
                    let tool_name = &call.function.name;
                    let raw_arguments = &call.function.arguments;
                    async move {
                        let result = with_tool_runtime_scope(
                            tool_call_id,
                            tools.call(tool_name, raw_arguments, tool_context),
                        )
                        .await;
                        (result, elapsed_ms(tool_started_at))
                    }
                })
                .collect::<Vec<_>>();
            let all_futures = join_all(futures);
            tokio::pin!(all_futures);
            let mut live_outputs = BTreeMap::<String, LiveToolOutputState>::new();

            let results = loop {
                tokio::select! {
                    maybe_event = receiver.recv() => {
                        let Some(event) = maybe_event else {
                            continue;
                        };
                        Self::handle_parallel_runtime_tool_event(
                            handler,
                            &data_dir,
                            &session_id,
                            iteration,
                            "parallel",
                            Some(&batch_id),
                            Some(batch_total),
                            &tool_runtime_index,
                            &mut live_outputs,
                            event,
                        );
                    }
                    results = &mut all_futures => {
                        while let Ok(event) = receiver.try_recv() {
                            Self::handle_parallel_runtime_tool_event(
                                handler,
                                &data_dir,
                                &session_id,
                                iteration,
                                "parallel",
                                Some(&batch_id),
                                Some(batch_total),
                                &tool_runtime_index,
                                &mut live_outputs,
                                event,
                            );
                        }
                        break results;
                    }
                }
            };
            clear_tool_event_sender(&session_id);
            results
        };

        let mut completed_calls = 0;
        let mut error_calls = 0;
        self.finish_parallel_batch_if_stop_requested(
            handler,
            iteration,
            &batch_id,
            completed_calls,
            batch_total,
            batch_started_at,
        )?;
        for (index, (call, (result, duration_ms))) in
            tool_calls.into_iter().zip(results.into_iter()).enumerate()
        {
            self.finish_parallel_batch_if_stop_requested(
                handler,
                iteration,
                &batch_id,
                completed_calls,
                batch_total,
                batch_started_at,
            )?;
            let result = match parse_tool_args(&call.function.arguments) {
                Some(args) => match self
                    .subdir_hint_tracker
                    .check_tool_call(&call.function.name, &args)
                {
                    Some(hint) => format!("{result}\n\n{hint}"),
                    None => result,
                },
                None => result,
            };
            let result = redact_secrets(result);

            if let Some(approval) = parse_approval_required(&result) {
                self.session.record_tool_timeline_entry(
                    call.id.clone(),
                    call.function.name.clone(),
                    approval.reason.clone(),
                    StoredToolPhase::Approval,
                    Some("parallel"),
                    Some(&batch_id),
                    Some(index + 1),
                    Some(batch_total),
                );
                self.session.record_approval_timeline_entry(
                    approval.approval_id.clone(),
                    call.function.name.clone(),
                    approval.reason.clone(),
                    approval.command.clone(),
                    Some("parallel"),
                    Some(&batch_id),
                    Some(index + 1),
                    Some(batch_total),
                );
                save_pending_approval(
                    &self.config.data_dir,
                    &approval.approval_id,
                    &self.session.session_id,
                    &call.id,
                    &call.function.name,
                    "parallel",
                    Some(&batch_id),
                    Some(index + 1),
                    Some(batch_total),
                    &call.function.arguments,
                    &approval.command,
                )?;
                self.upsert_archive_tool_call(
                    &call.id,
                    &call.function.name,
                    "parallel",
                    Some(&batch_id),
                    Some(index + 1),
                    Some(batch_total),
                    "approval",
                    Some(&call.function.arguments),
                    Some(&approval.reason),
                );
                self.upsert_archive_approval(
                    &approval.approval_id,
                    &call.id,
                    &call.function.name,
                    &approval.reason,
                    &approval.command,
                    "parallel",
                    Some(&batch_id),
                    Some(index + 1),
                    Some(batch_total),
                );
                let approval_id = approval.approval_id.clone();
                let approval_reason = approval.reason.clone();
                let approval_command = approval.command.clone();
                handler.on_event(AgentEvent::ApprovalRequired {
                    session_id: self.session.session_id.clone(),
                    tool_call_id: call.id.clone(),
                    tool_name: call.function.name.clone(),
                    approval_id,
                    reason: approval_reason.clone(),
                    command: approval_command,
                    execution_mode: "parallel".to_string(),
                    batch_id: Some(batch_id.clone()),
                    batch_index: Some(index + 1),
                    batch_total: Some(batch_total),
                });
                self.append_archive_event(
                    "approval_required",
                    "Tool approval required",
                    &truncated(approval_reason, 240),
                );
                self.update_goal_state_from_tool_outcome(
                    &call.id,
                    &call.function.name,
                    &approval.reason,
                    "parallel",
                    "approval_required",
                );
                self.reconcile_goal_state_after_tool_outcome(
                    handler,
                    &call.id,
                    &call.function.name,
                    &approval.reason,
                    "parallel",
                    "approval_required",
                )
                .await;
                handler.on_event(AgentEvent::ToolBatchFinished {
                    session_id: self.session.session_id.clone(),
                    iteration,
                    batch_id: batch_id.clone(),
                    completed_calls,
                    total_calls: batch_total,
                    status: "awaiting_approval".to_string(),
                    duration_ms: elapsed_ms(batch_started_at),
                });
                self.session.record_batch_timeline_entry(
                    batch_id.clone(),
                    iteration,
                    batch_total,
                    completed_calls,
                    StoredBatchStatus::AwaitingApproval,
                );
                self.persist_session_with_handler(handler)?;
                return Ok(Some(APPROVAL_PENDING_RESPONSE.to_string()));
            }

            let tool_status = classify_tool_result_status(&result);
            if tool_status == "error" {
                error_calls += 1;
            }
            self.session.record_tool_timeline_entry(
                call.id.clone(),
                call.function.name.clone(),
                result.clone(),
                tool_status_to_phase(tool_status),
                Some("parallel"),
                Some(&batch_id),
                Some(index + 1),
                Some(batch_total),
            );
            self.upsert_archive_tool_call(
                &call.id,
                &call.function.name,
                "parallel",
                Some(&batch_id),
                Some(index + 1),
                Some(batch_total),
                tool_status,
                Some(&call.function.arguments),
                Some(&result),
            );
            self.update_goal_state_from_tool_outcome(
                &call.id,
                &call.function.name,
                &result,
                "parallel",
                tool_status,
            );
            let reconcile_summary = self
                .reconcile_goal_state_after_tool_outcome(
                    handler,
                    &call.id,
                    &call.function.name,
                    &result,
                    "parallel",
                    tool_status,
                )
                .await;
            let tool_name = call.function.name.clone();
            handler.on_event(AgentEvent::ToolCallFinished {
                session_id: self.session.session_id.clone(),
                iteration,
                tool_call_id: call.id.clone(),
                tool_name: tool_name.clone(),
                status: tool_status.to_string(),
                duration_ms,
                output_preview: redacted_truncated(result.clone(), 500),
                execution_mode: "parallel".to_string(),
                batch_id: Some(batch_id.clone()),
                batch_index: Some(index + 1),
                batch_total: Some(batch_total),
            });
            self.append_archive_message("tool", &result, Some(&call.id), None);
            self.session.history.push(ChatMessage::tool(
                call.id,
                summarize_tool_result_for_history(
                    &tool_name,
                    &result,
                    tool_status,
                    reconcile_summary.as_deref(),
                ),
            ));
            completed_calls += 1;
            self.session.record_batch_timeline_entry(
                batch_id.clone(),
                iteration,
                batch_total,
                completed_calls,
                StoredBatchStatus::Running,
            );
            self.persist_session_with_handler(handler)?;
            handler.on_event(AgentEvent::ToolBatchProgress {
                session_id: self.session.session_id.clone(),
                iteration,
                batch_id: batch_id.clone(),
                completed_calls,
                total_calls: batch_total,
            });
        }

        self.finish_parallel_batch_if_stop_requested(
            handler,
            iteration,
            &batch_id,
            completed_calls,
            batch_total,
            batch_started_at,
        )?;
        let batch_status = if error_calls > 0 {
            "completed_with_errors"
        } else {
            "completed"
        };
        handler.on_event(AgentEvent::ToolBatchFinished {
            session_id: self.session.session_id.clone(),
            iteration,
            batch_id: batch_id.clone(),
            completed_calls,
            total_calls: batch_total,
            status: batch_status.to_string(),
            duration_ms: elapsed_ms(batch_started_at),
        });
        self.session.record_batch_timeline_entry(
            batch_id,
            iteration,
            batch_total,
            completed_calls,
            if error_calls > 0 {
                StoredBatchStatus::CompletedWithErrors
            } else {
                StoredBatchStatus::Completed
            },
        );
        let _ = self.persist_session();

        Ok(None)
    }

    fn finish_parallel_batch_if_stop_requested(
        &mut self,
        handler: &mut dyn EventHandler,
        iteration: usize,
        batch_id: &str,
        completed_calls: usize,
        total_calls: usize,
        batch_started_at: Instant,
    ) -> Result<()> {
        if !stop_requested(&self.config.data_dir, &self.session.session_id) {
            return Ok(());
        }

        handler.on_event(AgentEvent::ToolBatchFinished {
            session_id: self.session.session_id.clone(),
            iteration,
            batch_id: batch_id.to_string(),
            completed_calls,
            total_calls,
            status: "canceled".to_string(),
            duration_ms: elapsed_ms(batch_started_at),
        });
        self.session.record_batch_timeline_entry(
            batch_id,
            iteration,
            total_calls,
            completed_calls,
            StoredBatchStatus::Canceled,
        );
        let _ = self.persist_session();
        self.check_stop_requested(handler)
    }

    fn persist_session(&mut self) -> Result<std::path::PathBuf> {
        self.session.touch();
        self.sync_goal_runtime_state();
        let path = self.session_store.save(&self.session)?;
        self.upsert_archive_session();
        self.sync_wiki_views();
        Ok(path)
    }

    fn persist_session_with_handler(&mut self, handler: &mut dyn EventHandler) -> Result<()> {
        let path = self.persist_session()?;
        handler.on_event(AgentEvent::SessionSaved {
            session_id: self.session.session_id.clone(),
            path: path.display().to_string(),
        });
        Ok(())
    }

    async fn maybe_generate_session_title(
        &mut self,
        handler: &mut dyn EventHandler,
        user_input: &str,
        assistant_response: &str,
    ) {
        let user_turns = self
            .session
            .history
            .iter()
            .filter(|message| message.role == "user")
            .count();
        if !should_generate_title(user_turns, self.session.title.as_deref()) {
            return;
        }
        let background_client = self.background_client();
        let background_model = self.background_model();
        handler.on_event(AgentEvent::BackgroundModelRequestStarted {
            session_id: self.session.session_id.clone(),
            purpose: "title_generation".to_string(),
            model: background_model.clone(),
            message_count: 2,
        });
        let title_result = generate_title(
            &background_client,
            &background_model,
            user_input,
            assistant_response,
        )
        .await;
        let Ok(Some(title)) = title_result else {
            handler.on_event(AgentEvent::BackgroundModelRequestFinished {
                session_id: self.session.session_id.clone(),
                purpose: "title_generation".to_string(),
                model: background_model,
                status: "skipped".to_string(),
                content_preview: String::new(),
            });
            return;
        };
        handler.on_event(AgentEvent::BackgroundModelRequestFinished {
            session_id: self.session.session_id.clone(),
            purpose: "title_generation".to_string(),
            model: background_model,
            status: "ok".to_string(),
            content_preview: truncated(title.clone(), 240),
        });
        self.session.title = Some(title);
        let _ = self.persist_session();
    }
}

impl LiveToolOutputState {
    fn snapshot(&self) -> String {
        let mut sections = Vec::new();
        if !self.stdout.is_empty() {
            sections.push(format!("stdout:\n{}", self.stdout));
        }
        if !self.stderr.is_empty() {
            sections.push(format!("stderr:\n{}", self.stderr));
        }
        sections.join("\n\n")
    }
}

#[allow(clippy::too_many_arguments)]
fn persist_live_tool_snapshot(
    data_dir: &Path,
    session_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    detail: &str,
    execution_mode: &str,
    batch_id: Option<&str>,
    batch_index: Option<usize>,
    batch_total: Option<usize>,
) -> Result<()> {
    let store = SessionStore::new(data_dir.to_path_buf())?;
    let Some(mut session) = store.load(session_id)? else {
        return Ok(());
    };
    session.record_tool_timeline_entry(
        tool_call_id.to_string(),
        tool_name.to_string(),
        detail.to_string(),
        StoredToolPhase::Running,
        Some(execution_mode),
        batch_id,
        batch_index,
        batch_total,
    );
    session.touch();
    store.save(&session)?;
    Ok(())
}

fn append_tail_capped(target: &mut String, delta: &str, max_chars: usize) {
    target.push_str(delta);
    let count = target.chars().count();
    if count <= max_chars {
        return;
    }
    let tail = target.chars().skip(count - max_chars).collect::<String>();
    *target = format!("...\n{tail}");
}

const PARALLEL_SAFE_READ_TOOLS: &[&str] = &[
    "git_diff",
    "git_log",
    "git_status",
    "list_files",
    "session_search",
    "skill_view",
    "skills_list",
];

const PARALLEL_PATH_SCOPED_TOOLS: &[&str] = &[
    "delete_file",
    "move_file",
    "patch_file",
    "read_file",
    "search_files",
    "write_file",
];

fn should_parallelize_tool_batch(tool_calls: &[crate::types::ToolCall], ctx: &ToolContext) -> bool {
    if tool_calls.len() <= 1 {
        return false;
    }

    let workspace_root = ctx
        .workspace_root
        .canonicalize()
        .unwrap_or_else(|_| ctx.workspace_root.clone());
    let mut reserved_paths: Vec<PathBuf> = Vec::new();

    for call in tool_calls {
        let tool_name = call.function.name.as_str();
        let Some(arguments) = parse_tool_args(&call.function.arguments) else {
            return false;
        };

        if PARALLEL_PATH_SCOPED_TOOLS.contains(&tool_name) {
            let Some(paths) = parallel_scope_paths(tool_name, &arguments, &workspace_root) else {
                return false;
            };
            if paths.is_empty() {
                return false;
            }
            for (index, path) in paths.iter().enumerate() {
                if paths
                    .iter()
                    .skip(index + 1)
                    .any(|other| paths_overlap(path, other))
                {
                    return false;
                }
                if reserved_paths
                    .iter()
                    .any(|existing| paths_overlap(path, existing))
                {
                    return false;
                }
            }
            reserved_paths.extend(paths);
            continue;
        }

        if !PARALLEL_SAFE_READ_TOOLS.contains(&tool_name) {
            return false;
        }
    }

    true
}

fn parallel_scope_paths(
    tool_name: &str,
    arguments: &Value,
    workspace_root: &Path,
) -> Option<Vec<PathBuf>> {
    match tool_name {
        "delete_file" | "patch_file" | "read_file" | "search_files" | "write_file" => {
            let path = value_as_str(arguments, "path")?;
            Some(vec![normalize_parallel_scope_path(workspace_root, path)?])
        }
        "move_file" => {
            let source = value_as_str(arguments, "source_path")?;
            let destination = value_as_str(arguments, "destination_path")?;
            Some(vec![
                normalize_parallel_scope_path(workspace_root, source)?,
                normalize_parallel_scope_path(workspace_root, destination)?,
            ])
        }
        _ => None,
    }
}

fn value_as_str<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn normalize_parallel_scope_path(workspace_root: &Path, raw_path: &str) -> Option<PathBuf> {
    let candidate = if Path::new(raw_path).is_absolute() {
        PathBuf::from(raw_path)
    } else {
        workspace_root.join(raw_path)
    };
    let normalized = crate::tools::normalize_path(&candidate);
    normalized.starts_with(workspace_root).then_some(normalized)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    let left_parts = left.components().collect::<Vec<_>>();
    let right_parts = right.components().collect::<Vec<_>>();
    let common_len = left_parts.len().min(right_parts.len());
    left_parts[..common_len] == right_parts[..common_len]
}

fn sync_todos_from_goal_state(state: &crate::goal_state::GoalState, todos: &mut Vec<TodoItem>) {
    let active_goals = state
        .goals
        .iter()
        .filter(|goal| matches!(goal.level.as_str(), "current" | "subgoal"))
        .collect::<Vec<_>>();

    for goal in &active_goals {
        let todo_id = format!("goal:{}", goal.id);
        let content = format!("Goal: {}", goal.title);
        let status = map_goal_status_to_todo_status(&goal.status);
        if let Some(todo) = todos.iter_mut().find(|todo| todo.id == todo_id) {
            todo.content = content.clone();
            todo.status = status.to_string();
            todo.normalize();
        } else {
            todos.push(TodoItem::new(todo_id, content, status));
        }
    }

    let active_goal_ids = active_goals
        .iter()
        .map(|goal| format!("goal:{}", goal.id))
        .collect::<BTreeSet<_>>();
    for todo in todos.iter_mut().filter(|todo| todo.id.starts_with("goal:")) {
        if active_goal_ids.contains(&todo.id) {
            continue;
        }
        if todo.status == "in_progress" || todo.status == "pending" || todo.status == "blocked" {
            todo.status = "cancelled".to_string();
            todo.normalize();
        }
    }
}

fn sync_todos_from_delegate_worker(
    tool_call_id: &str,
    observation: &DelegateWorkerObservation,
    todos: &mut Vec<TodoItem>,
) {
    let mut active_step_ids = BTreeSet::new();

    for (idx, step) in observation.payload.step_updates.iter().enumerate() {
        let todo_id = delegate_worker_step_todo_id(tool_call_id, step, idx);
        let content = render_delegate_worker_todo_content(&observation.objective, step);
        let status = map_delegate_worker_step_status(step.status.as_deref());
        active_step_ids.insert(todo_id.clone());

        if let Some(todo) = todos.iter_mut().find(|todo| todo.id == todo_id) {
            todo.content = content.clone();
            todo.status = status.to_string();
            todo.normalize();
        } else {
            todos.push(TodoItem::new(todo_id, content, status));
        }
    }

    let worker_prefix = format!("worker:{tool_call_id}:");
    for todo in todos
        .iter_mut()
        .filter(|todo| todo.id.starts_with(&worker_prefix))
    {
        if active_step_ids.contains(&todo.id) {
            continue;
        }
        if todo.is_active() {
            todo.status = "cancelled".to_string();
            todo.normalize();
        }
    }
}

fn delegate_worker_step_todo_id(
    tool_call_id: &str,
    step: &DelegateWorkerStepUpdate,
    index: usize,
) -> String {
    let step_key = step
        .step_id
        .as_deref()
        .map(normalize_delegate_step_key)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("step-{}", index + 1));
    format!("worker:{tool_call_id}:{step_key}")
}

fn normalize_delegate_step_key(value: &str) -> String {
    let mut key = String::with_capacity(value.len());
    let mut previous_dash = false;
    for ch in value.chars() {
        let normalized = match ch {
            'a'..='z' | '0'..='9' => Some(ch),
            'A'..='Z' => Some(ch.to_ascii_lowercase()),
            _ => Some('-'),
        };
        if let Some(ch) = normalized {
            if ch == '-' {
                if previous_dash {
                    continue;
                }
                previous_dash = true;
            } else {
                previous_dash = false;
            }
            key.push(ch);
        }
    }
    key.trim_matches('-').to_string()
}

fn render_delegate_worker_todo_content(objective: &str, step: &DelegateWorkerStepUpdate) -> String {
    let step_id = step.step_id.as_deref().unwrap_or("step");
    match step.summary.as_deref().filter(|value| !value.is_empty()) {
        Some(summary) => format!(
            "Worker `{}`: {} - {}",
            inline_clip(objective, 48),
            inline_clip(step_id, 40),
            inline_clip(summary, 100)
        ),
        None => format!(
            "Worker `{}`: {}",
            inline_clip(objective, 48),
            inline_clip(step_id, 80)
        ),
    }
}

fn map_delegate_worker_step_status(status: Option<&str>) -> &'static str {
    match status
        .unwrap_or("pending")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "pending" => "pending",
        "in_progress" | "running" | "active" => "in_progress",
        "blocked" | "failed" | "error" => "blocked",
        "completed" | "complete" | "done" | "succeeded" => "completed",
        "cancelled" | "canceled" => "cancelled",
        _ => "pending",
    }
}

fn map_tool_status_to_solve_step_status(status: &str) -> &'static str {
    match status {
        "approval_required" | "awaiting_approval" => "blocked",
        "failed" | "error" => "blocked",
        "done" | "completed" => "completed",
        _ => "in_progress",
    }
}

fn classify_tool_result_status(result: &str) -> &'static str {
    if tool_result_indicates_error(result) {
        "error"
    } else {
        "done"
    }
}

fn tool_status_to_phase(status: &str) -> StoredToolPhase {
    match status {
        "error" | "failed" => StoredToolPhase::Error,
        "approval_required" | "awaiting_approval" => StoredToolPhase::Approval,
        _ => StoredToolPhase::Done,
    }
}

fn tool_result_indicates_error(result: &str) -> bool {
    let lowered = result.trim_start().to_ascii_lowercase();
    if lowered.starts_with("tool_error:") || lowered.starts_with("approval denied") {
        return true;
    }
    for line in result.lines() {
        let normalized = line.trim().to_ascii_lowercase();
        if let Some(value) = normalized.strip_prefix("status:") {
            if matches!(
                value.trim(),
                "timeout" | "canceled" | "cancelled" | "failed" | "error"
            ) {
                return true;
            }
        }
        if let Some(value) = normalized.strip_prefix("exit_code:") {
            if value
                .trim()
                .parse::<i32>()
                .is_ok_and(|exit_code| exit_code != 0)
            {
                return true;
            }
        }
    }
    false
}

fn map_goal_status_to_todo_status(status: &str) -> &'static str {
    match status {
        "pending" => "pending",
        "in_progress" => "in_progress",
        "blocked" => "blocked",
        "succeeded" => "completed",
        "failed" | "transferred" | "cancelled" => "cancelled",
        _ => "pending",
    }
}

fn map_goal_status_to_episode_status(status: &str) -> &'static str {
    match status {
        "succeeded" => "completed",
        "blocked" => "blocked",
        "failed" => "failed",
        "cancelled" | "transferred" => "cancelled",
        _ => "in_progress",
    }
}

fn summarize_active_todos_for_goal_loop(todos: &[TodoItem]) -> Option<String> {
    let active = todos
        .iter()
        .filter(|todo| matches!(todo.status.as_str(), "pending" | "in_progress" | "blocked"))
        .collect::<Vec<_>>();
    if active.is_empty() {
        return None;
    }

    Some(
        active
            .into_iter()
            .take(6)
            .map(|todo| format!("{} [{}]", inline_clip(&todo.content, 80), todo.status))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

fn merge_cognition(items: &mut Vec<CognitionItem>, incoming: CognitionItem) {
    if let Some(existing) = items.iter_mut().find(|item| item.id == incoming.id) {
        *existing = incoming;
    } else {
        items.push(incoming);
    }
}

fn merge_hot_data(items: &mut Vec<HotDataItem>, incoming: HotDataItem) {
    if let Some(existing) = items.iter_mut().find(|item| item.id == incoming.id) {
        *existing = incoming;
    } else {
        items.push(incoming);
    }
}

fn evidence_item(source_type: &str, source: &str, summary: &str, relation: &str) -> EvidenceItem {
    EvidenceItem {
        source_type: source_type.to_string(),
        source: source.to_string(),
        summary: summary.to_string(),
        relation: relation.to_string(),
        observed_at_unix: unix_now_secs(),
    }
}

fn render_evidence_summary(
    legacy: &str,
    items: &[EvidenceItem],
    max_chars: usize,
) -> Option<String> {
    if items.is_empty() && legacy.trim().is_empty() {
        return None;
    }
    let (supports, conflicts, context) = evidence_relation_counts(items);
    let latest = items
        .iter()
        .max_by_key(|item| item.observed_at_unix)
        .map(|item| inline_clip(&item.summary, max_chars))
        .unwrap_or_else(|| inline_clip(legacy, max_chars));
    let counts = [
        (supports > 0).then(|| format!("s={supports}")),
        (conflicts > 0).then(|| format!("c={conflicts}")),
        (context > 0).then(|| format!("x={context}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    Some(if counts.is_empty() {
        latest
    } else {
        format!("{} | {}", counts.join(" "), latest)
    })
}

fn evidence_relation_counts(items: &[EvidenceItem]) -> (usize, usize, usize) {
    (
        items
            .iter()
            .filter(|item| item.relation == "supports")
            .count(),
        items
            .iter()
            .filter(|item| item.relation == "conflicts")
            .count(),
        items
            .iter()
            .filter(|item| item.relation == "context")
            .count(),
    )
}

fn evidence_priority(items: &[EvidenceItem]) -> (usize, usize, usize, u64) {
    let (supports, conflicts, context) = evidence_relation_counts(items);
    let latest = items
        .iter()
        .map(|item| item.observed_at_unix)
        .max()
        .unwrap_or_default();
    (conflicts, supports, context, latest)
}

fn serialize_goal_state_reconcile_input(
    state: &GoalState,
    tool_call_id: &str,
    tool_name: &str,
    result: &str,
    execution_mode: &str,
    status: &str,
) -> String {
    let now = unix_now_secs();
    let focus_goal = select_focus_goal(state);
    let focus_goal_id = focus_goal.map(|goal| goal.id.as_str());
    let goals = prioritized_goal_items(&state.goals, focus_goal_id)
        .into_iter()
        .take(GOAL_STATE_COMPACTION_ITEM_LIMIT)
        .map(|goal| {
            format!(
                "- [{}] {} | level={} | status={} | confidence={:.2}{}{}{}",
                goal.id,
                inline_clip(&goal.title, 120),
                goal.level,
                goal.status,
                goal.confidence,
                goal.parent_id
                    .as_deref()
                    .map(|parent| format!(" | parent={parent}"))
                    .unwrap_or_default(),
                (!goal.summary.trim().is_empty())
                    .then(|| format!(" | summary={}", inline_clip(&goal.summary, 140)))
                    .unwrap_or_default(),
                render_evidence_summary(&goal.evidence, &goal.evidence_items, 120)
                    .map(|evidence| format!(" | evidence={evidence}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    let cognition = prioritized_cognition_items(&state.cognition)
        .into_iter()
        .take(GOAL_STATE_COMPACTION_ITEM_LIMIT)
        .map(|item| {
            format!(
                "- [{}] {} | confidence={:.2}{}",
                item.kind,
                inline_clip(&item.content, 180),
                item.confidence,
                render_evidence_summary(&item.evidence, &item.evidence_items, 120)
                    .map(|evidence| format!(" | evidence={evidence}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    let hot_data = prioritized_hot_data_items(&state.hot_data, focus_goal_id, now)
        .into_iter()
        .take(GOAL_STATE_COMPACTION_ITEM_LIMIT)
        .map(|item| {
            format!(
                "- [{}] {} | confidence={:.2}{}{}{}{}",
                item.id,
                inline_clip(&item.content, 180),
                item.confidence,
                (!item.source.trim().is_empty())
                    .then(|| format!(" | source={}", inline_clip(&item.source, 80)))
                    .unwrap_or_default(),
                item.goal_id
                    .as_deref()
                    .map(|goal_id| format!(" | goal={goal_id}"))
                    .unwrap_or_default(),
                item.expires_at_unix
                    .map(|expires_at| format!(" | expires_at={expires_at}"))
                    .unwrap_or_default(),
                render_evidence_summary("", &item.evidence_items, 120)
                    .map(|evidence| format!(" | evidence={evidence}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();

    inline_clip(
        &format!(
            "TOOL OUTCOME\ncall_id={tool_call_id}\nname={tool_name}\nexecution_mode={execution_mode}\nstatus={status}\nresult={}\n\nMISSION\n{}\n\nPHASE\n{}\n\nCURRENT FOCUS\n{}\n\nREFLECTION\n{}\n\nGOALS\n{}\n\nCOGNITION\n{}\n\nHOT DATA\n{}",
            inline_clip(result, 600),
            if state.mission.trim().is_empty() {
                "(none)".to_string()
            } else {
                inline_clip(&state.mission, 200)
            },
            state.phase,
            focus_goal
                .map(|goal| format!(
                    "[{}] {} | level={} | status={} | confidence={:.2}",
                    goal.id, goal.title, goal.level, goal.status, goal.confidence
                ))
                .unwrap_or_else(|| "(none)".to_string()),
            if state.reflection.trim().is_empty() {
                "(none)".to_string()
            } else {
                inline_clip(&state.reflection, 200)
            },
            if goals.is_empty() {
                "(none)".to_string()
            } else {
                goals.join("\n")
            },
            if cognition.is_empty() {
                "(none)".to_string()
            } else {
                cognition.join("\n")
            },
            if hot_data.is_empty() {
                "(none)".to_string()
            } else {
                hot_data.join("\n")
            }
        ),
        GOAL_STATE_RECONCILE_INPUT_CHARS,
    )
}

fn serialize_tool_result_summary_input(
    tool_name: &str,
    result: &str,
    execution_mode: &str,
    status: &str,
) -> String {
    inline_clip(
        &format!(
            "TOOL OUTCOME\nname={tool_name}\nexecution_mode={execution_mode}\nstatus={status}\nresult={}",
            inline_clip(result, 2_400)
        ),
        TOOL_RESULT_SUMMARY_INPUT_CHARS,
    )
}

fn serialize_goal_state_turn_reconcile_input(
    state: &GoalState,
    history: &[ChatMessage],
    user_input: &str,
    assistant_response: &str,
) -> String {
    let now = unix_now_secs();
    let focus_goal = select_focus_goal(state);
    let focus_goal_id = focus_goal.map(|goal| goal.id.as_str());
    let goals = prioritized_goal_items(&state.goals, focus_goal_id)
        .into_iter()
        .take(GOAL_STATE_COMPACTION_ITEM_LIMIT)
        .map(|goal| {
            format!(
                "- [{}] {} | level={} | status={} | confidence={:.2}{}{}{}",
                goal.id,
                inline_clip(&goal.title, 120),
                goal.level,
                goal.status,
                goal.confidence,
                goal.parent_id
                    .as_deref()
                    .map(|parent| format!(" | parent={parent}"))
                    .unwrap_or_default(),
                (!goal.summary.trim().is_empty())
                    .then(|| format!(" | summary={}", inline_clip(&goal.summary, 140)))
                    .unwrap_or_default(),
                render_evidence_summary(&goal.evidence, &goal.evidence_items, 120)
                    .map(|evidence| format!(" | evidence={evidence}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    let cognition = prioritized_cognition_items(&state.cognition)
        .into_iter()
        .take(GOAL_STATE_COMPACTION_ITEM_LIMIT)
        .map(|item| {
            format!(
                "- [{}] {} | confidence={:.2}{}",
                item.kind,
                inline_clip(&item.content, 180),
                item.confidence,
                render_evidence_summary(&item.evidence, &item.evidence_items, 120)
                    .map(|evidence| format!(" | evidence={evidence}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    let hot_data = prioritized_hot_data_items(&state.hot_data, focus_goal_id, now)
        .into_iter()
        .take(GOAL_STATE_COMPACTION_ITEM_LIMIT)
        .map(|item| {
            format!(
                "- [{}] {} | confidence={:.2}{}{}{}{}",
                item.id,
                inline_clip(&item.content, 180),
                item.confidence,
                (!item.source.trim().is_empty())
                    .then(|| format!(" | source={}", inline_clip(&item.source, 80)))
                    .unwrap_or_default(),
                item.goal_id
                    .as_deref()
                    .map(|goal_id| format!(" | goal={goal_id}"))
                    .unwrap_or_default(),
                item.expires_at_unix
                    .map(|expires_at| format!(" | expires_at={expires_at}"))
                    .unwrap_or_default(),
                render_evidence_summary("", &item.evidence_items, 120)
                    .map(|evidence| format!(" | evidence={evidence}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    let turn_transcript = latest_turn_transcript(history);

    inline_clip(
        &format!(
            "USER INPUT\n{}\n\nASSISTANT RESPONSE\n{}\n\nCURRENT TURN\n{}\n\nMISSION\n{}\n\nPHASE\n{}\n\nCURRENT FOCUS\n{}\n\nREFLECTION\n{}\n\nGOALS\n{}\n\nCOGNITION\n{}\n\nHOT DATA\n{}",
            inline_clip(user_input, 400),
            inline_clip(assistant_response, 500),
            if turn_transcript.is_empty() {
                "(none)".to_string()
            } else {
                turn_transcript
            },
            if state.mission.trim().is_empty() {
                "(none)".to_string()
            } else {
                inline_clip(&state.mission, 200)
            },
            state.phase,
            focus_goal
                .map(|goal| format!(
                    "[{}] {} | level={} | status={} | confidence={:.2}",
                    goal.id, goal.title, goal.level, goal.status, goal.confidence
                ))
                .unwrap_or_else(|| "(none)".to_string()),
            if state.reflection.trim().is_empty() {
                "(none)".to_string()
            } else {
                inline_clip(&state.reflection, 200)
            },
            if goals.is_empty() {
                "(none)".to_string()
            } else {
                goals.join("\n")
            },
            if cognition.is_empty() {
                "(none)".to_string()
            } else {
                cognition.join("\n")
            },
            if hot_data.is_empty() {
                "(none)".to_string()
            } else {
                hot_data.join("\n")
            }
        ),
        GOAL_STATE_TURN_RECONCILE_INPUT_CHARS,
    )
}

fn parse_tool_result_summary_response(raw: &str) -> Option<ToolResultSummaryResponse> {
    serde_json::from_str(raw.trim()).ok()
}

fn render_tool_result_summary_for_reconcile(
    summary: &ToolResultSummaryResponse,
    tool_name: &str,
    status: &str,
) -> Option<String> {
    let mut sections = Vec::new();
    let summary_text = inline_clip(summary.summary.trim(), 180);
    if !summary_text.is_empty() {
        sections.push(format!("summary={summary_text}"));
    } else {
        sections.push(format!(
            "summary={}",
            inline_clip(&summarize_tool_result(tool_name, "", status), 180)
        ));
    }
    if !summary.key_evidence.is_empty() {
        sections.push(format!(
            "key_evidence={}",
            summary
                .key_evidence
                .iter()
                .map(|item| inline_clip(item, 120))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    if !summary.candidate_hot_data.is_empty() {
        sections.push(format!(
            "candidate_hot_data={}",
            summary
                .candidate_hot_data
                .iter()
                .map(|item| inline_clip(item, 120))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    if !summary.candidate_risks.is_empty() {
        sections.push(format!(
            "candidate_risks={}",
            summary
                .candidate_risks
                .iter()
                .map(|item| inline_clip(item, 120))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    let rendered = sections.join("\n");
    (!rendered.trim().is_empty()).then_some(rendered)
}

fn latest_turn_transcript(history: &[ChatMessage]) -> String {
    let start = history
        .iter()
        .rposition(|message| message.role == "user")
        .unwrap_or(0);
    history[start..]
        .iter()
        .take(12)
        .map(|message| {
            let role = message.role.to_uppercase();
            let content = inline_clip(&message.content_text(), 220);
            match message.tool_call_id.as_deref() {
                Some(tool_call_id) => format!("{role} [{tool_call_id}]: {content}"),
                None => format!("{role}: {content}"),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_goal_state_reconcile_response(raw: &str) -> Option<GoalStateReconcileResponse> {
    serde_json::from_str(raw).ok()
}

fn apply_goal_state_reconcile_patch(state: &mut GoalState, patch: GoalStateReconcileResponse) {
    if let Some(mission) = patch.mission {
        state.mission = mission;
    }
    if let Some(phase) = patch.phase {
        state.phase = phase;
    }
    for goal in patch.goals {
        apply_goal_patch(&mut state.goals, goal);
    }
    for item in patch.cognition {
        if let Some(item) = materialize_cognition_patch(&state.cognition, item) {
            merge_cognition(&mut state.cognition, item);
        }
    }
    for item in patch.hot_data {
        if let Some(item) = materialize_hot_data_patch(&state.hot_data, item) {
            merge_hot_data(&mut state.hot_data, item);
        }
    }
    if let Some(focus_goal_id) = patch
        .current_focus_goal_id
        .filter(|goal_id| state.goals.iter().any(|goal| goal.id == *goal_id))
    {
        state.current_focus_goal_id = Some(focus_goal_id);
    }
    if let Some(reflection) = patch.reflection {
        state.reflection = reflection;
    }
    state.updated_at_unix = unix_now_secs();
    state.normalize();
}

fn apply_goal_patch(items: &mut Vec<GoalItem>, patch: GoalStateGoalPatch) {
    if patch.id.trim().is_empty() {
        return;
    }
    if let Some(existing) = items.iter_mut().find(|item| item.id == patch.id) {
        if let Some(title) = patch.title {
            existing.title = title;
        }
        if let Some(level) = patch.level {
            existing.level = level;
        }
        if let Some(status) = patch.status {
            existing.status = status;
        }
        if let Some(confidence) = patch.confidence {
            existing.confidence = confidence;
        }
        if let Some(parent_id) = patch.parent_id {
            existing.parent_id = Some(parent_id);
        }
        if let Some(summary) = patch.summary {
            existing.summary = summary;
        }
        if let Some(evidence) = patch.evidence {
            existing.evidence = evidence;
        }
        if let Some(evidence_items) = patch.evidence_items {
            existing.evidence_items = materialize_evidence_patches(evidence_items);
        }
        existing.updated_at_unix = unix_now_secs();
        existing.normalize();
        return;
    }

    let Some(title) = patch.title.filter(|value| !value.trim().is_empty()) else {
        return;
    };
    let mut goal = GoalItem {
        id: patch.id,
        title,
        level: patch.level.unwrap_or_else(|| "current".to_string()),
        status: patch.status.unwrap_or_else(|| "pending".to_string()),
        confidence: patch.confidence.unwrap_or(0.5),
        parent_id: patch.parent_id,
        summary: patch.summary.unwrap_or_default(),
        evidence: patch.evidence.unwrap_or_default(),
        evidence_items: materialize_evidence_patches(patch.evidence_items.unwrap_or_default()),
        updated_at_unix: unix_now_secs(),
    };
    goal.normalize();
    items.push(goal);
}

fn materialize_cognition_patch(
    existing_items: &[CognitionItem],
    patch: GoalStateCognitionPatch,
) -> Option<CognitionItem> {
    if patch.id.trim().is_empty() {
        return None;
    }
    if let Some(existing) = existing_items.iter().find(|item| item.id == patch.id) {
        let mut item = existing.clone();
        if let Some(kind) = patch.kind {
            item.kind = kind;
        }
        if let Some(content) = patch.content {
            item.content = content;
        }
        if let Some(confidence) = patch.confidence {
            item.confidence = confidence;
        }
        if let Some(evidence) = patch.evidence {
            item.evidence = evidence;
        }
        if let Some(evidence_items) = patch.evidence_items {
            item.evidence_items = materialize_evidence_patches(evidence_items);
        }
        item.updated_at_unix = unix_now_secs();
        item.normalize();
        return Some(item);
    }

    let mut item = CognitionItem {
        id: patch.id,
        kind: patch.kind.unwrap_or_else(|| "fact".to_string()),
        content: patch.content?,
        confidence: patch.confidence.unwrap_or(0.5),
        evidence: patch.evidence.unwrap_or_default(),
        evidence_items: materialize_evidence_patches(patch.evidence_items.unwrap_or_default()),
        updated_at_unix: unix_now_secs(),
    };
    item.normalize();
    Some(item)
}

fn materialize_hot_data_patch(
    existing_items: &[HotDataItem],
    patch: GoalStateHotDataPatch,
) -> Option<HotDataItem> {
    if patch.id.trim().is_empty() {
        return None;
    }
    if let Some(existing) = existing_items.iter().find(|item| item.id == patch.id) {
        let mut item = existing.clone();
        if let Some(content) = patch.content {
            item.content = content;
        }
        if let Some(confidence) = patch.confidence {
            item.confidence = confidence;
        }
        if let Some(source) = patch.source {
            item.source = source;
        }
        if let Some(goal_id) = patch.goal_id {
            item.goal_id = Some(goal_id);
        }
        if let Some(expires_at_unix) = patch.expires_at_unix {
            item.expires_at_unix = Some(expires_at_unix);
        }
        if let Some(evidence_items) = patch.evidence_items {
            item.evidence_items = materialize_evidence_patches(evidence_items);
        }
        item.updated_at_unix = unix_now_secs();
        item.normalize();
        return Some(item);
    }

    let mut item = HotDataItem {
        id: patch.id,
        content: patch.content?,
        confidence: patch.confidence.unwrap_or(0.5),
        source: patch.source.unwrap_or_default(),
        goal_id: patch.goal_id,
        expires_at_unix: patch.expires_at_unix,
        evidence_items: materialize_evidence_patches(patch.evidence_items.unwrap_or_default()),
        updated_at_unix: unix_now_secs(),
    };
    item.normalize();
    Some(item)
}

fn materialize_evidence_patches(patches: Vec<GoalStateEvidencePatch>) -> Vec<EvidenceItem> {
    patches
        .into_iter()
        .filter_map(|patch| {
            let summary = patch.summary?;
            Some(EvidenceItem {
                source_type: patch
                    .source_type
                    .unwrap_or_else(|| "system_state".to_string()),
                source: patch.source.unwrap_or_default(),
                summary,
                relation: patch.relation.unwrap_or_else(|| "supports".to_string()),
                observed_at_unix: patch.observed_at_unix.unwrap_or_else(unix_now_secs),
            })
        })
        .collect()
}

fn needs_goal_state_compaction(state: &GoalState) -> bool {
    let now = unix_now_secs();
    let active_hot_data = state
        .hot_data
        .iter()
        .filter(|item| !item.is_stale(now))
        .count();
    let total_chars = state
        .cognition
        .iter()
        .map(|item| {
            item.content.chars().count()
                + item.evidence.chars().count()
                + item
                    .evidence_items
                    .iter()
                    .map(|evidence| evidence.summary.chars().count())
                    .sum::<usize>()
        })
        .sum::<usize>()
        + state
            .hot_data
            .iter()
            .filter(|item| !item.is_stale(now))
            .map(|item| {
                item.content.chars().count()
                    + item.source.chars().count()
                    + item
                        .evidence_items
                        .iter()
                        .map(|evidence| evidence.summary.chars().count())
                        .sum::<usize>()
            })
            .sum::<usize>();

    state.cognition.len() > 6 || active_hot_data > 6 || total_chars > 900
}

fn serialize_goal_state_for_compaction(state: &GoalState) -> String {
    let now = unix_now_secs();
    let focus_goal = select_focus_goal(state);
    let focus_goal_id = focus_goal.map(|goal| goal.id.as_str());

    let goals = prioritized_goal_items(&state.goals, focus_goal_id)
        .into_iter()
        .take(GOAL_STATE_COMPACTION_ITEM_LIMIT)
        .map(|goal| {
            format!(
                "- [{}] {} | level={} | status={} | confidence={:.2}{}{}{}",
                goal.id,
                inline_clip(&goal.title, 120),
                goal.level,
                goal.status,
                goal.confidence,
                goal.parent_id
                    .as_deref()
                    .map(|parent| format!(" | parent={parent}"))
                    .unwrap_or_default(),
                (!goal.summary.trim().is_empty())
                    .then(|| format!(" | summary={}", inline_clip(&goal.summary, 140)))
                    .unwrap_or_default(),
                render_evidence_summary(&goal.evidence, &goal.evidence_items, 120)
                    .map(|evidence| format!(" | evidence={evidence}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();

    let cognition = prioritized_cognition_items(&state.cognition)
        .into_iter()
        .take(GOAL_STATE_COMPACTION_ITEM_LIMIT)
        .map(|item| {
            format!(
                "- [{}] {} | confidence={:.2}{}",
                item.kind,
                inline_clip(&item.content, 180),
                item.confidence,
                render_evidence_summary(&item.evidence, &item.evidence_items, 120)
                    .map(|evidence| format!(" | evidence={evidence}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();

    let hot_data = prioritized_hot_data_items(&state.hot_data, focus_goal_id, now)
        .into_iter()
        .take(GOAL_STATE_COMPACTION_ITEM_LIMIT)
        .map(|item| {
            format!(
                "- {} | confidence={:.2}{}{}{}",
                inline_clip(&item.content, 180),
                item.confidence,
                (!item.source.trim().is_empty())
                    .then(|| format!(" | source={}", inline_clip(&item.source, 80)))
                    .unwrap_or_default(),
                item.goal_id
                    .as_deref()
                    .map(|goal_id| format!(" | goal={goal_id}"))
                    .unwrap_or_default(),
                render_evidence_summary("", &item.evidence_items, 120)
                    .map(|evidence| format!(" | evidence={evidence}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();

    let focus = focus_goal
        .map(|goal| {
            format!(
                "[{}] {} | level={} | status={} | confidence={:.2}",
                goal.id, goal.title, goal.level, goal.status, goal.confidence
            )
        })
        .unwrap_or_else(|| "(none)".to_string());

    inline_clip(
        &format!(
            "MISSION\n{}\n\nPHASE\n{}\n\nREFLECTION\n{}\n\nFOCUS GOAL\n{focus}\n\nGOALS\n{}\n\nCOGNITION\n{}\n\nHOT DATA\n{}",
            if state.mission.trim().is_empty() {
                "(none)".to_string()
            } else {
                inline_clip(&state.mission, 180)
            },
            state.phase,
            if state.reflection.trim().is_empty() {
                "(none)".to_string()
            } else {
                inline_clip(&state.reflection, 180)
            },
            if goals.is_empty() {
                "(none)".to_string()
            } else {
                goals.join("\n")
            },
            if cognition.is_empty() {
                "(none)".to_string()
            } else {
                cognition.join("\n")
            },
            if hot_data.is_empty() {
                "(none)".to_string()
            } else {
                hot_data.join("\n")
            }
        ),
        GOAL_STATE_COMPACTION_INPUT_CHARS,
    )
}

fn parse_goal_state_compaction_response(raw: &str) -> Option<GoalStateCompactionResponse> {
    let mut parsed: GoalStateCompactionResponse = serde_json::from_str(raw).ok()?;
    parsed.cognition = inline_clip(parsed.cognition.trim(), GOAL_STATE_COMPACTION_OUTPUT_CHARS);
    parsed.hot_data = inline_clip(parsed.hot_data.trim(), GOAL_STATE_COMPACTION_OUTPUT_CHARS);
    if parsed.cognition.is_empty() || parsed.hot_data.is_empty() {
        return None;
    }
    parsed.confidence = Some(parsed.confidence.unwrap_or(0.75).clamp(0.0, 1.0));
    Some(parsed)
}

fn serialize_goal_state_for_execution_brief(state: &GoalState, user_input: &str) -> String {
    let goal_state = serialize_goal_state_for_compaction(state);
    if goal_state.is_empty() {
        return String::new();
    }
    inline_clip(
        &format!(
            "LATEST USER INPUT\n{}\n\nCURRENT GOAL STATE\n{}",
            inline_clip(user_input.trim(), 600),
            goal_state
        ),
        GOAL_STATE_EXECUTION_BRIEF_INPUT_CHARS,
    )
}

fn serialize_goal_state_for_delta(state: &GoalState, user_input: &str) -> String {
    let goal_state = serialize_goal_state_for_compaction(state);
    if goal_state.is_empty() {
        return String::new();
    }
    inline_clip(
        &format!(
            "LATEST USER INPUT\n{}\n\nCURRENT GOAL STATE\n{}",
            inline_clip(user_input.trim(), 600),
            goal_state
        ),
        GOAL_STATE_DELTA_INPUT_CHARS,
    )
}

fn parse_goal_state_execution_brief_response(raw: &str) -> Option<GoalStateExecutionBriefResponse> {
    let mut parsed: GoalStateExecutionBriefResponse = serde_json::from_str(raw).ok()?;
    parsed.focus = inline_clip(parsed.focus.trim(), GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS);
    parsed.next_step = inline_clip(
        parsed.next_step.trim(),
        GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS,
    );
    parsed.watch = inline_clip(parsed.watch.trim(), GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS);
    parsed.operating_rule = inline_clip(
        parsed.operating_rule.trim(),
        GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS,
    );
    if parsed.focus.is_empty() && parsed.next_step.is_empty() && parsed.operating_rule.is_empty() {
        return None;
    }
    parsed.confidence = Some(parsed.confidence.unwrap_or(0.75).clamp(0.0, 1.0));
    Some(parsed)
}

fn parse_goal_state_delta_response(raw: &str) -> Option<GoalStateDeltaResponse> {
    let mut parsed: GoalStateDeltaResponse = serde_json::from_str(raw).ok()?;
    parsed.mission = inline_clip(parsed.mission.trim(), GOAL_STATE_DELTA_OUTPUT_CHARS);
    parsed.phase = normalize_goal_phase_hint(&parsed.phase);
    parsed.current_focus_goal_id = inline_clip(
        parsed.current_focus_goal_id.trim(),
        GOAL_STATE_DELTA_OUTPUT_CHARS,
    );
    parsed.reflection = inline_clip(parsed.reflection.trim(), GOAL_STATE_DELTA_OUTPUT_CHARS);
    parsed.rationale = inline_clip(parsed.rationale.trim(), GOAL_STATE_DELTA_OUTPUT_CHARS);
    if parsed.mission.is_empty()
        && parsed.phase.is_empty()
        && parsed.current_focus_goal_id.is_empty()
        && parsed.reflection.is_empty()
    {
        return None;
    }
    Some(parsed)
}

fn serialize_meta_patterns_for_summary(state: &MetaPatternState) -> String {
    let patterns = state
        .patterns
        .iter()
        .take(6)
        .map(|pattern| {
            serde_json::json!({
                "id": pattern.id,
                "kind": pattern.kind,
                "problem_cluster": pattern.problem_cluster,
                "sample_count": pattern.sample_count,
                "confidence": pattern.confidence,
                "signal_patterns": pattern.signal_patterns,
                "recommended_strategies": pattern.recommended_strategies,
                "failure_patterns": pattern.failure_patterns,
                "representative_outcomes": pattern.representative_outcomes,
                "match_hints": pattern.match_hints,
                "model_summary": pattern.model_summary,
                "strategy_template": pattern.strategy_template,
            })
        })
        .collect::<Vec<_>>();
    if patterns.is_empty() {
        return String::new();
    }
    inline_clip(
        &serde_json::to_string_pretty(&serde_json::json!({ "patterns": patterns }))
            .unwrap_or_default(),
        8_000,
    )
}

fn parse_meta_pattern_summary_response(raw: &str) -> Option<MetaPatternSummaryResponse> {
    serde_json::from_str(raw).ok()
}

fn render_compacted_goal_state_block(
    state: &GoalState,
    compaction: &GoalStateCompactionResponse,
) -> String {
    let focus_goal = select_focus_goal(state);
    let focus_goal_id = focus_goal.map(|goal| goal.id.as_str());
    let mut lines = vec![
        "<goal-state>".to_string(),
        "[System note: The following is the agent's maintained goal state and model-compressed working memory. Use it as background context, update it when plans or beliefs change, and align actions with the active focus goal.]".to_string(),
        String::new(),
    ];
    if !state.mission.is_empty() {
        lines.push(format!("mission: {}", inline_clip(&state.mission, 180)));
    }
    lines.push(format!("phase: {}", state.phase));

    if let Some(goal) = focus_goal {
        lines.push(format!(
            "focus_goal: [{}] {} | level={} | status={} | confidence={:.2}",
            goal.id, goal.title, goal.level, goal.status, goal.confidence
        ));
        if !goal.summary.is_empty() {
            lines.push(format!(
                "focus_summary: {}",
                inline_clip(&goal.summary, 180)
            ));
        }
    } else {
        lines.push("focus_goal: (none)".to_string());
    }

    let goals = prioritized_goal_items(&state.goals, focus_goal_id);
    if !goals.is_empty() {
        lines.push("goals:".to_string());
        for goal in goals.into_iter().take(6) {
            lines.push(format!(
                "- [{}] {} | level={} | status={} | confidence={:.2}",
                goal.id,
                inline_clip(&goal.title, 120),
                goal.level,
                goal.status,
                goal.confidence
            ));
        }
    }

    lines.push(format!(
        "cognition_summary: {} | confidence={:.2}",
        compaction.cognition,
        compaction.confidence.unwrap_or(0.75)
    ));
    lines.push(format!(
        "hot_data_summary: {} | confidence={:.2}",
        compaction.hot_data,
        compaction.confidence.unwrap_or(0.75)
    ));
    if !state.reflection.is_empty() {
        lines.push(format!(
            "reflection: {}",
            inline_clip(&state.reflection, 180)
        ));
    }
    lines.push("</goal-state>".to_string());
    lines.join("\n")
}

fn normalize_goal_phase_hint(value: &str) -> String {
    match value.trim() {
        "understand" | "investigate" | "act" | "verify" | "finalize" => value.trim().to_string(),
        _ => String::new(),
    }
}

fn derive_execution_brief_from_goal_state(
    state: &GoalState,
) -> Option<GoalStateExecutionBriefResponse> {
    if state.goals.is_empty() && state.cognition.is_empty() && state.hot_data.is_empty() {
        return None;
    }

    let now = unix_now_secs();
    let focus_goal = select_focus_goal(state)?;
    let focus_goal_id = Some(focus_goal.id.as_str());
    let next_step = prioritized_hot_data_items(&state.hot_data, focus_goal_id, now)
        .into_iter()
        .find(|item| match item.goal_id.as_deref() {
            Some(goal_id) => goal_id == focus_goal.id,
            None => true,
        })
        .map(|item| inline_clip(&item.content, GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS))
        .unwrap_or_default();
    let watch = if focus_goal.status == "blocked" {
        if focus_goal.summary.trim().is_empty() {
            format!("Focus goal `{}` is blocked.", focus_goal.title)
        } else {
            inline_clip(&focus_goal.summary, GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS)
        }
    } else {
        prioritized_cognition_items(&state.cognition)
            .into_iter()
            .find(|item| matches!(item.kind.as_str(), "risk" | "unknown"))
            .map(|item| inline_clip(&item.content, GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS))
            .unwrap_or_default()
    };
    let operating_rule = if focus_goal.status == "blocked" {
        "Resolve the blocker or ask for the minimum clarification needed before switching work."
            .to_string()
    } else {
        "Prioritize actions that directly advance the focus goal; avoid unrelated work unless the user changes direction."
            .to_string()
    };

    Some(GoalStateExecutionBriefResponse {
        focus: inline_clip(
            &format!(
                "[{}] {} | phase={} | status={} | confidence={:.2}",
                focus_goal.id,
                focus_goal.title,
                state.phase,
                focus_goal.status,
                focus_goal.confidence
            ),
            GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS,
        ),
        next_step,
        watch: if !watch.is_empty() {
            watch
        } else {
            inline_clip(&state.reflection, GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS)
        },
        operating_rule: inline_clip(&operating_rule, GOAL_STATE_EXECUTION_BRIEF_OUTPUT_CHARS),
        confidence: Some(focus_goal.confidence.clamp(0.0, 1.0)),
    })
}

fn render_goal_state_delta_block(delta: &GoalStateDeltaResponse) -> String {
    let mut lines = vec![
        "<state-delta>".to_string(),
        "[System note: This is a sidecar-suggested goal-state update. Treat it as a justified proposal, not committed state. Apply it with `goal_state` only if current evidence still supports it.]".to_string(),
        String::new(),
    ];
    if !delta.mission.is_empty() {
        lines.push(format!("mission: {}", delta.mission));
    }
    if !delta.phase.is_empty() {
        lines.push(format!("phase: {}", delta.phase));
    }
    if !delta.current_focus_goal_id.is_empty() {
        lines.push(format!(
            "current_focus_goal_id: {}",
            delta.current_focus_goal_id
        ));
    }
    if !delta.reflection.is_empty() {
        lines.push(format!("reflection: {}", delta.reflection));
    }
    if !delta.rationale.is_empty() {
        lines.push(format!("rationale: {}", delta.rationale));
    }
    lines.push("</state-delta>".to_string());
    lines.join("\n")
}

fn render_goal_execution_brief_block(brief: &GoalStateExecutionBriefResponse) -> String {
    let mut lines = vec![
        "<execution-brief>".to_string(),
        "[System note: This is a short action digest derived from goal_state. Follow it when it matches current evidence; if it does not, update goal_state before continuing.]".to_string(),
        String::new(),
    ];
    if !brief.focus.is_empty() {
        lines.push(format!("focus: {}", brief.focus));
    }
    if !brief.next_step.is_empty() {
        lines.push(format!("next_step: {}", brief.next_step));
    }
    if !brief.watch.is_empty() {
        lines.push(format!("watch: {}", brief.watch));
    }
    if !brief.operating_rule.is_empty() {
        lines.push(format!("operating_rule: {}", brief.operating_rule));
    }
    lines.push(format!(
        "confidence: {:.2}",
        brief.confidence.unwrap_or(0.75).clamp(0.0, 1.0)
    ));
    lines.push("</execution-brief>".to_string());
    lines.join("\n")
}

#[cfg(test)]
fn render_goal_execution_guidance(state: &GoalState) -> Option<String> {
    if state.goals.is_empty() && state.cognition.is_empty() && state.hot_data.is_empty() {
        return None;
    }

    let now = unix_now_secs();
    let focus_goal = select_focus_goal(state)?;
    let focus_goal_id = Some(focus_goal.id.as_str());
    let next_step = prioritized_hot_data_items(&state.hot_data, focus_goal_id, now)
        .into_iter()
        .find(|item| match item.goal_id.as_deref() {
            Some(goal_id) => goal_id == focus_goal.id,
            None => true,
        })
        .map(|item| inline_clip(&item.content, 180));
    let supporting_goals = prioritized_goal_items(&state.goals, focus_goal_id)
        .into_iter()
        .filter(|goal| goal.id != focus_goal.id)
        .filter(|goal| goal.parent_id.as_deref() == Some(focus_goal.id.as_str()))
        .filter(|goal| matches!(goal.status.as_str(), "pending" | "in_progress" | "blocked"))
        .take(3)
        .map(|goal| format!("{} [{}]", inline_clip(&goal.title, 80), goal.status))
        .collect::<Vec<_>>();
    let blocker = if focus_goal.status == "blocked" {
        Some(if focus_goal.summary.trim().is_empty() {
            format!("Focus goal `{}` is blocked.", focus_goal.title)
        } else {
            inline_clip(&focus_goal.summary, 180)
        })
    } else {
        prioritized_cognition_items(&state.cognition)
            .into_iter()
            .find(|item| matches!(item.kind.as_str(), "risk" | "unknown"))
            .map(|item| inline_clip(&item.content, 180))
    };

    let mut lines = vec![
        "# Runtime Goal Guidance".to_string(),
        "Treat the maintained goal state as a strong hint about current priorities, not as an immutable contract. Prefer current evidence when the two conflict."
            .to_string(),
        format!(
            "Focus goal: [{}] {} | status={} | confidence={:.2}",
            focus_goal.id, focus_goal.title, focus_goal.status, focus_goal.confidence
        ),
    ];
    if let Some(next_step) = next_step {
        lines.push(format!("Next step: {next_step}"));
    }
    if !supporting_goals.is_empty() {
        lines.push(format!(
            "Supporting subgoals: {}",
            supporting_goals.join("; ")
        ));
    }
    if let Some(blocker) = blocker {
        lines.push(format!("Potential blocker: {blocker}"));
    }
    lines.push(if focus_goal.status == "blocked" {
        "Execution hint: resolve the blocker or ask for the minimum clarification needed before switching work."
            .to_string()
    } else {
        "Execution hint: prioritize actions that directly advance the focus goal; avoid drifting to unrelated work unless the user changes direction."
            .to_string()
    });
    Some(lines.join("\n"))
}

fn select_focus_goal<'a>(state: &'a GoalState) -> Option<&'a GoalItem> {
    state
        .current_focus_goal_id
        .as_deref()
        .and_then(|goal_id| state.goals.iter().find(|goal| goal.id == goal_id))
        .or_else(|| {
            prioritized_goal_items(&state.goals, None)
                .into_iter()
                .next()
        })
}

fn prioritized_goal_items<'a>(
    goals: &'a [GoalItem],
    focus_goal_id: Option<&str>,
) -> Vec<&'a GoalItem> {
    let mut goals = goals.iter().collect::<Vec<_>>();
    goals.sort_by(|left, right| {
        goal_focus_priority(left.id.as_str(), focus_goal_id)
            .cmp(&goal_focus_priority(right.id.as_str(), focus_goal_id))
            .then_with(|| {
                goal_status_priority(&left.status).cmp(&goal_status_priority(&right.status))
            })
            .then_with(|| goal_level_priority(&left.level).cmp(&goal_level_priority(&right.level)))
            .then_with(|| {
                evidence_priority(&right.evidence_items)
                    .cmp(&evidence_priority(&left.evidence_items))
            })
            .then_with(|| {
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.updated_at_unix.cmp(&left.updated_at_unix))
    });
    goals
}

fn prioritized_cognition_items(items: &[CognitionItem]) -> Vec<&CognitionItem> {
    let mut items = items.iter().collect::<Vec<_>>();
    items.sort_by(|left, right| {
        cognition_kind_priority(&left.kind)
            .cmp(&cognition_kind_priority(&right.kind))
            .then_with(|| {
                evidence_priority(&right.evidence_items)
                    .cmp(&evidence_priority(&left.evidence_items))
            })
            .then_with(|| {
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.updated_at_unix.cmp(&left.updated_at_unix))
    });
    items
}

fn prioritized_hot_data_items<'a>(
    items: &'a [HotDataItem],
    focus_goal_id: Option<&str>,
    now: u64,
) -> Vec<&'a HotDataItem> {
    let mut items = items
        .iter()
        .filter(|item| !item.is_stale(now))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        hot_data_focus_priority(left.goal_id.as_deref(), focus_goal_id)
            .cmp(&hot_data_focus_priority(
                right.goal_id.as_deref(),
                focus_goal_id,
            ))
            .then_with(|| {
                evidence_priority(&right.evidence_items)
                    .cmp(&evidence_priority(&left.evidence_items))
            })
            .then_with(|| {
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.updated_at_unix.cmp(&left.updated_at_unix))
    });
    items
}

fn goal_focus_priority(goal_id: &str, focus_goal_id: Option<&str>) -> usize {
    match focus_goal_id {
        Some(focus_goal_id) if goal_id == focus_goal_id => 0,
        Some(_) => 1,
        None => 2,
    }
}

fn goal_status_priority(status: &str) -> usize {
    match status {
        "in_progress" => 0,
        "blocked" => 1,
        "pending" => 2,
        "succeeded" => 3,
        "failed" => 4,
        "transferred" => 5,
        "cancelled" => 6,
        _ => 7,
    }
}

fn goal_level_priority(level: &str) -> usize {
    match level {
        "current" => 0,
        "subgoal" => 1,
        "long_term" => 2,
        _ => 3,
    }
}

fn cognition_kind_priority(kind: &str) -> usize {
    match kind {
        "fact" => 0,
        "decision" => 1,
        "assumption" => 2,
        "risk" => 3,
        "unknown" => 4,
        _ => 5,
    }
}

fn hot_data_focus_priority(goal_id: Option<&str>, focus_goal_id: Option<&str>) -> usize {
    match (goal_id, focus_goal_id) {
        (Some(goal_id), Some(focus_goal_id)) if goal_id == focus_goal_id => 0,
        (Some(_), _) => 1,
        _ => 2,
    }
}

fn push_context_injection(
    injections: &mut Vec<ContextInjection>,
    label: &'static str,
    content: String,
    max_chars: usize,
) {
    if content.trim().is_empty() {
        return;
    }
    injections.push(ContextInjection {
        label,
        content,
        max_chars,
    });
}

fn assemble_turn_messages(
    system_prompt: &str,
    history: &[ChatMessage],
    injections: &[ContextInjection],
    injection_budget_chars: usize,
) -> (Vec<ChatMessage>, ContextInjectionStats) {
    let prompt_history = trim_prompt_history(history, PROMPT_HISTORY_MAX_USER_TURNS);
    let mut messages = Vec::with_capacity(prompt_history.len() + 1);
    messages.push(ChatMessage::system(system_prompt.to_string()));
    messages.extend(prompt_history);
    let current_user_idx = messages.iter().rposition(|message| message.role == "user");
    if let Some(current_user_idx) = current_user_idx {
        if let Some(user_message) = messages.get_mut(current_user_idx) {
            let owned_injections = injections
                .iter()
                .map(|injection| ContextInjection {
                    label: injection.label,
                    content: injection.content.clone(),
                    max_chars: injection.max_chars,
                })
                .collect::<Vec<_>>();
            let (rendered_injections, injection_stats) =
                finalize_context_injections(owned_injections, injection_budget_chars);
            if !rendered_injections.is_empty() {
                let merged = format!(
                    "{}\n\n{}",
                    user_message.content_text(),
                    rendered_injections.join("\n\n")
                );
                user_message.content = Some(Value::String(merged));
            }
            return (messages, injection_stats);
        }
    }

    (messages, ContextInjectionStats::default())
}

fn trim_prompt_history(history: &[ChatMessage], max_user_turns: usize) -> Vec<ChatMessage> {
    if max_user_turns == 0 || history.is_empty() {
        return Vec::new();
    }

    let user_indices = history
        .iter()
        .enumerate()
        .filter_map(|(idx, message)| (message.role == "user").then_some(idx))
        .collect::<Vec<_>>();

    let Some(&start_idx) = user_indices.get(user_indices.len().saturating_sub(max_user_turns))
    else {
        return history.to_vec();
    };

    let mut trimmed = history[..start_idx]
        .iter()
        .filter(|message| is_context_compaction_message(message))
        .cloned()
        .collect::<Vec<_>>();
    trimmed.extend_from_slice(&history[start_idx..]);
    trimmed
}

fn is_context_compaction_message(message: &ChatMessage) -> bool {
    message.role == "system" && message.content_text().contains("[CONTEXT COMPACTION]")
}

fn continuation_prefix_digest(history: &[ChatMessage]) -> Option<String> {
    let last_assistant_idx = history
        .iter()
        .rposition(|message| message.role == "assistant")?;
    let prefix = &history[..=last_assistant_idx];
    let serialized = serde_json::to_string(prefix).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serialized.hash(&mut hasher);
    Some(format!("{:016x}", hasher.finish()))
}

fn supports_response_continuation(base_url: &str) -> bool {
    let normalized = base_url.trim().trim_end_matches('/').to_ascii_lowercase();
    normalized == "https://api.openai.com/v1"
        || normalized.starts_with("https://api.openai.com/v1/")
        || normalized.contains("/backend-api/codex")
}

fn finalize_context_injections(
    injections: Vec<ContextInjection>,
    total_budget_chars: usize,
) -> (Vec<String>, ContextInjectionStats) {
    let mut stats = ContextInjectionStats {
        total_blocks: injections.len(),
        ..ContextInjectionStats::default()
    };
    let mut used = 0usize;
    let mut rendered = Vec::new();

    for injection in injections {
        let normalized = injection.content.trim();
        if normalized.is_empty() {
            continue;
        }
        stats.original_chars += normalized.chars().count();
        let block_budget = injection
            .max_chars
            .min(total_budget_chars.saturating_sub(used));
        if block_budget < CONTEXT_INJECTION_MIN_BLOCK_CHARS {
            stats.skipped_labels.push(injection.label);
            continue;
        }

        let clipped = clip_block_chars(normalized, block_budget);
        if clipped.is_empty() {
            stats.skipped_labels.push(injection.label);
            continue;
        }

        let final_chars = clipped.chars().count();
        if final_chars < normalized.chars().count() {
            stats.clipped_labels.push(injection.label);
        }
        used += final_chars;
        stats.final_chars += final_chars;
        stats.kept_blocks += 1;
        rendered.push(clipped);
    }

    (rendered, stats)
}

fn render_context_budget_nudge(stats: &ContextInjectionStats) -> String {
    let mut details = Vec::new();
    if !stats.clipped_labels.is_empty() {
        details.push(format!("裁剪 {}", stats.clipped_labels.join(",")));
    }
    if !stats.skipped_labels.is_empty() {
        details.push(format!("跳过 {}", stats.skipped_labels.join(",")));
    }
    if details.is_empty() {
        details.push("未发生裁剪".to_string());
    }
    format!(
        "已按预算裁剪上下文注入：保留 {}/{} 个块，{} -> {} chars（{}）。",
        stats.kept_blocks,
        stats.total_blocks,
        stats.original_chars,
        stats.final_chars,
        details.join("；")
    )
}

fn sanitize_debug_file_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn clip_block_chars(value: &str, max_chars: usize) -> String {
    let normalized = value.trim();
    if normalized.is_empty() || max_chars == 0 {
        return String::new();
    }
    let char_count = normalized.chars().count();
    if char_count <= max_chars {
        return normalized.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    normalized
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>()
        + "..."
}

fn inline_clip(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ").trim().to_string();
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        normalized.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn redacted_truncated(text: impl Into<String>, max_len: usize) -> String {
    truncated(redact_secrets(text.into()), max_len)
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn classify_tool_cognition_kind(result: &str, status: &str) -> &'static str {
    if status == "approval_required" {
        return "risk";
    }
    if status == "error" {
        return "risk";
    }
    let lowered = result.to_lowercase();
    if lowered.starts_with("tool_error:") || lowered.contains("approval denied") {
        return "risk";
    }
    if lowered.starts_with("no ") || lowered.contains(" not found") {
        return "unknown";
    }
    "fact"
}

fn tool_observation_confidence(result: &str, status: &str) -> f32 {
    if status == "approval_required" {
        return 0.95;
    }
    if status == "error" {
        return 0.95;
    }
    let lowered = result.to_lowercase();
    if lowered.starts_with("tool_error:") || lowered.contains("approval denied") {
        0.95
    } else if lowered.starts_with("no ") || lowered.contains(" not found") {
        0.85
    } else {
        0.9
    }
}

fn should_reconcile_goal_state_after_tool(tool_name: &str, result: &str, status: &str) -> bool {
    if status != "done" {
        return true;
    }
    if is_high_signal_tool(tool_name) {
        return true;
    }
    let normalized = result.to_ascii_lowercase();
    normalized.starts_with("tool_error:")
        || [
            "error:",
            " error",
            "failed",
            " failure",
            "not found",
            "no file found",
            "timeout",
            "timed out",
            "exception",
            "denied",
            "blocked",
            "panic",
            "missing",
        ]
        .iter()
        .any(|needle| normalized.contains(needle))
}

fn should_summarize_tool_result_for_reconcile(tool_name: &str, result: &str) -> bool {
    if tool_name == "delegate_to_worker" {
        return false;
    }
    if !is_summary_worthy_tool(tool_name) {
        return false;
    }
    result.len() > 1_500 || result.lines().count() > 30
}

fn is_metadata_observation_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "list_files"
            | "skills_list"
            | "list_sessions"
            | "list_approvals"
            | "list_delegate_runs"
            | "list_due_cron_jobs"
            | "list_cron_jobs"
            | "list_providers"
            | "resolve_provider_status"
            | "extensions_overview"
            | "office_inspect"
            | "browser_find"
            | "browser_snapshot"
            | "git_status"
    )
}

fn is_high_signal_tool(tool_name: &str) -> bool {
    let tool_name = tool_name.to_ascii_lowercase();
    tool_name == "delegate_to_worker"
        || tool_name == "terminal"
        || tool_name == "execute_code"
        || tool_name.starts_with("browser_")
        || tool_name.starts_with("office_")
}

fn is_summary_worthy_tool(tool_name: &str) -> bool {
    let tool_name = tool_name.to_ascii_lowercase();
    tool_name == "delegate_to_worker"
        || tool_name == "terminal"
        || tool_name == "execute_code"
        || tool_name.starts_with("browser_")
        || tool_name.starts_with("office_")
}

fn summarize_tool_result_for_history(
    tool_name: &str,
    result: &str,
    status: &str,
    reconcile_summary: Option<&str>,
) -> String {
    if tool_name == "delegate_to_worker" {
        return summarize_tool_result(tool_name, result, status);
    }
    if is_metadata_observation_tool(tool_name) {
        return render_tool_observation_for_history(tool_name, result, status, reconcile_summary);
    }
    if let Some(summary) = reconcile_summary {
        if is_summary_worthy_tool(tool_name) {
            return render_tool_observation_for_history(tool_name, result, status, Some(summary));
        }
    }
    if is_summary_worthy_tool(tool_name) && (result.len() > 1_500 || result.lines().count() > 30) {
        return render_tool_observation_for_history(tool_name, result, status, reconcile_summary);
    }
    result.to_string()
}

fn should_run_context_compression_before_iteration(iteration: usize) -> bool {
    iteration > 1
}

fn summarize_tool_result(tool_name: &str, result: &str, status: &str) -> String {
    if tool_name == "delegate_to_worker" {
        if let Some(observation) = parse_delegate_worker_observation(result) {
            let summary = observation
                .payload
                .summary
                .as_deref()
                .map(|value| inline_clip(value, 160))
                .unwrap_or_else(|| inline_clip(&observation.objective, 160));
            return match observation
                .tool_status
                .as_deref()
                .or(Some(status))
                .unwrap_or(status)
            {
                "awaiting_approval" => {
                    format!("Delegated worker is waiting for approval: {summary}")
                }
                "failed" => format!("Delegated worker failed: {summary}"),
                _ => format!("Delegated worker reported: {summary}"),
            };
        }
    }
    let normalized = result.replace('\n', " ").trim().to_string();
    let snippet = inline_clip(&normalized, 180);
    match status {
        "approval_required" => format!("Tool `{tool_name}` requires approval: {snippet}"),
        "error" => format!("Tool `{tool_name}` failed: {snippet}"),
        "done" => {
            if normalized.to_lowercase().starts_with("tool_error:") {
                format!("Tool `{tool_name}` failed: {snippet}")
            } else {
                format!("Tool `{tool_name}` observed: {snippet}")
            }
        }
        other => format!("Tool `{tool_name}` status `{other}`: {snippet}"),
    }
}

fn tool_history_headline(tool_name: &str, result: &str, status: &str) -> String {
    if tool_name == "delegate_to_worker" {
        return summarize_tool_result(tool_name, result, status);
    }
    match status {
        "approval_required" => format!("Tool `{tool_name}` requires approval."),
        "error" => format!("Tool `{tool_name}` failed."),
        "done" => {
            if result
                .trim()
                .to_ascii_lowercase()
                .starts_with("tool_error:")
            {
                format!("Tool `{tool_name}` failed.")
            } else {
                format!("Tool `{tool_name}` observed new information.")
            }
        }
        other => format!("Tool `{tool_name}` status `{other}`."),
    }
}

fn render_tool_observation_for_history(
    tool_name: &str,
    result: &str,
    status: &str,
    reconcile_summary: Option<&str>,
) -> String {
    let mut lines = vec![tool_history_headline(tool_name, result, status)];

    if let Some(summary) = reconcile_summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!(
            "Goal-relevant summary: {}",
            inline_clip(summary, 240)
        ));
    }

    let facts = extract_tool_history_facts(result);
    if !facts.is_empty() {
        lines.push("Key facts:".to_string());
        lines.extend(
            facts
                .into_iter()
                .take(4)
                .map(|fact| format!("- {}", inline_clip(&fact, 140))),
        );
        return lines.join("\n");
    }

    let excerpt = compact_tool_excerpt(result, 240);
    if !excerpt.is_empty() {
        lines.push(format!("Excerpt: {}", inline_clip(&excerpt, 240)));
    }
    lines.join("\n")
}

fn extract_tool_history_facts(result: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(result) else {
        return Vec::new();
    };

    match value {
        Value::Array(items) => vec![format!("Returned {} items.", items.len())],
        Value::Object(map) => {
            let mut facts = Vec::new();
            for key in [
                "count",
                "total",
                "path",
                "paths",
                "file",
                "files",
                "matches",
                "results",
                "pages",
                "slides",
                "sheets",
                "format",
                "mime_type",
                "title",
                "status",
            ] {
                let Some(value) = map.get(key) else {
                    continue;
                };
                match value {
                    Value::String(text) if !text.trim().is_empty() => {
                        facts.push(format!("{key}: {}", text.trim()));
                    }
                    Value::Number(number) => {
                        facts.push(format!("{key}: {number}"));
                    }
                    Value::Bool(flag) => {
                        facts.push(format!("{key}: {flag}"));
                    }
                    Value::Array(items) => {
                        facts.push(format!("{key}: {} item(s)", items.len()));
                    }
                    Value::Object(object) => {
                        facts.push(format!("{key}: {} field(s)", object.len()));
                    }
                    _ => {}
                }
            }
            if facts.is_empty() && !map.is_empty() {
                facts.push(format!("Returned {} top-level field(s).", map.len()));
            }
            facts
        }
        _ => Vec::new(),
    }
}

fn compact_tool_excerpt(result: &str, max_chars: usize) -> String {
    let excerpt = result
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ");
    inline_clip(&excerpt, max_chars)
}

fn parse_delegate_worker_observation(result: &str) -> Option<DelegateWorkerObservation> {
    let envelope: DelegateWorkerToolResult = serde_json::from_str(result).ok()?;
    let worker_result = envelope.worker_result.as_deref()?.trim();
    if worker_result.is_empty() {
        return None;
    }

    let mut payload = serde_json::from_str::<DelegateWorkerResultPayload>(worker_result).ok();
    if payload.is_none() {
        payload = Some(DelegateWorkerResultPayload {
            summary: Some(worker_result.to_string()),
            ..Default::default()
        });
    }

    let mut payload = payload?;
    payload.summary = payload
        .summary
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    payload.key_evidence = payload
        .key_evidence
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    payload.candidate_beliefs = payload
        .candidate_beliefs
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    payload.candidate_risks = payload
        .candidate_risks
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    payload.recommended_next_actions = payload
        .recommended_next_actions
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    payload.raw_refs = payload
        .raw_refs
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    payload.step_updates = payload
        .step_updates
        .into_iter()
        .filter_map(|step| {
            let step_id = step.step_id.map(|value| value.trim().to_string());
            let status = step.status.map(|value| value.trim().to_string());
            let summary = step.summary.map(|value| value.trim().to_string());
            if step_id.as_deref().unwrap_or_default().is_empty()
                && status.as_deref().unwrap_or_default().is_empty()
                && summary.as_deref().unwrap_or_default().is_empty()
            {
                None
            } else {
                Some(DelegateWorkerStepUpdate {
                    step_id: step_id.filter(|value| !value.is_empty()),
                    status: status.filter(|value| !value.is_empty()),
                    summary: summary.filter(|value| !value.is_empty()),
                })
            }
        })
        .collect();

    let objective = envelope.objective.unwrap_or_default().trim().to_string();
    let objective = if objective.is_empty() {
        "Delegated worker task".to_string()
    } else {
        objective
    };

    Some(DelegateWorkerObservation {
        tool_status: envelope.status,
        objective,
        focus_goal_id: envelope
            .focus_goal_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        payload,
    })
}

fn apply_delegate_worker_observation(
    state: &mut GoalState,
    tool_call_id: &str,
    result: &str,
    execution_mode: &str,
    status: &str,
) {
    let Some(observation) = parse_delegate_worker_observation(result) else {
        return;
    };
    let effective_status = observation
        .tool_status
        .as_deref()
        .or(Some(status))
        .unwrap_or(status);
    let goal_id = observation
        .focus_goal_id
        .as_ref()
        .filter(|goal_id| state.goals.iter().any(|goal| goal.id == **goal_id))
        .cloned()
        .or_else(|| state.current_focus_goal_id.clone());

    let source = format!("{execution_mode}:delegate_to_worker");
    let base_evidence = format!(
        "Delegated worker `{}` reported status `{}` via tool output.",
        observation.objective, effective_status
    );
    let evidence_items = delegate_worker_evidence_items(
        &source,
        &base_evidence,
        &observation.payload.key_evidence,
        &observation.payload.raw_refs,
        if effective_status == "failed" {
            "conflicts"
        } else {
            "supports"
        },
    );

    if let Some(focus_goal_id) = goal_id.clone() {
        if state
            .current_focus_goal_id
            .as_ref()
            .is_none_or(|current| current == &focus_goal_id)
        {
            state.current_focus_goal_id = Some(focus_goal_id);
        }
    }

    if let Some(summary) = observation.payload.summary.as_deref() {
        let summary = inline_clip(summary, 220);
        merge_cognition(
            &mut state.cognition,
            CognitionItem {
                id: format!("delegate-worker:{tool_call_id}:summary"),
                kind: if effective_status == "failed" {
                    "risk".to_string()
                } else {
                    "fact".to_string()
                },
                content: format!("Worker summary for `{}`: {summary}", observation.objective),
                confidence: if effective_status == "failed" {
                    0.93
                } else {
                    0.86
                },
                evidence: base_evidence.clone(),
                evidence_items: evidence_items.clone(),
                updated_at_unix: unix_now_secs(),
            },
        );
        merge_hot_data(
            &mut state.hot_data,
            HotDataItem {
                id: format!("delegate-worker:{tool_call_id}:summary"),
                content: format!("Worker summary: {summary}"),
                confidence: if effective_status == "failed" {
                    0.93
                } else {
                    0.88
                },
                source: "delegate_to_worker".to_string(),
                goal_id: goal_id.clone(),
                expires_at_unix: Some(unix_now_secs() + 8 * 3600),
                evidence_items: evidence_items.clone(),
                updated_at_unix: unix_now_secs(),
            },
        );
    }

    for (idx, belief) in observation
        .payload
        .candidate_beliefs
        .iter()
        .take(3)
        .enumerate()
    {
        merge_cognition(
            &mut state.cognition,
            CognitionItem {
                id: format!("delegate-worker:{tool_call_id}:belief:{idx}"),
                kind: "fact".to_string(),
                content: inline_clip(belief, 220),
                confidence: 0.78,
                evidence: base_evidence.clone(),
                evidence_items: evidence_items.clone(),
                updated_at_unix: unix_now_secs(),
            },
        );
    }

    for (idx, risk) in observation
        .payload
        .candidate_risks
        .iter()
        .take(3)
        .enumerate()
    {
        merge_cognition(
            &mut state.cognition,
            CognitionItem {
                id: format!("delegate-worker:{tool_call_id}:risk:{idx}"),
                kind: "risk".to_string(),
                content: inline_clip(risk, 220),
                confidence: 0.82,
                evidence: base_evidence.clone(),
                evidence_items: evidence_items.clone(),
                updated_at_unix: unix_now_secs(),
            },
        );
    }

    if !observation.payload.recommended_next_actions.is_empty() {
        let next_actions = observation
            .payload
            .recommended_next_actions
            .iter()
            .take(3)
            .map(|value| inline_clip(value, 100))
            .collect::<Vec<_>>()
            .join("; ");
        merge_hot_data(
            &mut state.hot_data,
            HotDataItem {
                id: format!("delegate-worker:{tool_call_id}:next-actions"),
                content: format!("Worker-recommended next actions: {next_actions}"),
                confidence: 0.8,
                source: "delegate_to_worker".to_string(),
                goal_id: goal_id.clone(),
                expires_at_unix: Some(unix_now_secs() + 8 * 3600),
                evidence_items: evidence_items.clone(),
                updated_at_unix: unix_now_secs(),
            },
        );
    }

    if !observation.payload.step_updates.is_empty() {
        let step_summary = observation
            .payload
            .step_updates
            .iter()
            .take(4)
            .map(render_delegate_step_update)
            .collect::<Vec<_>>()
            .join("; ");
        merge_hot_data(
            &mut state.hot_data,
            HotDataItem {
                id: format!("delegate-worker:{tool_call_id}:steps"),
                content: format!("Worker step updates: {step_summary}"),
                confidence: 0.78,
                source: "delegate_to_worker".to_string(),
                goal_id: goal_id,
                expires_at_unix: Some(unix_now_secs() + 8 * 3600),
                evidence_items,
                updated_at_unix: unix_now_secs(),
            },
        );
    }
}

fn delegate_worker_evidence_items(
    source: &str,
    base_evidence: &str,
    key_evidence: &[String],
    raw_refs: &[String],
    relation: &str,
) -> Vec<EvidenceItem> {
    let mut items = vec![evidence_item(
        "tool_output",
        source,
        base_evidence,
        relation,
    )];
    for evidence in key_evidence.iter().take(4) {
        items.push(evidence_item("tool_output", source, evidence, relation));
    }
    if !raw_refs.is_empty() {
        items.push(evidence_item(
            "tool_output",
            source,
            &format!(
                "Related raw artifacts: {}",
                raw_refs
                    .iter()
                    .take(3)
                    .map(|value| inline_clip(value, 64))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "context",
        ));
    }
    items
}

fn render_delegate_step_update(step: &DelegateWorkerStepUpdate) -> String {
    let step_id = step.step_id.as_deref().unwrap_or("step");
    let status = step.status.as_deref().unwrap_or("updated");
    match step.summary.as_deref().filter(|value| !value.is_empty()) {
        Some(summary) => format!(
            "{} [{}] {}",
            inline_clip(step_id, 48),
            inline_clip(status, 24),
            inline_clip(summary, 80)
        ),
        None => format!("{} [{}]", inline_clip(step_id, 48), inline_clip(status, 24)),
    }
}

fn parse_tool_args(raw: &str) -> Option<Value> {
    serde_json::from_str(raw).ok()
}

fn repair_dangling_tool_calls(history: &mut Vec<ChatMessage>) -> usize {
    let mut repaired = 0usize;
    let mut sanitized = Vec::with_capacity(history.len());
    let mut index = 0usize;

    while index < history.len() {
        let message = history[index].clone();
        let Some(tool_calls) = message.tool_calls.clone().filter(|calls| !calls.is_empty()) else {
            if message.role != "tool" {
                sanitized.push(message);
            } else {
                repaired += 1;
            }
            index += 1;
            continue;
        };

        let expected_ids = tool_calls
            .iter()
            .map(|call| call.id.clone())
            .collect::<BTreeSet<_>>();
        let mut seen_ids = BTreeSet::new();
        sanitized.push(message);
        index += 1;

        while index < history.len() && history[index].role == "tool" {
            let tool_message = history[index].clone();
            let tool_call_id = tool_message.tool_call_id.clone().unwrap_or_default();
            if expected_ids.contains(&tool_call_id) {
                seen_ids.insert(tool_call_id);
                sanitized.push(tool_message);
            } else {
                repaired += 1;
            }
            index += 1;
        }

        for missing_id in expected_ids.difference(&seen_ids) {
            repaired += 1;
            sanitized.push(ChatMessage::tool(
                missing_id.clone(),
                "tool call abandoned because a new user turn started before this tool returned output",
            ));
        }
    }

    if repaired > 0 {
        *history = sanitized;
    }
    repaired
}

fn unix_now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn elapsed_ms(started_at: Instant) -> u128 {
    started_at.elapsed().as_millis()
}

struct ApprovalRequiredPayload {
    approval_id: String,
    reason: String,
    command: String,
}

fn parse_approval_required(value: &str) -> Option<ApprovalRequiredPayload> {
    if !value.starts_with("approval_required\n") {
        return None;
    }
    let approval_id = value
        .lines()
        .find_map(|line| line.strip_prefix("approval_id: "))?
        .to_string();
    let reason = value
        .lines()
        .find_map(|line| line.strip_prefix("reason: "))?
        .to_string();
    let command = value
        .lines()
        .find_map(|line| line.strip_prefix("command: "))?
        .to_string();
    Some(ApprovalRequiredPayload {
        approval_id,
        reason,
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Agent, ContextInjection, ContextInjectionStats, PROMPT_HISTORY_MAX_USER_TURNS,
        TurnModelRuntime, apply_goal_state_reconcile_patch, assemble_turn_messages,
        classify_tool_cognition_kind, classify_tool_result_status, continuation_prefix_digest,
        finalize_context_injections, latest_turn_transcript, needs_goal_state_compaction,
        parse_goal_state_compaction_response, parse_goal_state_delta_response,
        parse_goal_state_execution_brief_response, parse_goal_state_reconcile_response,
        parse_tool_result_summary_response, render_context_budget_nudge,
        render_goal_execution_brief_block, render_goal_execution_guidance,
        render_goal_state_delta_block, render_tool_result_summary_for_reconcile,
        repair_dangling_tool_calls, should_parallelize_tool_batch,
        should_reconcile_goal_state_after_tool, should_run_context_compression_before_iteration,
        should_summarize_tool_result_for_reconcile, summarize_active_todos_for_goal_loop,
        summarize_tool_result, summarize_tool_result_for_history, sync_todos_from_goal_state,
        trim_prompt_history,
    };
    use crate::approval::{request_approval, resolve_request, save_pending_approval};
    use crate::config::{AppConfig, AuxiliaryModelConfig};
    use crate::events::{AgentEvent, RecordingEventHandler};
    use crate::goal_state::{CognitionItem, GoalItem, GoalState, HotDataItem};
    use crate::llm::ApiMode;
    use crate::runtime_control::request_stop;
    use crate::runtime_profile::RuntimeProfile;
    use crate::skills::SkillStore;
    use crate::smart_model_routing::{SmartModelRoutingConfig, SmartModelTarget};
    use crate::todo::TodoItem;
    use crate::tools::ToolContext;
    use crate::types::{ChatMessage, ToolCall, ToolFunctionCall};

    fn build_test_config(base_url: &str, root: &std::path::Path) -> AppConfig {
        AppConfig {
            provider_id: "test".to_string(),
            provider_label: "Test".to_string(),
            provider_kind: "openai".to_string(),
            model: "gpt-4.1-mini".to_string(),
            base_url: base_url.to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
            skill_platform: "cli".to_string(),
            workspace_root: root.to_path_buf(),
            data_dir: root.join(".hermes-agent-rs"),
            session_id: None,
            max_iterations: 4,
            system_prompt_override: None,
            tool_allowlist: None,
            enable_shell_tool: false,
            debug_context: false,
            enable_solve_trace_context: false,
            enable_meta_pattern_context: false,
            enable_experience_context: false,
            auxiliary_model: None,
            smart_model_routing: None,
            runtime_profile: RuntimeProfile::fallback(root),
        }
    }

    fn build_tool_context(root: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace_root: root.to_path_buf(),
            data_dir: root.join(".hermes-agent-rs"),
            shell_enabled: false,
            skill_platform: "cli".to_string(),
            provider_id: "test".to_string(),
            model: "gpt-4.1-mini".to_string(),
            base_url: "mock://final-response".to_string(),
            api_key: None,
            max_iterations: 4,
            current_session_id: "test-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    fn build_tool_call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: format!("call-{name}"),
            kind: "function".to_string(),
            function: ToolFunctionCall {
                name: name.to_string(),
                arguments: arguments.to_string(),
            },
        }
    }

    #[test]
    fn classifies_tool_result_status_from_error_markers() {
        assert_eq!(classify_tool_result_status("path: README.md\nok"), "done");
        assert_eq!(
            classify_tool_result_status("tool_error: failed to read missing.txt"),
            "error"
        );
        assert_eq!(
            classify_tool_result_status("status: timeout\nexit_code: -1"),
            "error"
        );
        assert_eq!(
            classify_tool_result_status("status: completed\nexit_code: 2\nstderr:\nfailed"),
            "error"
        );
        assert_eq!(
            classify_tool_result_status(
                "language: bash\nprogram: bash\nstatus: completed\nexit_code: 0"
            ),
            "done"
        );
        assert_eq!(
            classify_tool_result_status("approval denied for command `rm -rf build`: denied"),
            "error"
        );
    }

    #[test]
    fn syncs_goal_state_into_goal_prefixed_todos() {
        let state = GoalState {
            mission: "Implement goal loop".to_string(),
            phase: "act".to_string(),
            current_focus_goal_id: Some("g-current".to_string()),
            reflection: String::new(),
            goals: vec![
                GoalItem {
                    id: "g-current".to_string(),
                    title: "Implement goal loop".to_string(),
                    level: "current".to_string(),
                    status: "in_progress".to_string(),
                    confidence: 0.8,
                    parent_id: None,
                    summary: String::new(),
                    evidence: String::new(),
                    evidence_items: vec![],
                    updated_at_unix: 1,
                },
                GoalItem {
                    id: "g-sub".to_string(),
                    title: "Wire todo sync".to_string(),
                    level: "subgoal".to_string(),
                    status: "blocked".to_string(),
                    confidence: 0.6,
                    parent_id: Some("g-current".to_string()),
                    summary: String::new(),
                    evidence: String::new(),
                    evidence_items: vec![],
                    updated_at_unix: 1,
                },
            ],
            cognition: Vec::new(),
            hot_data: Vec::new(),
            updated_at_unix: 1,
        };
        let mut todos = vec![TodoItem::new("manual-1", "User-managed task", "pending")];

        sync_todos_from_goal_state(&state, &mut todos);

        assert!(
            todos
                .iter()
                .any(|todo| todo.id == "goal:g-current" && todo.status == "in_progress")
        );
        assert!(
            todos
                .iter()
                .any(|todo| todo.id == "goal:g-sub" && todo.status == "blocked")
        );
        assert!(todos.iter().any(|todo| todo.id == "manual-1"));
    }

    #[test]
    fn summarizes_active_todos_for_goal_loop() {
        let todos = vec![
            TodoItem::new("goal:g1", "Implement goal loop", "in_progress"),
            TodoItem::new("goal:g2", "Wait for clarification", "blocked"),
            TodoItem::new("manual", "Already done", "completed"),
        ];

        let summary = summarize_active_todos_for_goal_loop(&todos).expect("summary");
        assert!(summary.contains("Implement goal loop"));
        assert!(summary.contains("blocked"));
        assert!(!summary.contains("Already done"));
    }

    #[test]
    fn classifies_and_summarizes_tool_outcomes() {
        assert_eq!(
            classify_tool_cognition_kind("tool_error: path missing", "done"),
            "risk"
        );
        assert_eq!(
            classify_tool_cognition_kind("no memory results for query `x`", "done"),
            "unknown"
        );
        assert_eq!(
            classify_tool_cognition_kind("saved memory abc", "done"),
            "fact"
        );

        let approval =
            summarize_tool_result("terminal", "needs approval for rm", "approval_required");
        assert!(approval.contains("requires approval"));
        let success = summarize_tool_result("read_file", "contents loaded", "done");
        assert!(success.contains("observed"));
    }

    #[test]
    fn keeps_main_loop_history_on_summaries_for_noisy_tools() {
        let summarized = summarize_tool_result_for_history(
            "terminal",
            "raw\n".repeat(100).as_str(),
            "done",
            Some("summary=Compiler errors cluster in src/types"),
        );
        assert!(summarized.contains("Goal-relevant summary:"));
        assert!(summarized.contains("Compiler errors cluster in src/types"));

        let delegate = summarize_tool_result_for_history(
            "delegate_to_worker",
            r#"{"status":"completed","objective":"Inspect build","worker_result":"{\"summary\":\"Trait mismatch\"}"}"#,
            "done",
            None,
        );
        assert!(delegate.contains("Delegated worker"));
    }

    #[test]
    fn compresses_metadata_tool_results_into_observations_for_history() {
        let summarized = summarize_tool_result_for_history(
            "office_inspect",
            r#"{"format":"pptx","slides":12,"path":"/tmp/demo.pptx","details":{"author":"private-author"}}"#,
            "done",
            None,
        );
        assert!(summarized.contains("Tool `office_inspect` observed"));
        assert!(summarized.contains("Key facts:"));
        assert!(summarized.contains("format: pptx"));
        assert!(summarized.contains("slides: 12"));
        assert!(!summarized.contains("\"details\":{\"author\":\"private-author\"}"));
    }

    #[test]
    fn preserves_content_heavy_tool_results_for_history() {
        let raw = "fn main() {\n    println!(\"hello\");\n}\n";
        let summarized = summarize_tool_result_for_history("read_file", raw, "done", None);
        assert_eq!(summarized, raw);
    }

    #[test]
    fn defaults_to_main_model_first_for_existing_goal_state() {
        assert!(!should_run_context_compression_before_iteration(1));
        assert!(should_run_context_compression_before_iteration(2));
    }

    #[test]
    fn persists_context_debug_snapshot_when_enabled() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.debug_context = true;
        let agent = Agent::new(config).expect("agent");
        let messages = vec![
            ChatMessage::system("system prompt"),
            ChatMessage::user("hello"),
        ];
        let injections = vec![ContextInjection {
            label: "goal_state",
            content: "focus goal".to_string(),
            max_chars: 120,
        }];
        let stats = ContextInjectionStats {
            total_blocks: 1,
            kept_blocks: 1,
            original_chars: 10,
            final_chars: 10,
            clipped_labels: Vec::new(),
            skipped_labels: Vec::new(),
        };

        agent.persist_debug_context_snapshot(
            "main_request_preflight",
            Some(1),
            None,
            Some("gpt-test"),
            Some(1234),
            &messages,
            Some(&injections),
            Some(&stats),
            0,
        );

        let debug_dir = tmp
            .path()
            .join(".hermes-agent-rs")
            .join("runtime")
            .join("context-debug")
            .join(agent.session_id());
        let entries = std::fs::read_dir(&debug_dir)
            .expect("read dir")
            .collect::<Result<Vec<_>, _>>()
            .expect("entries");
        assert_eq!(entries.len(), 1);
        let raw = std::fs::read_to_string(entries[0].path()).expect("read snapshot");
        assert!(raw.contains("\"phase\": \"main_request_preflight\""));
        assert!(raw.contains("\"projected_tokens\": 1234"));
        assert!(raw.contains("\"messages\""));
    }

    #[test]
    fn gates_goal_state_reconcile_to_high_signal_or_error_outcomes() {
        assert!(!should_reconcile_goal_state_after_tool(
            "read_file",
            "contents loaded",
            "done"
        ));
        assert!(should_reconcile_goal_state_after_tool(
            "read_file",
            "no file found for requested path",
            "done"
        ));
        assert!(should_reconcile_goal_state_after_tool(
            "terminal",
            "build output",
            "done"
        ));
        assert!(should_reconcile_goal_state_after_tool(
            "read_file",
            "needs approval",
            "approval_required"
        ));
        assert!(should_summarize_tool_result_for_reconcile(
            "terminal",
            &"line\n".repeat(40)
        ));
    }

    #[test]
    fn parses_and_renders_tool_result_summary() {
        let parsed = parse_tool_result_summary_response(
            r#"{"summary":"Compiler errors cluster in src/types","key_evidence":["trait bound mismatch repeats"],"candidate_hot_data":["errors cluster in types layer"],"candidate_risks":["leaf fixes may hide root cause"]}"#,
        )
        .expect("parsed");
        let rendered = render_tool_result_summary_for_reconcile(&parsed, "terminal", "done")
            .expect("rendered");
        assert!(rendered.contains("summary=Compiler errors cluster in src/types"));
        assert!(rendered.contains("candidate_risks=leaf fixes may hide root cause"));
    }

    #[test]
    fn parses_goal_state_compaction_response_json() {
        let parsed = parse_goal_state_compaction_response(
            r#"{"cognition":"keep the blocker and verified facts","hot_data":"recent tool output and next action","confidence":0.81}"#,
        )
        .expect("parsed");

        assert_eq!(parsed.cognition, "keep the blocker and verified facts");
        assert_eq!(parsed.hot_data, "recent tool output and next action");
        assert_eq!(parsed.confidence, Some(0.81));
    }

    #[test]
    fn parses_and_renders_goal_state_execution_brief_response() {
        let parsed = parse_goal_state_execution_brief_response(
            r#"{"focus":"[goal-current] Implement the request | status=in_progress | confidence=0.82","next_step":"Inspect the relevant module before editing.","watch":"Avoid unrelated cleanup.","operating_rule":"Advance the active goal directly.","confidence":0.84}"#,
        )
        .expect("parsed");
        let rendered = render_goal_execution_brief_block(&parsed);

        assert!(rendered.contains("<execution-brief>"));
        assert!(rendered.contains("next_step: Inspect the relevant module before editing."));
        assert!(rendered.contains("operating_rule: Advance the active goal directly."));
        assert!(rendered.contains("confidence: 0.84"));
    }

    #[test]
    fn parses_and_renders_goal_state_delta_response() {
        let parsed = parse_goal_state_delta_response(
            r#"{"mission":"Implement the current request cleanly","phase":"investigate","current_focus_goal_id":"goal-current","reflection":"Inspect before editing.","rationale":"The request implies implementation but the next safe move is targeted inspection."}"#,
        )
        .expect("parsed");
        let rendered = render_goal_state_delta_block(&parsed);

        assert!(rendered.contains("<state-delta>"));
        assert!(rendered.contains("phase: investigate"));
        assert!(rendered.contains("current_focus_goal_id: goal-current"));
        assert!(rendered.contains("rationale: The request implies implementation"));
    }

    #[test]
    fn parses_goal_state_reconcile_response_json() {
        let parsed = parse_goal_state_reconcile_response(
            r#"{"current_focus_goal_id":"goal-current","goals":[{"id":"goal-current","status":"blocked","confidence":0.4,"evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"A blocker was observed.","relation":"supports","observed_at_unix":123}]}],"cognition":[{"id":"risk-1","kind":"risk","content":"blocked","confidence":0.8,"evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"A blocker was observed.","relation":"supports","observed_at_unix":123}]}],"hot_data":[{"id":"hot-1","content":"needs clarification","confidence":0.7,"source":"reconcile","evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"Clarification is needed next.","relation":"context","observed_at_unix":123}]}]}"#,
        )
        .expect("parsed");

        assert_eq!(
            parsed.current_focus_goal_id.as_deref(),
            Some("goal-current")
        );
        assert_eq!(parsed.goals.len(), 1);
        assert_eq!(parsed.cognition.len(), 1);
        assert_eq!(parsed.hot_data.len(), 1);
        assert_eq!(
            parsed.goals[0].evidence_items.as_ref().map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn applies_goal_state_reconcile_patch_to_existing_goal() {
        let mut state = GoalState {
            mission: "Handle current request".to_string(),
            phase: "investigate".to_string(),
            current_focus_goal_id: Some("goal-current".to_string()),
            reflection: String::new(),
            goals: vec![GoalItem {
                id: "goal-current".to_string(),
                title: "Current goal".to_string(),
                level: "current".to_string(),
                status: "in_progress".to_string(),
                confidence: 0.7,
                parent_id: None,
                summary: "Working".to_string(),
                evidence: "User request".to_string(),
                evidence_items: vec![],
                updated_at_unix: 1,
            }],
            cognition: Vec::new(),
            hot_data: Vec::new(),
            updated_at_unix: 1,
        };

        let patch = parse_goal_state_reconcile_response(
            r#"{"current_focus_goal_id":"goal-current","goals":[{"id":"goal-current","status":"blocked","confidence":0.41,"summary":"Blocked by missing input","evidence":"Tool result found a blocker.","evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"Missing input blocked the goal.","relation":"supports","observed_at_unix":123}]}],"cognition":[{"id":"risk-1","kind":"risk","content":"Missing input is blocking the goal","confidence":0.88,"evidence":"tool","evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"Missing input blocked the goal.","relation":"supports","observed_at_unix":123}]}],"hot_data":[{"id":"hot-1","content":"Need clarification before next step","confidence":0.82,"source":"reconcile","goal_id":"goal-current","evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"Clarification is the next action.","relation":"context","observed_at_unix":123}]}]}"#,
        )
        .expect("patch");

        apply_goal_state_reconcile_patch(&mut state, patch);

        assert_eq!(state.goals[0].status, "blocked");
        assert_eq!(state.goals[0].summary, "Blocked by missing input");
        assert_eq!(state.cognition.len(), 1);
        assert_eq!(state.hot_data.len(), 1);
        assert_eq!(state.goals[0].evidence_items.len(), 1);
        assert_eq!(state.cognition[0].evidence_items.len(), 1);
        assert_eq!(state.hot_data[0].evidence_items.len(), 1);
    }

    #[test]
    fn builds_latest_turn_transcript_from_last_user_turn() {
        let history = vec![
            ChatMessage::user("Earlier request"),
            ChatMessage::assistant("Earlier answer"),
            ChatMessage::user("Current request"),
            ChatMessage::tool("call-1", "tool output"),
            ChatMessage::assistant("Implemented the fix"),
        ];

        let transcript = latest_turn_transcript(&history);
        assert!(!transcript.contains("Earlier request"));
        assert!(transcript.contains("USER: Current request"));
        assert!(transcript.contains("TOOL [call-1]: tool output"));
        assert!(transcript.contains("ASSISTANT: Implemented the fix"));
    }

    #[test]
    fn renders_goal_execution_guidance_with_next_step_and_subgoals() {
        let state = GoalState {
            mission: "Implement goal loop".to_string(),
            phase: "act".to_string(),
            current_focus_goal_id: Some("g-current".to_string()),
            reflection: String::new(),
            goals: vec![
                GoalItem {
                    id: "g-current".to_string(),
                    title: "Implement goal loop".to_string(),
                    level: "current".to_string(),
                    status: "in_progress".to_string(),
                    confidence: 0.83,
                    parent_id: None,
                    summary: "Drive the implementation to completion".to_string(),
                    evidence: String::new(),
                    evidence_items: vec![],
                    updated_at_unix: 10,
                },
                GoalItem {
                    id: "g-sub".to_string(),
                    title: "Wire todo sync".to_string(),
                    level: "subgoal".to_string(),
                    status: "pending".to_string(),
                    confidence: 0.74,
                    parent_id: Some("g-current".to_string()),
                    summary: String::new(),
                    evidence: String::new(),
                    evidence_items: vec![],
                    updated_at_unix: 9,
                },
            ],
            cognition: vec![],
            hot_data: vec![HotDataItem {
                id: "plan-next-step".to_string(),
                content: "Start by wiring the goal-state todo synchronization".to_string(),
                confidence: 0.79,
                source: "goal_state_plan".to_string(),
                goal_id: Some("g-current".to_string()),
                expires_at_unix: None,
                evidence_items: vec![],
                updated_at_unix: 11,
            }],
            updated_at_unix: 11,
        };

        let guidance = render_goal_execution_guidance(&state).expect("guidance");
        assert!(guidance.contains("Focus goal: [g-current] Implement goal loop"));
        assert!(
            guidance.contains("Next step: Start by wiring the goal-state todo synchronization")
        );
        assert!(guidance.contains("Supporting subgoals: Wire todo sync [pending]"));
        assert!(guidance.contains("prioritize actions that directly advance the focus goal"));
    }

    #[test]
    fn renders_blocked_goal_execution_guidance() {
        let state = GoalState {
            mission: "Unblock runtime approval flow".to_string(),
            phase: "investigate".to_string(),
            current_focus_goal_id: Some("g-current".to_string()),
            reflection: String::new(),
            goals: vec![GoalItem {
                id: "g-current".to_string(),
                title: "Unblock runtime approval flow".to_string(),
                level: "current".to_string(),
                status: "blocked".to_string(),
                confidence: 0.51,
                parent_id: None,
                summary: "Blocked on approval handshake details".to_string(),
                evidence: String::new(),
                evidence_items: vec![],
                updated_at_unix: 20,
            }],
            cognition: vec![],
            hot_data: vec![],
            updated_at_unix: 20,
        };

        let guidance = render_goal_execution_guidance(&state).expect("guidance");
        assert!(guidance.contains("Potential blocker: Blocked on approval handshake details"));
        assert!(
            guidance.contains("resolve the blocker or ask for the minimum clarification needed")
        );
    }

    #[test]
    fn detects_when_goal_state_needs_model_compaction() {
        let state = GoalState {
            mission: "Implement goal loop".to_string(),
            phase: "act".to_string(),
            current_focus_goal_id: Some("g1".to_string()),
            reflection: String::new(),
            goals: vec![GoalItem {
                id: "g1".to_string(),
                title: "Implement goal loop".to_string(),
                level: "current".to_string(),
                status: "in_progress".to_string(),
                confidence: 0.8,
                parent_id: None,
                summary: String::new(),
                evidence: String::new(),
                evidence_items: vec![],
                updated_at_unix: 1,
            }],
            cognition: (0..7)
                .map(|idx| CognitionItem {
                    id: format!("c{idx}"),
                    kind: "fact".to_string(),
                    content: format!("verified observation {idx}"),
                    confidence: 0.8,
                    evidence: "tool".to_string(),
                    evidence_items: vec![],
                    updated_at_unix: idx as u64 + 1,
                })
                .collect(),
            hot_data: vec![HotDataItem {
                id: "h1".to_string(),
                content: "recent runtime fact".to_string(),
                confidence: 0.9,
                source: "tool".to_string(),
                goal_id: Some("g1".to_string()),
                expires_at_unix: None,
                evidence_items: vec![],
                updated_at_unix: 1,
            }],
            updated_at_unix: 1,
        };

        assert!(needs_goal_state_compaction(&state));
    }

    #[test]
    fn parallelizes_safe_read_only_batches() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = build_tool_context(tmp.path());
        let calls = vec![
            build_tool_call("list_files", serde_json::json!({ "path": "." })),
            build_tool_call("session_search", serde_json::json!({ "query": "auth bug" })),
            build_tool_call("skills_list", serde_json::json!({})),
        ];

        assert!(should_parallelize_tool_batch(&calls, &ctx));
    }

    #[test]
    fn rejects_overlapping_path_scoped_batches() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = build_tool_context(tmp.path());
        let calls = vec![
            build_tool_call(
                "write_file",
                serde_json::json!({ "path": "src/lib.rs", "content": "alpha" }),
            ),
            build_tool_call(
                "patch_file",
                serde_json::json!({
                    "path": "src/lib.rs",
                    "old_text": "alpha",
                    "new_text": "beta"
                }),
            ),
        ];

        assert!(!should_parallelize_tool_batch(&calls, &ctx));
    }

    #[test]
    fn repairs_dangling_tool_calls_before_new_turn() {
        let mut history = vec![
            ChatMessage::user("make slides"),
            ChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "fc_missing".to_string(),
                    kind: "function".to_string(),
                    function: ToolFunctionCall {
                        name: "office_create".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                tool_call_id: None,
            },
            ChatMessage::user("try again"),
        ];

        let repaired = repair_dangling_tool_calls(&mut history);

        assert_eq!(repaired, 1);
        assert_eq!(history.len(), 4);
        assert_eq!(history[2].role, "tool");
        assert_eq!(history[2].tool_call_id.as_deref(), Some("fc_missing"));
        assert_eq!(history[3].content_text(), "try again");
    }

    #[test]
    fn drops_orphan_tool_outputs_during_repair() {
        let mut history = vec![
            ChatMessage::user("hello"),
            ChatMessage::tool("orphan", "old output"),
            ChatMessage::assistant("done"),
        ];

        let repaired = repair_dangling_tool_calls(&mut history);

        assert_eq!(repaired, 1);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");
    }

    #[test]
    fn rejects_batches_with_non_whitelisted_tools() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = build_tool_context(tmp.path());
        let calls = vec![
            build_tool_call("read_file", serde_json::json!({ "path": "README.md" })),
            build_tool_call(
                "terminal",
                serde_json::json!({ "command": "git status", "workdir": "." }),
            ),
        ];

        assert!(!should_parallelize_tool_batch(&calls, &ctx));
    }

    #[test]
    fn stop_request_marks_parallel_batch_canceled() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut agent =
            Agent::new(build_test_config("mock://final-response", tmp.path())).expect("agent");
        let session_id = agent.session_id().to_string();
        request_stop(&tmp.path().join(".hermes-agent-rs"), &session_id).expect("request stop");

        let mut handler = RecordingEventHandler::new();
        let error = agent
            .finish_parallel_batch_if_stop_requested(
                &mut handler,
                2,
                "parallel-2-call-list",
                1,
                3,
                std::time::Instant::now(),
            )
            .expect_err("stop should abort batch");
        assert!(error.to_string().contains("stop requested"));

        assert!(matches!(
            handler.events().first(),
            Some(AgentEvent::ToolBatchFinished {
                batch_id,
                completed_calls,
                total_calls,
                status,
                ..
            }) if batch_id == "parallel-2-call-list"
                && *completed_calls == 1
                && *total_calls == 3
                && status == "canceled"
        ));
        assert!(matches!(
            handler.events().last(),
            Some(AgentEvent::Error { message, .. })
            if message.contains("stop requested")
        ));
    }

    #[tokio::test]
    async fn resumed_approval_preserves_parallel_tool_metadata() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("hello.txt"), "hello").expect("write");
        let mut agent =
            Agent::new(build_test_config("mock://terminal-approval", tmp.path())).expect("agent");
        agent
            .session
            .history
            .push(ChatMessage::user("delete hello.txt"));

        let data_dir = tmp.path().join(".hermes-agent-rs");
        let approval = request_approval(
            &data_dir,
            agent.session_id(),
            "rm -rf hello.txt",
            "destructive command",
        )
        .expect("request");
        save_pending_approval(
            &data_dir,
            &approval.id,
            agent.session_id(),
            "tool-terminal-1",
            "terminal",
            "parallel",
            Some("parallel-1-call-terminal"),
            Some(2),
            Some(3),
            &serde_json::json!({ "command": "rm -rf hello.txt" }).to_string(),
            "rm -rf hello.txt",
        )
        .expect("pending");
        resolve_request(&data_dir, &approval.id, true).expect("approve");

        let mut handler = RecordingEventHandler::new();
        let response = agent
            .resume_pending_approval_with_handler(&approval.id, &mut handler)
            .await
            .expect("resume");

        assert_eq!(response, "command completed");
        assert!(matches!(
            handler.events().iter().find(|event| matches!(event, AgentEvent::ToolCallStarted { .. })),
            Some(AgentEvent::ToolCallStarted {
                tool_call_id,
                execution_mode,
                batch_id,
                batch_index,
                batch_total,
                ..
            }) if tool_call_id == "tool-terminal-1"
                && execution_mode == "parallel"
                && batch_id.as_deref() == Some("parallel-1-call-terminal")
                && *batch_index == Some(2)
                && *batch_total == Some(3)
        ));
        assert!(matches!(
            handler.events().iter().find(|event| matches!(event, AgentEvent::ToolCallFinished { .. })),
            Some(AgentEvent::ToolCallFinished {
                tool_call_id,
                execution_mode,
                batch_id,
                batch_index,
                batch_total,
                ..
            }) if tool_call_id == "tool-terminal-1"
                && execution_mode == "parallel"
                && batch_id.as_deref() == Some("parallel-1-call-terminal")
                && *batch_index == Some(2)
                && *batch_total == Some(3)
        ));
    }

    #[tokio::test]
    async fn retries_with_reduced_output_budget_after_output_cap_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut agent =
            Agent::new(build_test_config("mock://output-cap-retry", tmp.path())).expect("agent");
        let mut handler = RecordingEventHandler::new();

        let response = agent
            .run_prompt_with_handler("Need a concise fix summary", &mut handler)
            .await
            .expect("response");

        assert_eq!(response, "mock final response after output retry");
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::Nudge { kind, message, .. }
            if kind == "context" && message.contains("输出上限")
        )));
    }

    #[tokio::test]
    async fn debug_context_preview_does_not_auto_inject_skills() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut agent =
            Agent::new(build_test_config("mock://final-response", tmp.path())).expect("agent");
        let store = SkillStore::new_with_platform(
            agent.config.data_dir.clone(),
            Some(&agent.config.skill_platform),
        )
        .expect("store");
        store
            .save(
                "coding",
                "rust-build-debug",
                "Debug Rust build failures with cargo diagnostics.",
                &["rust".to_string(), "build".to_string(), "debug".to_string()],
                "Run cargo check first, then inspect the failing crate and compiler output.",
            )
            .expect("skill");

        let preview = agent
            .debug_context_preview("debug a failing rust build")
            .await
            .expect("preview");

        assert!(preview.matched_skills.is_empty());
        assert!(!preview.injections.iter().any(|item| item.label == "skills"));
    }

    #[tokio::test]
    async fn forces_context_compression_and_retries_after_overflow_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut agent = Agent::new(build_test_config(
            "mock://context-overflow-retry",
            tmp.path(),
        ))
        .expect("agent");
        for idx in 0..14 {
            agent.session.history.push(ChatMessage::user(format!(
                "Earlier user message {idx}: {}",
                "A".repeat(140)
            )));
            agent.session.history.push(ChatMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String(format!(
                    "Earlier assistant message {idx}: {}",
                    "B".repeat(160)
                ))),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        let mut handler = RecordingEventHandler::new();
        let response = agent
            .run_prompt_with_handler("Continue from current state", &mut handler)
            .await
            .expect("response");

        assert_eq!(response, "mock final response after context retry");
        assert!(agent.session.history.iter().any(|message| {
            message.role == "system" && message.content_text().starts_with("[CONTEXT COMPACTION]")
        }));
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::Nudge { kind, message, .. }
            if kind == "context" && message.contains("请求超限后已自动压缩")
        )));
    }

    #[tokio::test]
    async fn persists_detected_context_limit_for_future_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut agent = Agent::new(build_test_config(
            "mock://context-overflow-retry",
            tmp.path(),
        ))
        .expect("agent");
        for idx in 0..14 {
            agent.session.history.push(ChatMessage::user(format!(
                "Earlier user message {idx}: {}",
                "A".repeat(140)
            )));
            agent.session.history.push(ChatMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String(format!(
                    "Earlier assistant message {idx}: {}",
                    "B".repeat(160)
                ))),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        let mut handler = RecordingEventHandler::new();
        let _ = agent
            .run_prompt_with_handler("Continue from current state", &mut handler)
            .await
            .expect("response");

        let cached = crate::context_limit_cache::load_context_length(
            &tmp.path().join(".hermes-agent-rs"),
            "gpt-4.1-mini",
            "mock://context-overflow-retry",
        )
        .expect("load cache");
        assert_eq!(cached, Some(200_000));

        let next_agent = Agent::new(build_test_config(
            "mock://context-overflow-retry",
            tmp.path(),
        ))
        .expect("next agent");
        assert_eq!(next_agent.context_compressor.context_length(), 200_000);
    }

    #[tokio::test]
    async fn retries_after_rate_limit_with_short_backoff() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut agent = Agent::new(build_test_config(
            "mock://rate-limit-then-success",
            tmp.path(),
        ))
        .expect("agent");
        let mut handler = RecordingEventHandler::new();

        let response = agent
            .run_prompt_with_handler("Need a stable answer", &mut handler)
            .await
            .expect("response");

        assert_eq!(response, "mock final response after rate limit retry");
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::Nudge { kind, message, .. }
            if kind == "retry" && message.contains("请求限流")
        )));
    }

    #[tokio::test]
    async fn synthesizes_final_response_after_tool_iteration_limit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("demo.txt"), "demo").expect("fixture");
        let mut config = build_test_config("mock://endless-tool-loop", tmp.path());
        config.max_iterations = 2;
        let mut agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        let response = agent
            .run_prompt_with_handler("keep listing files", &mut handler)
            .await
            .expect("response");

        assert_eq!(response, "mock final response after tool budget");
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::Nudge { kind, message, .. }
            if kind == "loop" && message.contains("工具调用上限")
        )));
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::ModelRequestStarted {
                iteration: 3,
                tool_count: 0,
                ..
            }
        )));
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::AssistantMessage { content, .. }
            if content == "mock final response after tool budget"
        )));
    }

    #[tokio::test]
    async fn uses_auxiliary_model_for_title_generation() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let mut agent = Agent::new(config).expect("agent");

        let response = agent
            .run_prompt("Need help fixing auth wiring")
            .await
            .expect("response");

        assert_eq!(response, "mock final response");
        assert_eq!(
            agent.session.title.as_deref(),
            Some("Auxiliary Session Title")
        );
    }

    #[tokio::test]
    async fn uses_auxiliary_model_for_context_compression() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://context-overflow-retry", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let mut agent = Agent::new(config).expect("agent");
        for idx in 0..14 {
            agent.session.history.push(ChatMessage::user(format!(
                "Earlier user message {idx}: {}",
                "A".repeat(140)
            )));
            agent.session.history.push(ChatMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String(format!(
                    "Earlier assistant message {idx}: {}",
                    "B".repeat(160)
                ))),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        let response = agent
            .run_prompt("Continue from current state")
            .await
            .expect("response");

        assert_eq!(response, "mock final response after context retry");
        assert!(agent.session.history.iter().any(|message| {
            message.role == "system"
                && message
                    .content_text()
                    .contains("Auxiliary summary generated by sidecar model")
        }));
    }

    #[tokio::test]
    async fn uses_auxiliary_model_for_goal_state_compaction() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        agent
            .goal_state_store
            .replace(
                &agent.session.session_id,
                GoalState {
                    mission: "Implement goal loop".to_string(),
                    phase: "act".to_string(),
                    current_focus_goal_id: Some("g1".to_string()),
                    reflection: String::new(),
                    goals: vec![GoalItem {
                        id: "g1".to_string(),
                        title: "Implement goal loop".to_string(),
                        level: "current".to_string(),
                        status: "in_progress".to_string(),
                        confidence: 0.8,
                        parent_id: None,
                        summary: "Need compressed working memory".to_string(),
                        evidence: "user request".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    cognition: (0..7)
                        .map(|idx| CognitionItem {
                            id: format!("c{idx}"),
                            kind: "fact".to_string(),
                            content: format!("verified observation {idx}"),
                            confidence: 0.8,
                            evidence: "tool".to_string(),
                            evidence_items: vec![],
                            updated_at_unix: idx as u64 + 1,
                        })
                        .collect(),
                    hot_data: (0..7)
                        .map(|idx| HotDataItem {
                            id: format!("h{idx}"),
                            content: format!("recent runtime fact {idx}"),
                            confidence: 0.8,
                            source: "tool".to_string(),
                            goal_id: Some("g1".to_string()),
                            expires_at_unix: None,
                            evidence_items: vec![],
                            updated_at_unix: idx as u64 + 1,
                        })
                        .collect(),
                    updated_at_unix: 1,
                },
            )
            .expect("replace");

        let block = agent
            .build_goal_state_context_block(&mut handler, true)
            .await
            .expect("goal-state block");

        assert!(block.contains("Compacted cognition from auxiliary model"));
        assert!(block.contains("Compacted hot data from auxiliary model"));
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, model, .. }
            if purpose == "goal_state_compaction" && model == "gpt-4.1-nano"
        )));
    }

    #[tokio::test]
    async fn uses_auxiliary_model_for_goal_state_execution_brief() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        agent
            .goal_state_store
            .replace(
                &agent.session.session_id,
                GoalState {
                    mission: "Implement the current request".to_string(),
                    phase: "investigate".to_string(),
                    current_focus_goal_id: Some("goal-current".to_string()),
                    reflection: "Keep edits aligned to the active goal.".to_string(),
                    goals: vec![GoalItem {
                        id: "goal-current".to_string(),
                        title: "Current goal".to_string(),
                        level: "current".to_string(),
                        status: "in_progress".to_string(),
                        confidence: 0.83,
                        parent_id: None,
                        summary: "Implement the current request".to_string(),
                        evidence: "user request".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    cognition: vec![CognitionItem {
                        id: "risk-1".to_string(),
                        kind: "risk".to_string(),
                        content: "Avoid drifting into unrelated cleanup".to_string(),
                        confidence: 0.74,
                        evidence: "project state".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    hot_data: vec![HotDataItem {
                        id: "next-step".to_string(),
                        content: "Inspect the relevant code path before editing.".to_string(),
                        confidence: 0.79,
                        source: "goal_state".to_string(),
                        goal_id: Some("goal-current".to_string()),
                        expires_at_unix: None,
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    updated_at_unix: 1,
                },
            )
            .expect("replace");

        let block = agent
            .build_execution_brief_context_block(&mut handler, "implement the first version")
            .await
            .expect("execution brief");

        assert!(block.contains("<execution-brief>"));
        assert!(block.contains("next_step: Inspect the relevant code path before editing."));
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, model, .. }
            if purpose == "goal_state_execution_brief" && model == "gpt-4.1-nano"
        )));
    }

    #[tokio::test]
    async fn uses_auxiliary_model_for_goal_state_delta() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        agent
            .goal_state_store
            .replace(
                &agent.session.session_id,
                GoalState {
                    mission: "Implement the current request".to_string(),
                    phase: "act".to_string(),
                    current_focus_goal_id: Some("goal-current".to_string()),
                    reflection: "Need to keep edits focused.".to_string(),
                    goals: vec![GoalItem {
                        id: "goal-current".to_string(),
                        title: "Current goal".to_string(),
                        level: "current".to_string(),
                        status: "in_progress".to_string(),
                        confidence: 0.83,
                        parent_id: None,
                        summary: "Implement the current request".to_string(),
                        evidence: "user request".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    cognition: vec![],
                    hot_data: vec![],
                    updated_at_unix: 1,
                },
            )
            .expect("replace");

        let block = agent
            .build_goal_state_delta_context_block(&mut handler, "implement the first version")
            .await
            .expect("state delta");

        assert!(block.contains("<state-delta>"));
        assert!(block.contains("phase: investigate"));
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, model, .. }
            if purpose == "goal_state_delta" && model == "gpt-4.1-nano"
        )));
    }

    #[tokio::test]
    async fn skips_goal_state_compaction_for_first_pass_context_block() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        agent
            .goal_state_store
            .replace(
                &agent.session.session_id,
                GoalState {
                    mission: "Implement goal loop".to_string(),
                    phase: "act".to_string(),
                    current_focus_goal_id: Some("g1".to_string()),
                    reflection: String::new(),
                    goals: vec![GoalItem {
                        id: "g1".to_string(),
                        title: "Implement goal loop".to_string(),
                        level: "current".to_string(),
                        status: "in_progress".to_string(),
                        confidence: 0.8,
                        parent_id: None,
                        summary: "Need compressed working memory".to_string(),
                        evidence: "user request".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    cognition: (0..7)
                        .map(|idx| CognitionItem {
                            id: format!("c{idx}"),
                            kind: "fact".to_string(),
                            content: format!("verified observation {idx}"),
                            confidence: 0.8,
                            evidence: "tool".to_string(),
                            evidence_items: vec![],
                            updated_at_unix: idx as u64 + 1,
                        })
                        .collect(),
                    hot_data: (0..7)
                        .map(|idx| HotDataItem {
                            id: format!("h{idx}"),
                            content: format!("recent runtime fact {idx}"),
                            confidence: 0.8,
                            source: "tool".to_string(),
                            goal_id: Some("g1".to_string()),
                            expires_at_unix: None,
                            evidence_items: vec![],
                            updated_at_unix: idx as u64 + 1,
                        })
                        .collect(),
                    updated_at_unix: 1,
                },
            )
            .expect("replace");

        let block = agent
            .build_goal_state_context_block(&mut handler, false)
            .await
            .expect("goal-state block");

        assert!(block.contains("next_step_hint:"));
        assert!(block.contains("key_beliefs:"));
        assert!(!block.contains("Compacted cognition from auxiliary model"));
        assert!(!handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, .. }
            if purpose == "goal_state_compaction"
        )));
    }

    #[tokio::test]
    async fn uses_auxiliary_model_for_goal_state_reconcile() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        agent
            .goal_state_store
            .replace(
                &agent.session.session_id,
                GoalState {
                    mission: "Current goal".to_string(),
                    phase: "investigate".to_string(),
                    current_focus_goal_id: Some("goal-current".to_string()),
                    reflection: String::new(),
                    goals: vec![GoalItem {
                        id: "goal-current".to_string(),
                        title: "Current goal".to_string(),
                        level: "current".to_string(),
                        status: "in_progress".to_string(),
                        confidence: 0.7,
                        parent_id: None,
                        summary: "Working".to_string(),
                        evidence: "user request".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    cognition: Vec::new(),
                    hot_data: Vec::new(),
                    updated_at_unix: 1,
                },
            )
            .expect("replace");

        agent.update_goal_state_from_tool_outcome(
            "tool-1",
            "read_file",
            "no file found for requested path",
            "sequential",
            "done",
        );
        agent
            .reconcile_goal_state_after_tool_outcome(
                &mut handler,
                "tool-1",
                "read_file",
                "no file found for requested path",
                "sequential",
                "done",
            )
            .await;

        let state = agent
            .goal_state_store
            .load(&agent.session.session_id)
            .expect("load");
        let goal = state
            .goals
            .iter()
            .find(|goal| goal.id == "goal-current")
            .expect("goal");
        assert_eq!(goal.status, "blocked");
        assert!(
            state
                .cognition
                .iter()
                .any(|item| item.id == "reconcile:blocker")
        );
        assert!(
            state
                .hot_data
                .iter()
                .any(|item| item.id == "reconcile:next-step")
        );
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, model, .. }
            if purpose == "goal_state_reconcile" && model == "gpt-4.1-nano"
        )));
    }

    #[tokio::test]
    async fn skips_goal_state_reconcile_for_low_signal_tool_success() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        agent
            .goal_state_store
            .replace(
                &agent.session.session_id,
                GoalState {
                    mission: "Current goal".to_string(),
                    phase: "investigate".to_string(),
                    current_focus_goal_id: Some("goal-current".to_string()),
                    reflection: String::new(),
                    goals: vec![GoalItem {
                        id: "goal-current".to_string(),
                        title: "Current goal".to_string(),
                        level: "current".to_string(),
                        status: "in_progress".to_string(),
                        confidence: 0.7,
                        parent_id: None,
                        summary: "Working".to_string(),
                        evidence: "user request".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    cognition: Vec::new(),
                    hot_data: Vec::new(),
                    updated_at_unix: 1,
                },
            )
            .expect("replace");

        agent.update_goal_state_from_tool_outcome(
            "tool-1",
            "read_file",
            "contents loaded",
            "sequential",
            "done",
        );
        agent
            .reconcile_goal_state_after_tool_outcome(
                &mut handler,
                "tool-1",
                "read_file",
                "contents loaded",
                "sequential",
                "done",
            )
            .await;

        assert!(!handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, .. }
            if purpose == "goal_state_reconcile" || purpose == "tool_result_summary"
        )));
    }

    #[tokio::test]
    async fn uses_auxiliary_model_for_tool_result_summary_when_result_is_noisy() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        agent
            .goal_state_store
            .replace(
                &agent.session.session_id,
                GoalState {
                    mission: "Current goal".to_string(),
                    phase: "investigate".to_string(),
                    current_focus_goal_id: Some("goal-current".to_string()),
                    reflection: String::new(),
                    goals: vec![GoalItem {
                        id: "goal-current".to_string(),
                        title: "Current goal".to_string(),
                        level: "current".to_string(),
                        status: "in_progress".to_string(),
                        confidence: 0.7,
                        parent_id: None,
                        summary: "Working".to_string(),
                        evidence: "user request".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    cognition: Vec::new(),
                    hot_data: Vec::new(),
                    updated_at_unix: 1,
                },
            )
            .expect("replace");

        let result = format!(
            "tool_error: build failed\n{}",
            "trait bound mismatch\n".repeat(40)
        );
        agent.update_goal_state_from_tool_outcome(
            "tool-1",
            "terminal",
            &result,
            "sequential",
            "done",
        );
        agent
            .reconcile_goal_state_after_tool_outcome(
                &mut handler,
                "tool-1",
                "terminal",
                &result,
                "sequential",
                "done",
            )
            .await;

        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, model, .. }
            if purpose == "tool_result_summary" && model == "gpt-4.1-nano"
        )));
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, model, .. }
            if purpose == "goal_state_reconcile" && model == "gpt-4.1-nano"
        )));
    }

    #[tokio::test]
    async fn skips_goal_state_plan_when_existing_focus_is_already_available() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let mut agent = Agent::new(config).expect("agent");
        agent
            .goal_state_store
            .replace(
                &agent.session.session_id,
                GoalState {
                    mission: "Current goal".to_string(),
                    phase: "act".to_string(),
                    current_focus_goal_id: Some("goal-current".to_string()),
                    reflection: String::new(),
                    goals: vec![GoalItem {
                        id: "goal-current".to_string(),
                        title: "Current goal".to_string(),
                        level: "current".to_string(),
                        status: "in_progress".to_string(),
                        confidence: 0.7,
                        parent_id: None,
                        summary: "Working".to_string(),
                        evidence: "user request".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    cognition: Vec::new(),
                    hot_data: Vec::new(),
                    updated_at_unix: 1,
                },
            )
            .expect("replace");

        let mut handler = RecordingEventHandler::new();
        let _ = agent
            .run_prompt_with_handler("Continue with the current goal", &mut handler)
            .await
            .expect("response");

        assert!(!handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, .. }
            if purpose == "goal_state_plan"
        )));
    }

    #[tokio::test]
    async fn skips_goal_state_plan_before_first_response_even_for_new_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let mut agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        let _ = agent
            .run_prompt_with_handler("Start a new task from scratch", &mut handler)
            .await
            .expect("response");

        assert!(!handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, .. }
            if purpose == "goal_state_plan"
        )));
    }

    #[test]
    fn updates_goal_state_from_delegate_worker_result() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let agent =
            Agent::new(build_test_config("mock://final-response", tmp.path())).expect("agent");

        agent
            .goal_state_store
            .replace(
                &agent.session.session_id,
                GoalState {
                    mission: "Fix the build".to_string(),
                    phase: "investigate".to_string(),
                    current_focus_goal_id: Some("goal-current".to_string()),
                    reflection: String::new(),
                    goals: vec![GoalItem {
                        id: "goal-current".to_string(),
                        title: "Fix the build".to_string(),
                        level: "current".to_string(),
                        status: "in_progress".to_string(),
                        confidence: 0.7,
                        parent_id: None,
                        summary: "Investigate compile failures".to_string(),
                        evidence: "user request".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    cognition: Vec::new(),
                    hot_data: Vec::new(),
                    updated_at_unix: 1,
                },
            )
            .expect("replace");
        let _ = agent.solve_trace_store.start_episode(
            &agent.session.session_id,
            "turn-1",
            "turn-1",
            "Fix the build",
            "Investigate compile failures",
            Some("goal-current"),
            Some("Fix the build"),
        );

        let delegate_result = serde_json::json!({
            "status": "completed",
            "objective": "Diagnose the current Rust build failure",
            "focus_goal_id": "goal-current",
            "worker_result": serde_json::json!({
                "summary": "Build failures cluster around trait bound mismatches in the types layer.",
                "key_evidence": [
                    "cargo check errors cluster in src/types/mod.rs",
                    "the failures began after a shared trait signature change"
                ],
                "candidate_beliefs": [
                    "The shared trait signature change is the primary root cause."
                ],
                "candidate_risks": [
                    "Fixing individual implementations first may mask the real issue."
                ],
                "step_updates": [
                    {
                        "step_id": "inspect-build-errors",
                        "status": "completed",
                        "summary": "Grouped the compiler errors by module and error family."
                    }
                ],
                "recommended_next_actions": [
                    "Compare the shared trait definition against every implementor."
                ],
                "raw_refs": [
                    "artifact://cargo/123"
                ]
            })
            .to_string()
        })
        .to_string();

        agent.update_goal_state_from_tool_outcome(
            "tool-1",
            "delegate_to_worker",
            &delegate_result,
            "sequential",
            "done",
        );

        let state = agent
            .goal_state_store
            .load(&agent.session.session_id)
            .expect("load");
        assert!(state.cognition.iter().any(|item| {
            item.id == "delegate-worker:tool-1:summary"
                && item.content.contains("trait bound mismatches")
        }));
        assert!(state.cognition.iter().any(|item| {
            item.id == "delegate-worker:tool-1:belief:0"
                && item.content.contains("primary root cause")
        }));
        assert!(state.cognition.iter().any(|item| {
            item.id == "delegate-worker:tool-1:risk:0"
                && item.kind == "risk"
                && item.content.contains("mask the real issue")
        }));
        assert!(state.hot_data.iter().any(|item| {
            item.id == "delegate-worker:tool-1:next-actions"
                && item.content.contains("Compare the shared trait definition")
        }));
        assert!(state.hot_data.iter().any(|item| {
            item.id == "delegate-worker:tool-1:steps"
                && item.content.contains("inspect-build-errors")
        }));

        let todos = agent
            .todo_store
            .load(&agent.session.session_id)
            .expect("load todos");
        assert!(todos.iter().any(|todo| {
            todo.id == "worker:tool-1:inspect-build-errors"
                && todo.status == "completed"
                && todo.content.contains("Grouped the compiler errors")
        }));

        let trace = agent
            .solve_trace_store
            .load(&agent.session.session_id)
            .expect("load trace");
        let episode = trace
            .episodes
            .iter()
            .find(|episode| episode.id == "turn-1")
            .expect("episode");
        assert!(episode.steps.iter().any(|step| {
            step.id == "delegate:tool-1"
                && step
                    .action
                    .contains("Diagnose the current Rust build failure")
        }));
        assert!(episode.decisions.iter().any(|decision| {
            decision.id == "delegate:tool-1:next"
                && decision
                    .chosen
                    .contains("Compare the shared trait definition")
        }));
        assert!(
            episode
                .supplements
                .iter()
                .any(|item| item.contains("primary root cause"))
        );

        agent.record_turn_solve_outcome("Inspect the shared trait definition next.");
        let experiences = agent
            .experience_store
            .load(&agent.session.session_id)
            .expect("load experiences");
        assert!(experiences.records.iter().any(|record| {
            record.source_episode_id == "turn-1"
                && record.problem_frame.contains("Fix the build")
                && record
                    .successful_strategy
                    .iter()
                    .any(|item| item.contains("Compare the shared trait definition"))
        }));

        let meta_patterns = agent
            .meta_pattern_store
            .load(&agent.session.session_id)
            .expect("load meta patterns");
        assert!(meta_patterns.patterns.iter().any(|pattern| {
            pattern.problem_cluster.contains("Fix the build")
                && pattern
                    .recommended_strategies
                    .iter()
                    .any(|item| item.contains("compare the shared trait definition"))
        }));
    }

    #[tokio::test]
    async fn uses_auxiliary_model_for_turn_end_goal_state_reconcile() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://final-response", tmp.path());
        config.auxiliary_model = Some(AuxiliaryModelConfig {
            model: "gpt-4.1-nano".to_string(),
            base_url: "mock://auxiliary-summary-title".to_string(),
            api_key: None,
            api_mode: ApiMode::ChatCompletions,
        });
        let mut agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        agent
            .goal_state_store
            .replace(
                &agent.session.session_id,
                GoalState {
                    mission: "Current goal".to_string(),
                    phase: "act".to_string(),
                    current_focus_goal_id: Some("goal-current".to_string()),
                    reflection: String::new(),
                    goals: vec![GoalItem {
                        id: "goal-current".to_string(),
                        title: "Current goal".to_string(),
                        level: "current".to_string(),
                        status: "in_progress".to_string(),
                        confidence: 0.7,
                        parent_id: None,
                        summary: "Working".to_string(),
                        evidence: "user request".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 1,
                    }],
                    cognition: Vec::new(),
                    hot_data: Vec::new(),
                    updated_at_unix: 1,
                },
            )
            .expect("replace");
        agent
            .session
            .history
            .push(ChatMessage::user("Need the feature implemented"));
        agent.session.history.push(ChatMessage::assistant(
            "Implemented the feature and verified the result",
        ));

        agent
            .reconcile_goal_state_after_turn(
                &mut handler,
                "Need the feature implemented",
                "Implemented the feature and verified the result",
            )
            .await;

        let state = agent
            .goal_state_store
            .load(&agent.session.session_id)
            .expect("load");
        let goal = state
            .goals
            .iter()
            .find(|goal| goal.id == "goal-current")
            .expect("goal");
        assert_eq!(goal.status, "succeeded");
        assert!(
            state
                .cognition
                .iter()
                .any(|item| item.id == "turn-end:completion")
        );
        assert!(
            state
                .hot_data
                .iter()
                .any(|item| item.id == "turn-end:result")
        );
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::BackgroundModelRequestStarted { purpose, model, .. }
            if purpose == "goal_state_turn_reconcile" && model == "gpt-4.1-nano"
        )));
    }

    #[tokio::test]
    async fn routes_simple_turns_to_cheap_model() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://output-cap-retry", tmp.path());
        config.smart_model_routing = Some(SmartModelRoutingConfig {
            max_simple_chars: 160,
            max_simple_words: 28,
            cheap_model: SmartModelTarget {
                provider: None,
                model: "gpt-4.1-nano".to_string(),
                base_url: Some("mock://final-response".to_string()),
                api_key: None,
                api_key_env: None,
            },
        });
        let mut agent = Agent::new(config).expect("agent");
        let mut handler = RecordingEventHandler::new();

        let response = agent
            .run_prompt_with_handler("Summarize this.", &mut handler)
            .await
            .expect("response");

        assert_eq!(response, "mock final response");
        assert!(handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::Nudge { kind, message, .. }
            if kind == "routing" && message.contains("gpt-4.1-nano")
        )));
        assert!(!handler.events().iter().any(|event| matches!(
            event,
            AgentEvent::Nudge { kind, message, .. }
            if kind == "context" && message.contains("输出上限")
        )));
    }

    #[test]
    fn applies_context_injection_budgets() {
        let injections = vec![
            ContextInjection {
                label: "goal_state",
                content: "A".repeat(260),
                max_chars: 220,
            },
            ContextInjection {
                label: "todo",
                content: "B".repeat(260),
                max_chars: 220,
            },
            ContextInjection {
                label: "plugin_context",
                content: "C".repeat(260),
                max_chars: 220,
            },
        ];

        let (rendered, stats) = finalize_context_injections(injections, 500);

        assert_eq!(rendered.len(), 2);
        assert_eq!(stats.total_blocks, 3);
        assert_eq!(stats.kept_blocks, 2);
        assert!(stats.clipped_labels.contains(&"goal_state"));
        assert!(stats.clipped_labels.contains(&"todo"));
        assert!(stats.skipped_labels.contains(&"plugin_context"));
        assert!(stats.final_chars <= 500);
        assert!(render_context_budget_nudge(&stats).contains("plugin_context"));
    }

    #[test]
    fn assembles_turn_messages_into_latest_user_message() {
        let history = vec![
            ChatMessage::user("earlier request"),
            ChatMessage::assistant("earlier answer"),
            ChatMessage::user("latest request"),
        ];
        let injections = vec![ContextInjection {
            label: "goal_state",
            content: "<goal-state>focus</goal-state>".to_string(),
            max_chars: 220,
        }];

        let (messages, stats) = assemble_turn_messages("system prompt", &history, &injections, 200);

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1].content_text(), "earlier request");
        assert!(messages[3].content_text().contains("latest request"));
        assert!(
            messages[3]
                .content_text()
                .contains("<goal-state>focus</goal-state>")
        );
        assert_eq!(stats.kept_blocks, 1);
    }

    #[test]
    fn trims_prompt_history_to_recent_five_user_turns() {
        let mut history = Vec::new();
        for turn in 1..=7 {
            history.push(ChatMessage::user(format!("user turn {turn}")));
            history.push(ChatMessage::assistant(format!("assistant turn {turn}")));
        }

        let trimmed = trim_prompt_history(&history, 5);

        assert_eq!(trimmed.len(), 10);
        assert_eq!(
            trimmed.first().map(ChatMessage::content_text).as_deref(),
            Some("user turn 3")
        );
        assert_eq!(
            trimmed.last().map(ChatMessage::content_text).as_deref(),
            Some("assistant turn 7")
        );
    }

    #[test]
    fn rebuild_retry_messages_reuses_trimmed_prompt_history() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = build_test_config("mock://default", tmp.path());
        let mut agent = Agent::new(config).expect("agent");
        for turn in 1..=7 {
            agent
                .session
                .history
                .push(ChatMessage::user(format!("user turn {turn}")));
            agent
                .session
                .history
                .push(ChatMessage::assistant(format!("assistant turn {turn}")));
        }

        let previous_messages = vec![
            ChatMessage::system("system prompt"),
            ChatMessage::user("user turn 7\n\n# goal_state\nfocus"),
        ];

        let rebuilt = agent.rebuild_retry_messages(&previous_messages, "user turn 7");

        assert_eq!(
            rebuilt.first().map(|message| message.role.as_str()),
            Some("system")
        );
        assert_eq!(
            rebuilt
                .iter()
                .filter(|message| message.role == "user")
                .count(),
            PROMPT_HISTORY_MAX_USER_TURNS
        );
        assert_eq!(
            rebuilt.get(1).map(ChatMessage::content_text).as_deref(),
            Some("user turn 3")
        );
        assert_eq!(
            rebuilt.last().map(ChatMessage::content_text).as_deref(),
            Some("assistant turn 7")
        );
        assert!(
            rebuilt
                .iter()
                .rev()
                .find(|message| message.role == "user")
                .map(ChatMessage::content_text)
                .unwrap_or_default()
                .contains("# goal_state\nfocus")
        );
    }

    #[test]
    fn continuation_prefix_digest_changes_when_history_before_latest_assistant_changes() {
        let original = vec![
            ChatMessage::system("system prompt"),
            ChatMessage::user("earlier request"),
            ChatMessage::assistant("earlier answer"),
            ChatMessage::user("latest request"),
        ];
        let mut changed = original.clone();
        changed[0] = ChatMessage::system("system prompt with extra guidance");

        let original_digest =
            continuation_prefix_digest(&original).expect("original continuation digest");
        let changed_digest =
            continuation_prefix_digest(&changed).expect("changed continuation digest");

        assert_ne!(original_digest, changed_digest);
    }

    #[test]
    fn disables_response_continuation_for_non_official_responses_endpoints() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = build_test_config("https://mdlbus.com/v1", tmp.path());
        let mut agent = Agent::new(config).expect("agent");
        agent.session.set_latest_response_state(
            Some("resp_test".to_string()),
            Some("https://mdlbus.com/v1|gpt-test".to_string()),
            continuation_prefix_digest(&agent.session.history),
        );

        let runtime = TurnModelRuntime {
            client: agent.client.clone(),
            model: "gpt-test".to_string(),
            base_url: "https://mdlbus.com/v1".to_string(),
            api_mode: crate::llm::ApiMode::Responses,
            routed_label: None,
            uses_primary: true,
        };

        assert_eq!(agent.response_continuation_id_for_runtime(&runtime), None);
    }

    #[tokio::test]
    async fn keeps_complex_turns_on_primary_model() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut config = build_test_config("mock://output-cap-retry", tmp.path());
        config.smart_model_routing = Some(SmartModelRoutingConfig {
            max_simple_chars: 160,
            max_simple_words: 28,
            cheap_model: SmartModelTarget {
                provider: None,
                model: "gpt-4.1-nano".to_string(),
                base_url: Some("mock://final-response".to_string()),
                api_key: None,
                api_key_env: None,
            },
        });
        let mut agent = Agent::new(config).expect("agent");

        let response = agent
            .run_prompt("Debug this stacktrace and patch the error")
            .await
            .expect("response");

        assert_eq!(response, "mock final response after output retry");
    }
}
