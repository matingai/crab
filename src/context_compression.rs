use anyhow::Result;
use std::env;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

use crate::context_limit_cache::{load_context_length, save_context_length};
use crate::context_probe::probe_context_length;
use crate::llm::OpenAiCompatClient;
use crate::session::StoredSession;
use crate::types::{ChatMessage, ToolCall};

const SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION] Earlier turns in this conversation were compacted to save context space. The summary below describes work that was already completed. Files may already be changed, commands may already have run, and prior errors may already be resolved. Continue from the current workspace state and avoid repeating completed work.";
const SUMMARY_PROMPT: &str = "You are compressing an ongoing coding-agent conversation. Summarize the earlier turns so another coding agent can continue the task without losing important context. Focus on: the user's goal, decisions already made, files changed or inspected, commands and outputs that mattered, approvals or blockers, work already completed, and the best next steps. Be concrete. Use short sections titled Goal, Progress, Key Details, Open Items. Do not invent facts. Keep the summary compact but specific.";
const DEFAULT_THRESHOLD_TOKENS: usize = 24_000;
const DEFAULT_TARGET_TAIL_TOKENS: usize = 6_000;
const DEFAULT_PRESERVE_FIRST_MESSAGES: usize = 3;
const DEFAULT_PRESERVE_RECENT_MESSAGES: usize = 10;
const DEFAULT_MAX_SUMMARY_INPUT_CHARS: usize = 32_000;
const DEFAULT_MAX_SUMMARY_OUTPUT_CHARS: usize = 25_600;
const DEFAULT_SUMMARY_FAILURE_COOLDOWN_SECONDS: u64 = 600;
const DEFAULT_CONTEXT_LENGTH: usize = 128_000;
const MIN_THRESHOLD_TOKENS: usize = 24_000;
const MAX_THRESHOLD_TOKENS: usize = 180_000;
const MIN_TARGET_TAIL_TOKENS: usize = 6_000;
const MAX_TARGET_TAIL_TOKENS: usize = 40_000;
const MIN_SUMMARY_INPUT_CHARS: usize = 32_000;
const MAX_SUMMARY_INPUT_CHARS: usize = 120_000;
const MIN_SUMMARY_OUTPUT_CHARS: usize = 8_000;
const MAX_SUMMARY_OUTPUT_CHARS: usize = 48_000;
const PRUNED_TOOL_PLACEHOLDER: &str = "[Old tool output cleared to save context space]";
const TOOL_OUTPUT_PRUNE_MIN_CHARS: usize = 240;
const SUMMARY_STRUCTURE: &str = "Use this exact structure:\n\n## Goal\n[What the user is trying to accomplish]\n\n## Constraints & Preferences\n[User preferences, coding style, constraints, important decisions]\n\n## Progress\n### Done\n[Completed work — include specific file paths, commands run, results obtained]\n### In Progress\n[Work currently underway]\n### Blocked\n[Any blockers or issues encountered]\n\n## Key Decisions\n[Important technical decisions and why they were made]\n\n## Relevant Files\n[Files read, modified, or created — with brief note on each]\n\n## Next Steps\n[What needs to happen next to continue the work]\n\n## Critical Context\n[Any specific values, error messages, configuration details, or data that would be lost without explicit preservation]\n\n## Tools & Patterns\n[Which tools were used, how they were used effectively, and any tool-specific discoveries]";

#[derive(Debug, Clone)]
pub struct ContextCompressionConfig {
    pub enabled: bool,
    pub context_length: usize,
    pub threshold_tokens: usize,
    pub target_tail_tokens: usize,
    pub preserve_first_messages: usize,
    pub preserve_recent_messages: usize,
    pub max_summary_input_chars: usize,
    pub max_summary_output_chars: usize,
}

impl Default for ContextCompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            context_length: DEFAULT_CONTEXT_LENGTH,
            threshold_tokens: DEFAULT_THRESHOLD_TOKENS,
            target_tail_tokens: DEFAULT_TARGET_TAIL_TOKENS,
            preserve_first_messages: DEFAULT_PRESERVE_FIRST_MESSAGES,
            preserve_recent_messages: DEFAULT_PRESERVE_RECENT_MESSAGES,
            max_summary_input_chars: DEFAULT_MAX_SUMMARY_INPUT_CHARS,
            max_summary_output_chars: DEFAULT_MAX_SUMMARY_OUTPUT_CHARS,
        }
    }
}

impl ContextCompressionConfig {
    pub fn from_env() -> Self {
        Self::for_runtime("", "", "")
    }

    pub fn for_runtime_with_data_dir(
        data_dir: &Path,
        model: &str,
        provider_kind: &str,
        base_url: &str,
    ) -> Self {
        let cached = load_context_length(data_dir, model, base_url)
            .ok()
            .flatten();
        let probed = if cached.is_none()
            && env::var("HERMES_RS_CONTEXT_LENGTH").is_err()
            && should_run_sync_context_probe()
        {
            probe_context_length(model, base_url).inspect(|context_length| {
                let _ = save_context_length(data_dir, model, base_url, *context_length);
            })
        } else {
            None
        };
        Self::for_runtime_with_context_length(model, provider_kind, base_url, cached.or(probed))
    }

    pub fn for_runtime(model: &str, provider_kind: &str, base_url: &str) -> Self {
        Self::for_runtime_with_context_length(model, provider_kind, base_url, None)
    }

