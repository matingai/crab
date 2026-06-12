use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ContextSourceSummary {
    pub label: String,
    pub status: String,
    pub original_chars: usize,
    pub final_chars: usize,
    pub max_chars: usize,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    SessionReady {
        session_id: String,
        resumed: bool,
    },
    TurnStarted {
        session_id: String,
        turn_id: String,
        user_input_preview: String,
        input_chars: usize,
        resumed: bool,
    },
    TurnFinished {
        session_id: String,
        turn_id: String,
        status: String,
        duration_ms: u128,
        tool_call_count: usize,
        response_preview: String,
    },
    TurnInterrupted {
        session_id: String,
        turn_id: String,
        phase: String,
        reason: String,
        message: String,
    },
    IterationStarted {
        session_id: String,
        iteration: usize,
        max_iterations: usize,
    },
    AssistantDelta {
        session_id: String,
        iteration: usize,
        delta: String,
    },
    SkillMatched {
        session_id: String,
        skills: Vec<String>,
    },
    GoalStateUpdated {
        session_id: String,
        source: String,
        phase: String,
        mission_preview: String,
        focus_goal_id: Option<String>,
        focus_goal_title: Option<String>,
        focus_goal_status: Option<String>,
        focus_goal_confidence: Option<f32>,
        goal_count: usize,
        active_goal_count: usize,
        blocked_goal_count: usize,
        cognition_count: usize,
        hot_data_count: usize,
    },
    TodoStateUpdated {
        session_id: String,
        source: String,
        total: usize,
        pending: usize,
        in_progress: usize,
        blocked: usize,
        completed: usize,
        cancelled: usize,
        active_count: usize,
        active_preview: Vec<String>,
    },
    SolveTraceUpdated {
        session_id: String,
        episode_id: String,
        turn_id: String,
        source: String,
        entry_kind: String,
        entry_id: Option<String>,
        status: String,
        goal_preview: String,
        action_preview: String,
        observation_preview: String,
        step_count: usize,
        decision_count: usize,
        supplement_count: usize,
    },
    ContextPrepared {
        session_id: String,
        phase: String,
        iteration: Option<usize>,
        projected_tokens: usize,
        request_budget_tokens: usize,
        message_count: usize,
        tool_count: usize,
        total_blocks: usize,
        kept_blocks: usize,
        original_chars: usize,
        final_chars: usize,
        clipped_labels: Vec<String>,
        skipped_labels: Vec<String>,
        duration_ms: u128,
    },
    ContextSourcesUpdated {
        session_id: String,
        phase: String,
        iteration: Option<usize>,
        total_blocks: usize,
        kept_blocks: usize,
        clipped_count: usize,
        skipped_count: usize,
        original_chars: usize,
        final_chars: usize,
        sources: Vec<ContextSourceSummary>,
    },
    ContextCompacted {
        session_id: String,
        reason: String,
        original_message_count: usize,
        compressed_message_count: usize,
        original_estimated_tokens: usize,
        compressed_estimated_tokens: usize,
        pruned_tool_messages: usize,
        used_summary: bool,
    },
    ModelRecovery {
        session_id: String,
        iteration: usize,
        model: String,
        kind: String,
        action: String,
        attempt: usize,
        delay_ms: Option<u64>,
        output_budget_tokens: Option<usize>,
        context_limit_tokens: Option<usize>,
        error_preview: String,
    },
    ModelRequestStarted {
        session_id: String,
        iteration: usize,
        model: String,
        api_mode: String,
        message_count: usize,
        tool_count: usize,
        output_budget_tokens: Option<usize>,
        uses_response_continuation: bool,
    },
    ModelRequestFinished {
        session_id: String,
        iteration: usize,
        model: String,
        status: String,
        duration_ms: u128,
        tool_call_count: usize,
        prompt_tokens: Option<usize>,
        completion_tokens: Option<usize>,
        total_tokens: Option<usize>,
        content_preview: String,
    },
    BackgroundModelRequestStarted {
        session_id: String,
        purpose: String,
        model: String,
        api_mode: String,
        message_count: usize,
    },
    BackgroundModelRequestFinished {
        session_id: String,
        purpose: String,
        model: String,
        status: String,
        duration_ms: u128,
        prompt_tokens: Option<usize>,
        completion_tokens: Option<usize>,
        total_tokens: Option<usize>,
        content_preview: String,
    },
    SkillLifecycleSuggested {
        session_id: String,
        action: String,
        category: String,
        name: String,
        description: String,
        keywords: Vec<String>,
        task_kinds: Vec<String>,
        requires_tools: Vec<String>,
        requires_shell: bool,
        reason: String,
    },
    DelegateRunUpdated {
        session_id: String,
        iteration: usize,
        tool_call_id: String,
        delegate_run_id: String,
        delegate_session_id: String,
        parent_delegate_run_id: Option<String>,
        root_delegate_run_id: String,
        status: String,
        source: String,
        attempt: usize,
        max_iterations: usize,
        objective_preview: String,
        result_preview: String,
        execution_mode: String,
        batch_id: Option<String>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    },
    ToolBatchStarted {
        session_id: String,
        iteration: usize,
        batch_id: String,
        total_calls: usize,
    },
    ToolBatchProgress {
        session_id: String,
        iteration: usize,
        batch_id: String,
        completed_calls: usize,
        total_calls: usize,
    },
    ToolBatchFinished {
        session_id: String,
        iteration: usize,
        batch_id: String,
        completed_calls: usize,
        total_calls: usize,
        status: String,
        duration_ms: u128,
    },
    ToolCallStarted {
        session_id: String,
        iteration: usize,
        tool_call_id: String,
        tool_name: String,
        arguments_preview: String,
        execution_mode: String,
        batch_id: Option<String>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    },
    ToolCallDelta {
        session_id: String,
        iteration: usize,
        tool_call_id: String,
        tool_name: String,
        detail_preview: String,
        execution_mode: String,
        batch_id: Option<String>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    },
    ToolCallFinished {
        session_id: String,
        iteration: usize,
        tool_call_id: String,
        tool_name: String,
        status: String,
        duration_ms: u128,
        output_preview: String,
        execution_mode: String,
        batch_id: Option<String>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    },
    ApprovalRequired {
        session_id: String,
        tool_call_id: String,
        tool_name: String,
        approval_id: String,
        reason: String,
        command: String,
        execution_mode: String,
        batch_id: Option<String>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    },
    ApprovalResolved {
        session_id: String,
        tool_call_id: String,
        tool_name: String,
        approval_id: String,
        status: String,
        approved: bool,
        execution_mode: String,
        batch_id: Option<String>,
        batch_index: Option<usize>,
        batch_total: Option<usize>,
    },
    AssistantMessage {
        session_id: String,
        content: String,
    },
    SessionSaved {
        session_id: String,
        path: String,
        path_preview: String,
        turn_id: String,
        history_count: usize,
        timeline_count: usize,
        pending_approval_count: usize,
        has_response_continuation: bool,
        updated_at_unix: u64,
    },
    Nudge {
        session_id: String,
        kind: String,
        message: String,
    },
    Error {
        session_id: String,
        message: String,
    },
}