    fn for_runtime_with_context_length(
        model: &str,
        provider_kind: &str,
        base_url: &str,
        cached_context_length: Option<usize>,
    ) -> Self {
        let default = Self::default();
        let context_length = env_usize(
            "HERMES_RS_CONTEXT_LENGTH",
            cached_context_length
                .unwrap_or_else(|| infer_context_length(model, provider_kind, base_url)),
        );
        let threshold_tokens = clamp_usize(
            context_length / 2,
            MIN_THRESHOLD_TOKENS,
            MAX_THRESHOLD_TOKENS,
        );
        let target_tail_tokens = clamp_usize(
            context_length / 8,
            MIN_TARGET_TAIL_TOKENS,
            MAX_TARGET_TAIL_TOKENS,
        );
        let max_summary_input_chars = clamp_usize(
            context_length,
            MIN_SUMMARY_INPUT_CHARS,
            MAX_SUMMARY_INPUT_CHARS,
        ) * 4
            / 10;
        let max_summary_output_chars = clamp_usize(
            context_length / 5,
            MIN_SUMMARY_OUTPUT_CHARS,
            MAX_SUMMARY_OUTPUT_CHARS,
        );
        Self {
            enabled: env_flag("HERMES_RS_CONTEXT_COMPRESSION", true),
            context_length,
            threshold_tokens: env_usize(
                "HERMES_RS_CONTEXT_COMPRESSION_THRESHOLD_TOKENS",
                threshold_tokens.max(default.threshold_tokens),
            ),
            target_tail_tokens: env_usize(
                "HERMES_RS_CONTEXT_COMPRESSION_TARGET_TAIL_TOKENS",
                target_tail_tokens.max(default.target_tail_tokens),
            ),
            preserve_first_messages: env_usize(
                "HERMES_RS_CONTEXT_COMPRESSION_PRESERVE_FIRST_MESSAGES",
                default.preserve_first_messages,
            ),
            preserve_recent_messages: env_usize(
                "HERMES_RS_CONTEXT_COMPRESSION_PRESERVE_RECENT_MESSAGES",
                default.preserve_recent_messages,
            ),
            max_summary_input_chars: env_usize(
                "HERMES_RS_CONTEXT_COMPRESSION_MAX_SUMMARY_INPUT_CHARS",
                max_summary_input_chars.max(default.max_summary_input_chars),
            ),
            max_summary_output_chars: env_usize(
                "HERMES_RS_CONTEXT_COMPRESSION_MAX_SUMMARY_OUTPUT_CHARS",
                max_summary_output_chars.max(default.max_summary_output_chars),
            ),
        }
    }
}

fn should_run_sync_context_probe() -> bool {
    Handle::try_current().is_err()
}

#[derive(Debug, Clone)]
pub struct ContextCompressionOutcome {
    pub original_message_count: usize,
    pub compressed_message_count: usize,
    pub original_estimated_tokens: usize,
    pub compressed_estimated_tokens: usize,
    pub pruned_tool_messages: usize,
    pub used_summary: bool,
}

pub struct ContextCompressor {
    config: ContextCompressionConfig,
    summary_failure_cooldown_until: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct CompressionPlan {
    compress_start: usize,
    compress_end: usize,
}

impl ContextCompressor {
    pub fn new(config: ContextCompressionConfig) -> Self {
        Self {
            config,
            summary_failure_cooldown_until: None,
        }
    }

    pub fn from_env() -> Self {
        Self::new(ContextCompressionConfig::from_env())
    }

    pub fn for_runtime(model: &str, provider_kind: &str, base_url: &str) -> Self {
        Self::new(ContextCompressionConfig::for_runtime(
            model,
            provider_kind,
            base_url,
        ))
    }

    pub fn for_runtime_with_data_dir(
        data_dir: &Path,
        model: &str,
        provider_kind: &str,
        base_url: &str,
    ) -> Self {
        Self::new(ContextCompressionConfig::for_runtime_with_data_dir(
            data_dir,
            model,
            provider_kind,
            base_url,
        ))
    }

    pub fn context_length(&self) -> usize {
        self.config.context_length
    }

    pub fn apply_context_limit(&mut self, context_length: usize) {
        if context_length == 0 || context_length >= self.config.context_length {
            return;
        }

        self.config.context_length = context_length;
        self.config.threshold_tokens = self.config.threshold_tokens.min(clamp_usize(
            context_length / 2,
            MIN_THRESHOLD_TOKENS,
            MAX_THRESHOLD_TOKENS,
        ));
        self.config.target_tail_tokens = self.config.target_tail_tokens.min(clamp_usize(
            context_length / 8,
            MIN_TARGET_TAIL_TOKENS,
            MAX_TARGET_TAIL_TOKENS,
        ));
        self.config.max_summary_input_chars = self.config.max_summary_input_chars.min(
            clamp_usize(
                context_length,
                MIN_SUMMARY_INPUT_CHARS,
                MAX_SUMMARY_INPUT_CHARS,
            ) * 4
                / 10,
        );
        self.config.max_summary_output_chars =
            self.config.max_summary_output_chars.min(clamp_usize(
                context_length / 5,
                MIN_SUMMARY_OUTPUT_CHARS,
                MAX_SUMMARY_OUTPUT_CHARS,
            ));
    }

    pub async fn maybe_compress_session(
        &mut self,
        session: &mut StoredSession,
        client: &OpenAiCompatClient,
        model: &str,
    ) -> Result<Option<ContextCompressionOutcome>> {
        self.compress_session(session, client, model, false).await
    }

    pub async fn force_compress_session(
        &mut self,
        session: &mut StoredSession,
        client: &OpenAiCompatClient,
        model: &str,
    ) -> Result<Option<ContextCompressionOutcome>> {
        self.compress_session(session, client, model, true).await
    }

    async fn compress_session(
        &mut self,
        session: &mut StoredSession,
        client: &OpenAiCompatClient,
        model: &str,
        force: bool,
    ) -> Result<Option<ContextCompressionOutcome>> {
        if !self.config.enabled {
            return Ok(None);
        }

        let original_message_count = session.history.len();
        let original_estimated_tokens = estimate_messages_tokens(&session.history);
        if !force && original_estimated_tokens < self.config.threshold_tokens {
            return Ok(None);
        }

        let tail_start = protected_tail_start(
            &session.history,
            self.config.preserve_recent_messages,
            self.config.target_tail_tokens,
        );
        let pruned_tool_messages = prune_old_tool_results(&mut session.history, tail_start);
        let after_prune_estimated_tokens = estimate_messages_tokens(&session.history);
        if !force && after_prune_estimated_tokens < self.config.threshold_tokens {
            return Ok(Some(ContextCompressionOutcome {
                original_message_count,
                compressed_message_count: session.history.len(),
                original_estimated_tokens,
                compressed_estimated_tokens: after_prune_estimated_tokens,
                pruned_tool_messages,
                used_summary: false,
            }));
        }

        let Some(plan) = (if force {
            self.plan(&session.history)
                .or_else(|| self.forced_plan(&session.history))
        } else {
            self.plan(&session.history)
        }) else {
            return if pruned_tool_messages > 0 {
                Ok(Some(ContextCompressionOutcome {
                    original_message_count,
                    compressed_message_count: session.history.len(),
                    original_estimated_tokens,
                    compressed_estimated_tokens: after_prune_estimated_tokens,
                    pruned_tool_messages,
                    used_summary: false,
                }))
            } else {
                Ok(None)
            };
        };

        let turns_to_compress = &session.history[plan.compress_start..plan.compress_end];
        let summary = if self
            .summary_failure_cooldown_until
            .is_some_and(|until| Instant::now() < until)
        {
            build_static_fallback_summary(
                turns_to_compress,
                self.config.max_summary_output_chars,
                pruned_tool_messages,
            )
        } else {
            match self
                .summarize_messages(turns_to_compress, client, model)
                .await
            {
                Ok(summary) if !summary.trim().is_empty() => {
                    self.summary_failure_cooldown_until = None;
                    normalize_summary(&summary, self.config.max_summary_output_chars)
                }
                Ok(_) | Err(_) => {
                    self.summary_failure_cooldown_until = Some(
                        Instant::now()
                            + Duration::from_secs(DEFAULT_SUMMARY_FAILURE_COOLDOWN_SECONDS),
                    );
                    build_static_fallback_summary(
                        turns_to_compress,
                        self.config.max_summary_output_chars,
                        pruned_tool_messages,
                    )
                }
            }
        };

        let mut compressed_history = Vec::with_capacity(
            session.history.len() - (plan.compress_end - plan.compress_start) + 1,
        );
        compressed_history.extend_from_slice(&session.history[..plan.compress_start]);
        compressed_history.push(ChatMessage::system(format!(
            "{SUMMARY_PREFIX}\n\n{}",
            summary.trim()
        )));
        compressed_history.extend_from_slice(&session.history[plan.compress_end..]);
        compressed_history = sanitize_tool_pairs(compressed_history);

        let compressed_estimated_tokens = estimate_messages_tokens(&compressed_history);
        if compressed_estimated_tokens >= original_estimated_tokens {
            return Ok(None);
        }

        session.history = compressed_history;
        Ok(Some(ContextCompressionOutcome {
            original_message_count,
            compressed_message_count: session.history.len(),
            original_estimated_tokens,
            compressed_estimated_tokens,
            pruned_tool_messages,
            used_summary: true,
        }))
    }

    fn plan(&self, history: &[ChatMessage]) -> Option<CompressionPlan> {
        if history.len()
            <= self
                .config
                .preserve_first_messages
                .saturating_add(self.config.preserve_recent_messages)
        {
            return None;
        }

        let mut compress_start = self.config.preserve_first_messages.min(history.len());
        let mut compress_end = protected_tail_start(
            history,
            self.config.preserve_recent_messages,
            self.config.target_tail_tokens,
        )
        .max(compress_start);

        compress_start = align_boundary_forward(history, compress_start);
        compress_end = align_boundary_backward(history, compress_end).max(compress_start);

        if compress_end <= compress_start {
            return None;
        }

        let compressed_tokens = estimate_messages_tokens(&history[compress_start..compress_end]);
        if compressed_tokens < self.config.target_tail_tokens / 2 {
            return None;
        }

        Some(CompressionPlan {
            compress_start,
            compress_end,
        })
    }