impl AgentEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::SessionReady { .. } => "session_ready",
            Self::TurnStarted { .. } => "turn_started",
            Self::TurnFinished { .. } => "turn_finished",
            Self::TurnInterrupted { .. } => "turn_interrupted",
            Self::IterationStarted { .. } => "iteration_started",
            Self::AssistantDelta { .. } => "assistant_delta",
            Self::SkillMatched { .. } => "skill_matched",
            Self::GoalStateUpdated { .. } => "goal_state_updated",
            Self::TodoStateUpdated { .. } => "todo_state_updated",
            Self::SolveTraceUpdated { .. } => "solve_trace_updated",
            Self::ContextPrepared { .. } => "context_prepared",
            Self::ContextSourcesUpdated { .. } => "context_sources_updated",
            Self::ContextCompacted { .. } => "context_compacted",
            Self::ModelRecovery { .. } => "model_recovery",
            Self::ModelRequestStarted { .. } => "model_request_started",
            Self::ModelRequestFinished { .. } => "model_request_finished",
            Self::BackgroundModelRequestStarted { .. } => "background_model_request_started",
            Self::BackgroundModelRequestFinished { .. } => "background_model_request_finished",
            Self::SkillLifecycleSuggested { .. } => "skill_lifecycle_suggested",
            Self::DelegateRunUpdated { .. } => "delegate_run_updated",
            Self::ToolBatchStarted { .. } => "tool_batch_started",
            Self::ToolBatchProgress { .. } => "tool_batch_progress",
            Self::ToolBatchFinished { .. } => "tool_batch_finished",
            Self::ToolCallStarted { .. } => "tool_call_started",
            Self::ToolCallDelta { .. } => "tool_call_delta",
            Self::ToolCallFinished { .. } => "tool_call_finished",
            Self::ApprovalRequired { .. } => "approval_required",
            Self::ApprovalResolved { .. } => "approval_resolved",
            Self::AssistantMessage { .. } => "assistant_message",
            Self::SessionSaved { .. } => "session_saved",
            Self::Nudge { .. } => "nudge",
            Self::Error { .. } => "error",
        }
    }
}

pub trait EventHandler: Send {
    fn on_event(&mut self, event: AgentEvent);
}

#[derive(Default)]
pub struct NoopEventHandler;

impl EventHandler for NoopEventHandler {
    fn on_event(&mut self, _event: AgentEvent) {}
}

#[derive(Default)]
pub struct RecordingEventHandler {
    events: Vec<AgentEvent>,
}

impl RecordingEventHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_events(self) -> Vec<AgentEvent> {
        self.events
    }

    pub fn events(&self) -> &[AgentEvent] {
        &self.events
    }
}

impl EventHandler for RecordingEventHandler {
    fn on_event(&mut self, event: AgentEvent) {
        self.events.push(event);
    }
}