    fn forced_plan(&self, history: &[ChatMessage]) -> Option<CompressionPlan> {
        if history.len() <= self.config.preserve_first_messages.saturating_add(4) {
            return None;
        }

        let mut compress_start = self.config.preserve_first_messages.min(history.len());
        compress_start = align_boundary_forward(history, compress_start);

        let protected_tail_messages = self.config.preserve_recent_messages.max(3);
        let mut compress_end = history
            .len()
            .saturating_sub(protected_tail_messages)
            .max(compress_start + 1);
        compress_end = align_boundary_backward(history, compress_end).max(compress_start);

        if compress_end <= compress_start {
            return None;
        }

        Some(CompressionPlan {
            compress_start,
            compress_end,
        })
    }

    async fn summarize_messages(
        &self,
        messages: &[ChatMessage],
        client: &OpenAiCompatClient,
        model: &str,
    ) -> Result<String> {
        let (previous_summary, effective_messages) = extract_previous_summary(messages);
        let serialized = cap_chars(
            serialize_messages(&effective_messages),
            self.config.max_summary_input_chars,
        );
        let user_prompt = if let Some(previous_summary) = previous_summary {
            format!(
                "You are updating an existing context-compaction summary.\n\nPREVIOUS SUMMARY:\n{previous_summary}\n\nNEW TURNS TO INCORPORATE:\n{serialized}\n\nPreserve still-relevant details from the previous summary, add new progress, and remove only clearly obsolete details.\n\n{SUMMARY_STRUCTURE}\n\nBe concrete. Include file paths, commands, errors, approvals, and next steps. Write only the summary body."
            )
        } else {
            format!(
                "Summarize these earlier conversation turns for future continuation:\n\n{serialized}\n\n{SUMMARY_STRUCTURE}\n\nBe concrete. Include file paths, commands, errors, approvals, and next steps. Write only the summary body."
            )
        };
        let prompt_messages = vec![
            ChatMessage::system(SUMMARY_PROMPT),
            ChatMessage::user(user_prompt),
        ];
        let response = client.respond(model, &prompt_messages, &[]).await?;
        Ok(normalize_summary(
            &response.message.content_text(),
            self.config.max_summary_output_chars,
        ))
    }
}

pub fn estimate_messages_tokens(messages: &[ChatMessage]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

fn estimate_message_tokens(message: &ChatMessage) -> usize {
    let mut chars = message.content_text().chars().count();
    if let Some(tool_calls) = &message.tool_calls {
        for tool_call in tool_calls {
            chars += tool_call.function.name.chars().count();
            chars += tool_call.function.arguments.chars().count();
        }
    }
    chars / 4 + 12
}

fn protected_tail_start(
    history: &[ChatMessage],
    preserve_recent_messages: usize,
    target_tail_tokens: usize,
) -> usize {
    if history.is_empty() {
        return 0;
    }

    let mut accumulated_tokens = 0usize;
    let mut protected_messages = 0usize;

    for idx in (0..history.len()).rev() {
        accumulated_tokens += estimate_message_tokens(&history[idx]);
        protected_messages += 1;
        if protected_messages >= preserve_recent_messages
            && accumulated_tokens >= target_tail_tokens
        {
            return idx;
        }
    }

    0
}

fn serialize_messages(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(serialize_message)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn serialize_message(message: &ChatMessage) -> String {
    let role = message.role.to_uppercase();
    let mut body = if message.role == "tool" {
        serialize_tool_content(&message.content_text())
    } else {
        truncate_middle(&message.content_text(), 1_200, 800, 240)
    };

    if let Some(tool_calls) = &message.tool_calls {
        let calls = tool_calls
            .iter()
            .map(|call| {
                format!(
                    "{}({})",
                    call.function.name,
                    truncate_middle(&call.function.arguments, 400, 320, 48)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !calls.is_empty() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str("[tool_calls]\n");
            body.push_str(&calls);
        }
    }

    if let Some(tool_call_id) = &message.tool_call_id {
        if !tool_call_id.trim().is_empty() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&format!("[tool_call_id] {tool_call_id}"));
        }
    }

    format!("[{role}]\n{}", body.trim())
}

fn serialize_tool_content(value: &str) -> String {
    if value == PRUNED_TOOL_PLACEHOLDER {
        return value.to_string();
    }
    truncate_middle(value, 900, 520, 160)
}

fn truncate_middle(value: &str, max_chars: usize, head_chars: usize, tail_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return value.to_string();
    }

    let head = chars.iter().take(head_chars).collect::<String>();
    let tail = chars
        .iter()
        .rev()
        .take(tail_chars)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{head}\n...[truncated]...\n{tail}")
}

fn cap_chars(value: String, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return value;
    }

    let head_chars = max_chars.saturating_mul(2) / 3;
    let tail_chars = max_chars.saturating_sub(head_chars).saturating_sub(32);
    let head = chars.iter().take(head_chars).collect::<String>();
    let tail = chars
        .iter()
        .rev()
        .take(tail_chars)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{head}\n...[truncated earlier turns]...\n{tail}")
}

fn normalize_summary(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    let body = trimmed
        .strip_prefix(SUMMARY_PREFIX)
        .unwrap_or(trimmed)
        .trim();
    cap_chars(body.to_string(), max_chars)
}

fn build_static_fallback_summary(
    messages: &[ChatMessage],
    max_chars: usize,
    pruned_tool_messages: usize,
) -> String {
    let (previous_summary, effective_messages) = extract_previous_summary(messages);
    let removed_messages = effective_messages.len();
    let removed_tokens = estimate_messages_tokens(&effective_messages);
    let prune_note = if pruned_tool_messages > 0 {
        format!(
            " Old tool outputs had already been pruned {} time(s) before fallback compaction.",
            pruned_tool_messages
        )
    } else {
        String::new()
    };

    let summary = if let Some(previous_summary) = previous_summary {
        format!(
            "{previous_summary}\n\n## Critical Context\nThe existing summary above was preserved. {removed_messages} additional message(s) from older conversation history (~{removed_tokens} tokens) were compacted with a static fallback because a fresh structured summary was unavailable.{prune_note}\n\n## Next Steps\nReconstruct the most recent detailed status from the current workspace state, the surviving tail of the conversation, and any visible file/tool traces before repeating work."
        )
    } else {
        format!(
            "## Goal\nContinue the existing task from the current workspace state.\n\n## Constraints & Preferences\nDo not repeat completed work; assume files may already be changed and commands may already have run.\n\n## Progress\n### Done\nOlder conversation history was compacted to free context space.\n### In Progress\nResume from the surviving recent turns and current workspace state.\n### Blocked\nA fresh structured summary for the removed turns was unavailable.\n\n## Key Decisions\nPrefer current workspace state over assumptions from missing history.\n\n## Relevant Files\nInspect the files changed in the workspace to recover exact status.\n\n## Next Steps\nCheck the surviving recent turns, current diffs, and relevant files before continuing.\n\n## Critical Context\nRemoved {removed_messages} older message(s), roughly {removed_tokens} tokens, using a static fallback summary.{prune_note}\n\n## Tools & Patterns\nRely on the surviving tail context and workspace inspection to recover specifics."
        )
    };

    normalize_summary(&summary, max_chars)
}

fn prune_old_tool_results(history: &mut [ChatMessage], tail_start: usize) -> usize {
    let mut pruned = 0usize;
    for message in history.iter_mut().take(tail_start) {
        if message.role != "tool" {
            continue;
        }
        let content = message.content_text();
        if content == PRUNED_TOOL_PLACEHOLDER
            || content.chars().count() <= TOOL_OUTPUT_PRUNE_MIN_CHARS
        {
            continue;
        }
        message.content = Some(serde_json::Value::String(
            PRUNED_TOOL_PLACEHOLDER.to_string(),
        ));
        pruned += 1;
    }
    pruned
}

fn extract_previous_summary(messages: &[ChatMessage]) -> (Option<String>, Vec<ChatMessage>) {
    let mut previous_summary = None;
    let mut filtered = Vec::with_capacity(messages.len());
    for message in messages {
        if is_summary_message(message) {
            previous_summary = summary_body(message);
            continue;
        }
        filtered.push(message.clone());
    }
    (previous_summary, filtered)
}

fn align_boundary_forward(history: &[ChatMessage], mut idx: usize) -> usize {
    while idx < history.len() && history[idx].role == "tool" {
        idx += 1;
    }
    idx
}

fn align_boundary_backward(history: &[ChatMessage], idx: usize) -> usize {
    if idx == 0 || idx >= history.len() {
        return idx;
    }

    let mut check = idx;
    while check > 0 && history[check - 1].role == "tool" {
        check -= 1;
    }

    if check > 0 {
        let assistant = &history[check - 1];
        if assistant.role == "assistant" && assistant.tool_calls.is_some() {
            return check - 1;
        }
    }

    idx
}

fn sanitize_tool_pairs(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let surviving_call_ids = messages
        .iter()
        .filter(|message| message.role == "assistant")
        .flat_map(|message| message.tool_calls.as_ref().into_iter().flatten())
        .map(tool_call_id)
        .filter(|value| !value.is_empty())
        .collect::<std::collections::BTreeSet<_>>();

    let mut result_call_ids = messages
        .iter()
        .filter(|message| message.role == "tool")
        .filter_map(|message| message.tool_call_id.clone())
        .filter(|value| !value.trim().is_empty())
        .collect::<std::collections::BTreeSet<_>>();

    let mut sanitized = messages
        .into_iter()
        .filter(|message| {
            if message.role != "tool" {
                return true;
            }
            message
                .tool_call_id
                .as_ref()
                .is_some_and(|value| surviving_call_ids.contains(value))
        })
        .collect::<Vec<_>>();

    let missing_results = surviving_call_ids
        .iter()
        .filter(|call_id| !result_call_ids.contains(*call_id))
        .cloned()
        .collect::<Vec<_>>();

    if missing_results.is_empty() {
        return sanitized;
    }

    let mut repaired = Vec::with_capacity(sanitized.len() + missing_results.len());
    for message in sanitized.drain(..) {
        let current_tool_calls = message.tool_calls.clone();
        repaired.push(message);
        if let Some(tool_calls) = current_tool_calls {
            for tool_call in tool_calls {
                let call_id = tool_call_id(&tool_call);
                if call_id.is_empty() || result_call_ids.contains(&call_id) {
                    continue;
                }
                repaired.push(ChatMessage::tool(
                    call_id.clone(),
                    "[Result from earlier conversation — see context summary above]",
                ));
                result_call_ids.insert(call_id);
            }
        }
    }

    repaired
}

fn tool_call_id(tool_call: &ToolCall) -> String {
    tool_call.id.trim().to_string()
}

fn is_summary_message(message: &ChatMessage) -> bool {
    message.role == "system" && message.content_text().starts_with(SUMMARY_PREFIX)
}

fn summary_body(message: &ChatMessage) -> Option<String> {
    let content = message.content_text();
    let body = content
        .strip_prefix(SUMMARY_PREFIX)
        .unwrap_or(content.as_str())
        .trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

fn env_flag(name: &str, default_value: bool) -> bool {
    match env::var(name) {
        Ok(value) => matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "on"),
        Err(_) => default_value,
    }
}

fn env_usize(name: &str, default_value: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

fn clamp_usize(value: usize, min: usize, max: usize) -> usize {
    value.max(min).min(max)
}

fn infer_context_length(model: &str, provider_kind: &str, base_url: &str) -> usize {
    let model = model.trim().to_lowercase();
    let provider_kind = provider_kind.trim().to_lowercase();
    let base_url = base_url.trim().to_lowercase();

    if model.contains("gpt-4.1") {
        return 1_000_000;
    }
    if model.contains("gpt-5")
        || model.contains("codex")
        || model.starts_with("o3")
        || model.starts_with("o4")
        || provider_kind == "openai-codex"
        || base_url.contains("/backend-api/codex")
    {
        return 200_000;
    }
    if model.contains("claude")
        || model.contains("gemini")
        || model.contains("qwen")
        || model.contains("deepseek")
        || model.contains("kimi")
        || model.contains("glm")
        || model.contains("minimax")
    {
        return 128_000;
    }
    if base_url.contains("localhost")
        || base_url.contains("127.0.0.1")
        || base_url.contains("0.0.0.0")
        || base_url.contains("[::1]")
    {
        return 64_000;
    }
    DEFAULT_CONTEXT_LENGTH
}

#[cfg(test)]
mod tests {
    use super::{
        ContextCompressionConfig, ContextCompressor, PRUNED_TOOL_PLACEHOLDER, SUMMARY_PREFIX,
        align_boundary_backward, estimate_messages_tokens, infer_context_length,
        sanitize_tool_pairs,
    };
    use crate::llm::{ApiMode, OpenAiCompatClient};
    use crate::session::StoredSession;
    use crate::types::ChatMessage;
    use crate::types::ToolCall;
    use crate::types::ToolFunctionCall;

    #[tokio::test]
    async fn compresses_long_history_into_summary_message() {
        let mut compressor = ContextCompressor::new(ContextCompressionConfig {
            enabled: true,
            context_length: 8_000,
            threshold_tokens: 200,
            target_tail_tokens: 80,
            preserve_first_messages: 2,
            preserve_recent_messages: 3,
            max_summary_input_chars: 8_000,
            max_summary_output_chars: 8_000,
        });
        let client =
            OpenAiCompatClient::new("mock://final-response", None, ApiMode::ChatCompletions)
                .expect("client");
        let mut session = StoredSession::new("demo".to_string(), "gpt-4.1-mini".to_string());
        for idx in 0..18 {
            session.history.push(ChatMessage::user(format!(
                "User message {idx}: {}",
                "A".repeat(180)
            )));
            session.history.push(ChatMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String(format!(
                    "Assistant message {idx}: {}",
                    "B".repeat(220)
                ))),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        let before_tokens = estimate_messages_tokens(&session.history);
        let outcome = compressor
            .maybe_compress_session(&mut session, &client, "gpt-4.1-mini")
            .await
            .expect("compression")
            .expect("compressed");

        assert!(outcome.original_message_count > outcome.compressed_message_count);
        assert!(outcome.original_estimated_tokens > outcome.compressed_estimated_tokens);
        assert!(estimate_messages_tokens(&session.history) < before_tokens);
        assert!(session.history.iter().any(|message| {
            message.role == "system" && message.content_text().starts_with(SUMMARY_PREFIX)
        }));
        assert!(outcome.used_summary);
    }

    #[tokio::test]
    async fn prunes_old_tool_outputs_before_summary_when_possible() {
        let mut compressor = ContextCompressor::new(ContextCompressionConfig {
            enabled: true,
            context_length: 8_000,
            threshold_tokens: 170,
            target_tail_tokens: 60,
            preserve_first_messages: 1,
            preserve_recent_messages: 4,
            max_summary_input_chars: 8_000,
            max_summary_output_chars: 8_000,
        });
        let client =
            OpenAiCompatClient::new("mock://final-response", None, ApiMode::ChatCompletions)
                .expect("client");
        let mut session = StoredSession::new("demo".to_string(), "gpt-4.1-mini".to_string());
        session
            .history
            .push(ChatMessage::user("Inspect the existing implementation"));
        session.history.push(ChatMessage {
            role: "assistant".to_string(),
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call-read".to_string(),
                kind: "function".to_string(),
                function: ToolFunctionCall {
                    name: "read_file".to_string(),
                    arguments: "{\"path\":\"src/lib.rs\"}".to_string(),
                },
            }]),
            tool_call_id: None,
        });
        session
            .history
            .push(ChatMessage::tool("call-read", "X".repeat(1200)));
        session.history.push(ChatMessage::user(format!(
            "Recent short request {}",
            "Y".repeat(40)
        )));
        session.history.push(ChatMessage {
            role: "assistant".to_string(),
            content: Some(serde_json::Value::String("Recent answer".to_string())),
            tool_calls: None,
            tool_call_id: None,
        });
        session.history.push(ChatMessage::user(format!(
            "Tail message {}",
            "Z".repeat(40)
        )));
        session.history.push(ChatMessage {
            role: "assistant".to_string(),
            content: Some(serde_json::Value::String("Tail answer".to_string())),
            tool_calls: None,
            tool_call_id: None,
        });

        let outcome = compressor
            .maybe_compress_session(&mut session, &client, "gpt-4.1-mini")
            .await
            .expect("compression")
            .expect("changed");

        assert!(outcome.pruned_tool_messages > 0);
        assert!(!outcome.used_summary);
        assert!(
            session.history.iter().any(|message| message.role == "tool"
                && message.content_text() == PRUNED_TOOL_PLACEHOLDER)
        );
    }

    #[tokio::test]
    async fn updates_existing_summary_instead_of_nesting_summaries() {
        let mut compressor = ContextCompressor::new(ContextCompressionConfig {
            enabled: true,
            context_length: 8_000,
            threshold_tokens: 200,
            target_tail_tokens: 80,
            preserve_first_messages: 2,
            preserve_recent_messages: 3,
            max_summary_input_chars: 8_000,
            max_summary_output_chars: 8_000,
        });
        let client =
            OpenAiCompatClient::new("mock://final-response", None, ApiMode::ChatCompletions)
                .expect("client");
        let mut session = StoredSession::new("demo".to_string(), "gpt-4.1-mini".to_string());
        session.history.push(ChatMessage::user(format!(
            "Initial request {}",
            "A".repeat(100)
        )));
        session.history.push(ChatMessage {
            role: "assistant".to_string(),
            content: Some(serde_json::Value::String(format!(
                "Initial answer {}",
                "B".repeat(120)
            ))),
            tool_calls: None,
            tool_call_id: None,
        });
        session.history.push(ChatMessage::system(format!(
            "{SUMMARY_PREFIX}\n\n## Goal\nPrevious summary\n\n## Constraints & Preferences\nNone"
        )));
        for idx in 0..8 {
            session.history.push(ChatMessage::user(format!(
                "Later user message {idx}: {}",
                "C".repeat(120)
            )));
            session.history.push(ChatMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String(format!(
                    "Later assistant message {idx}: {}",
                    "D".repeat(140)
                ))),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        let outcome = compressor
            .maybe_compress_session(&mut session, &client, "gpt-4.1-mini")
            .await
            .expect("compression")
            .expect("compressed");

        let summaries = session
            .history
            .iter()
            .filter(|message| {
                message.role == "system" && message.content_text().starts_with(SUMMARY_PREFIX)
            })
            .collect::<Vec<_>>();
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].content_text().contains("Updated summary"));
        assert!(outcome.used_summary);
    }

    #[tokio::test]
    async fn uses_static_fallback_summary_when_summary_generation_fails() {
        let mut compressor = ContextCompressor::new(ContextCompressionConfig {
            enabled: true,
            context_length: 8_000,
            threshold_tokens: 180,
            target_tail_tokens: 80,
            preserve_first_messages: 2,
            preserve_recent_messages: 3,
            max_summary_input_chars: 8_000,
            max_summary_output_chars: 2_000,
        });
        let client =
            OpenAiCompatClient::new("mock://compression-error", None, ApiMode::ChatCompletions)
                .expect("client");
        let mut session = StoredSession::new("demo".to_string(), "gpt-4.1-mini".to_string());
        for idx in 0..14 {
            session.history.push(ChatMessage::user(format!(
                "Fallback user message {idx}: {}",
                "A".repeat(140)
            )));
            session.history.push(ChatMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String(format!(
                    "Fallback assistant message {idx}: {}",
                    "B".repeat(160)
                ))),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        let before_tokens = estimate_messages_tokens(&session.history);
        let outcome = compressor
            .maybe_compress_session(&mut session, &client, "gpt-4.1-mini")
            .await
            .expect("compression")
            .expect("compressed");

        let summary = session
            .history
            .iter()
            .find(|message| {
                message.role == "system" && message.content_text().starts_with(SUMMARY_PREFIX)
            })
            .expect("summary message")
            .content_text();
        assert!(
            summary.contains("A fresh structured summary for the removed turns was unavailable")
        );
        assert!(estimate_messages_tokens(&session.history) < before_tokens);
        assert!(outcome.used_summary);
    }

    #[tokio::test]
    async fn trims_overlong_summary_output() {
        let mut compressor = ContextCompressor::new(ContextCompressionConfig {
            enabled: true,
            context_length: 8_000,
            threshold_tokens: 180,
            target_tail_tokens: 80,
            preserve_first_messages: 2,
            preserve_recent_messages: 3,
            max_summary_input_chars: 8_000,
            max_summary_output_chars: 1_200,
        });
        let client = OpenAiCompatClient::new(
            "mock://compression-long-summary",
            None,
            ApiMode::ChatCompletions,
        )
        .expect("client");
        let mut session = StoredSession::new("demo".to_string(), "gpt-4.1-mini".to_string());
        for idx in 0..14 {
            session.history.push(ChatMessage::user(format!(
                "Long summary user message {idx}: {}",
                "A".repeat(140)
            )));
            session.history.push(ChatMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String(format!(
                    "Long summary assistant message {idx}: {}",
                    "B".repeat(160)
                ))),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        let outcome = compressor
            .maybe_compress_session(&mut session, &client, "gpt-4.1-mini")
            .await
            .expect("compression")
            .expect("compressed");

        let summary = session
            .history
            .iter()
            .find(|message| {
                message.role == "system" && message.content_text().starts_with(SUMMARY_PREFIX)
            })
            .expect("summary message")
            .content_text();
        assert!(summary.contains("...[truncated earlier turns]..."));
        assert!(summary.chars().count() <= SUMMARY_PREFIX.chars().count() + 1_400);
        assert!(outcome.used_summary);
    }

    #[tokio::test]
    async fn force_compression_can_compact_even_with_large_context_budget() {
        let mut compressor = ContextCompressor::new(ContextCompressionConfig {
            enabled: true,
            context_length: 1_000_000,
            threshold_tokens: 500_000,
            target_tail_tokens: 40_000,
            preserve_first_messages: 3,
            preserve_recent_messages: 10,
            max_summary_input_chars: 32_000,
            max_summary_output_chars: 8_000,
        });
        let client =
            OpenAiCompatClient::new("mock://compression-error", None, ApiMode::ChatCompletions)
                .expect("client");
        let mut session = StoredSession::new("demo".to_string(), "gpt-4.1".to_string());
        for idx in 0..14 {
            session.history.push(ChatMessage::user(format!(
                "Force user message {idx}: {}",
                "A".repeat(140)
            )));
            session.history.push(ChatMessage {
                role: "assistant".to_string(),
                content: Some(serde_json::Value::String(format!(
                    "Force assistant message {idx}: {}",
                    "B".repeat(160)
                ))),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        let outcome = compressor
            .force_compress_session(&mut session, &client, "gpt-4.1")
            .await
            .expect("compression")
            .expect("compressed");

        assert!(outcome.used_summary);
        assert!(session.history.iter().any(|message| {
            message.role == "system" && message.content_text().starts_with(SUMMARY_PREFIX)
        }));
    }

    #[test]
    fn boundary_alignment_moves_before_assistant_tool_group() {
        let history = vec![
            ChatMessage::user("head"),
            ChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call-a".to_string(),
                    kind: "function".to_string(),
                    function: ToolFunctionCall {
                        name: "read_file".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                tool_call_id: None,
            },
            ChatMessage::tool("call-a", "tool output"),
            ChatMessage::user("tail"),
        ];

        assert_eq!(align_boundary_backward(&history, 3), 1);
    }

    #[test]
    fn sanitize_tool_pairs_removes_orphans_and_adds_missing_results() {
        let sanitized = sanitize_tool_pairs(vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call-a".to_string(),
                    kind: "function".to_string(),
                    function: ToolFunctionCall {
                        name: "read_file".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                tool_call_id: None,
            },
            ChatMessage::tool("call-a", "ok"),
            ChatMessage::tool("orphan", "should disappear"),
            ChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call-b".to_string(),
                    kind: "function".to_string(),
                    function: ToolFunctionCall {
                        name: "search_files".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                tool_call_id: None,
            },
        ]);

        assert_eq!(
            sanitized
                .iter()
                .filter(|message| message.role == "tool"
                    && message.tool_call_id.as_deref() == Some("orphan"))
                .count(),
            0
        );
        assert!(sanitized.iter().any(|message| {
            message.role == "tool"
                && message.tool_call_id.as_deref() == Some("call-b")
                && message
                    .content_text()
                    .contains("Result from earlier conversation")
        }));
    }

    #[test]
    fn infers_large_context_for_gpt_41_and_codex() {
        assert_eq!(infer_context_length("gpt-4.1", "openai", ""), 1_000_000);
        assert_eq!(
            infer_context_length(
                "gpt-5-codex",
                "openai-codex",
                "https://chatgpt.com/backend-api/codex"
            ),
            200_000
        );
    }

    #[test]
    fn runtime_config_adapts_thresholds_to_model_context() {
        let gpt41 =
            ContextCompressionConfig::for_runtime("gpt-4.1", "openai", "https://api.openai.com/v1");
        let local = ContextCompressionConfig::for_runtime(
            "qwen-coder",
            "custom",
            "http://127.0.0.1:1234/v1",
        );

        assert!(gpt41.context_length > local.context_length);
        assert!(gpt41.threshold_tokens > local.threshold_tokens);
        assert!(gpt41.target_tail_tokens >= local.target_tail_tokens);
        assert!(gpt41.max_summary_output_chars >= local.max_summary_output_chars);
    }

    #[test]
    fn runtime_config_prefers_cached_context_length_when_available() {
        let tmp = tempfile::tempdir().expect("tempdir");
        crate::context_limit_cache::save_context_length(
            tmp.path(),
            "qwen-coder",
            "http://127.0.0.1:1234/v1",
            32_768,
        )
        .expect("save cache");

        let config = ContextCompressionConfig::for_runtime_with_data_dir(
            tmp.path(),
            "qwen-coder",
            "custom",
            "http://127.0.0.1:1234/v1",
        );

        assert_eq!(config.context_length, 32_768);
        assert!(config.threshold_tokens <= 32_768);
    }
}
