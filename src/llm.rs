use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::types::{
    ChatChoice, ChatMessage, ChatResponse, TokenUsage, ToolCall, ToolDefinition, ToolFunctionCall,
};

#[derive(Debug, Clone, Default)]
pub struct RequestOptions {
    pub max_output_tokens: Option<usize>,
    pub previous_response_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub message: ChatMessage,
    pub response_id: Option<String>,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiMode {
    ChatCompletions,
    Responses,
}

impl ApiMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
        }
    }
}

#[derive(Debug, Serialize)]
struct ResponsesRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_response_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    store: bool,
    stream: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesResponse {
    id: Option<String>,
    #[serde(default)]
    output: Vec<ResponsesOutputItem>,
    #[serde(default)]
    usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesOutputItem {
    #[serde(rename = "type")]
    kind: String,
    id: Option<String>,
    call_id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
    #[serde(default)]
    content: Vec<ResponsesContentItem>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResponsesContentItem {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Clone)]
pub struct OpenAiCompatClient {
    http: Client,
    base_url: String,
    api_key: Option<String>,
    api_mode: ApiMode,
}

impl OpenAiCompatClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        api_mode: ApiMode,
    ) -> Result<Self> {
        let http = Client::builder()
            .user_agent("crab/0.1.0")
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            api_mode,
        })
    }

    pub async fn respond(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ModelResponse> {
        self.respond_with_options(model, messages, tools, RequestOptions::default())
            .await
    }

    pub async fn respond_with_options(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: RequestOptions,
    ) -> Result<ModelResponse> {
        if let Some(response) = self.mock_chat_response(messages, tools, &options) {
            let _ = model;
            return response.and_then(|response| {
                response
                    .into_first_message_and_usage()
                    .map(|(message, usage)| ModelResponse {
                        message,
                        response_id: None,
                        usage,
                    })
            });
        }

        match self.api_mode {
            ApiMode::ChatCompletions => {
                self.chat_completions(model, messages, tools, options).await
            }
            ApiMode::Responses => {
                self.responses_respond(model, messages, tools, options)
                    .await
            }
        }
    }

    pub async fn respond_stream<F>(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        on_text_delta: F,
    ) -> Result<ModelResponse>
    where
        F: FnMut(&str),
    {
        self.respond_stream_with_options(
            model,
            messages,
            tools,
            RequestOptions::default(),
            on_text_delta,
        )
        .await
    }

    pub async fn respond_stream_with_options<F>(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: RequestOptions,
        mut on_text_delta: F,
    ) -> Result<ModelResponse>
    where
        F: FnMut(&str),
    {
        #[cfg(test)]
        if self.base_url == "mock://terminal-approval" {
            let _ = model;
            let _ = messages;
            let _ = tools;
            bail!("mock streaming disabled");
        }

        match self.api_mode {
            ApiMode::ChatCompletions => {
                self.chat_completions_stream(model, messages, tools, options, on_text_delta)
                    .await
            }
            ApiMode::Responses => {
                self.responses_stream(model, messages, tools, options, &mut on_text_delta)
                    .await
            }
        }
    }

    pub async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ChatResponse> {
        self.chat_with_options(model, messages, tools, RequestOptions::default())
            .await
    }

    pub async fn chat_with_options(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: RequestOptions,
    ) -> Result<ChatResponse> {
        let response = self
            .respond_with_options(model, messages, tools, options)
            .await?;
        Ok(ChatResponse {
            choices: vec![ChatChoice {
                message: response.message,
            }],
            usage: response.usage,
        })
    }

    pub async fn chat_stream<F>(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        on_text_delta: F,
    ) -> Result<ChatMessage>
    where
        F: FnMut(&str),
    {
        self.respond_stream(model, messages, tools, on_text_delta)
            .await
            .map(|response| response.message)
    }

    pub async fn chat_stream_with_options<F>(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: RequestOptions,
        on_text_delta: F,
    ) -> Result<ChatMessage>
    where
        F: FnMut(&str),
    {
        self.respond_stream_with_options(model, messages, tools, options, on_text_delta)
            .await
            .map(|response| response.message)
    }

    async fn chat_completions(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: RequestOptions,
    ) -> Result<ModelResponse> {
        self.responses_respond(
            model,
            messages,
            tools,
            RequestOptions {
                max_output_tokens: options.max_output_tokens,
                previous_response_id: None,
            },
        )
        .await
    }

    async fn responses_respond(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: RequestOptions,
    ) -> Result<ModelResponse> {
        let url = format!("{}/responses", self.base_url);
        let (instructions, input, previous_response_id) =
            responses_request_parts(messages, options.previous_response_id);
        let body = ResponsesRequest {
            model: model.to_string(),
            instructions,
            input,
            previous_response_id,
            tools: responses_tools(tools),
            max_output_tokens: options.max_output_tokens,
            tool_choice: (!tools.is_empty()).then(|| "auto".to_string()),
            store: false,
            stream: false,
        };

        let response = self.send_json(url, &body).await?;
        let text = response
            .text()
            .await
            .context("failed to read responses API body")?;
        let parsed: ResponsesResponse =
            serde_json::from_str(&text).context("failed to parse responses API body")?;
        responses_to_model_response(parsed)
    }

    async fn chat_completions_stream<F>(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: RequestOptions,
        mut on_text_delta: F,
    ) -> Result<ModelResponse>
    where
        F: FnMut(&str),
    {
        self.responses_stream(
            model,
            messages,
            tools,
            RequestOptions {
                max_output_tokens: options.max_output_tokens,
                previous_response_id: None,
            },
            &mut on_text_delta,
        )
        .await
    }

    async fn responses_stream<F>(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: RequestOptions,
        on_text_delta: &mut F,
    ) -> Result<ModelResponse>
    where
        F: FnMut(&str),
    {
        let url = format!("{}/responses", self.base_url);
        let (instructions, input, previous_response_id) =
            responses_request_parts(messages, options.previous_response_id);
        let body = ResponsesRequest {
            model: model.to_string(),
            instructions,
            input,
            previous_response_id,
            tools: responses_tools(tools),
            max_output_tokens: options.max_output_tokens,
            tool_choice: (!tools.is_empty()).then(|| "auto".to_string()),
            store: false,
            stream: true,
        };

        let response = self
            .send_json(url, &body)
            .await
            .context("streaming responses request failed")?;
        let mut buffer = String::new();
        let mut stream = response.bytes_stream();
        let mut fallback_text = String::new();
        let mut final_response: Option<ResponsesResponse> = None;

        while let Some(chunk) = stream.next().await {
            let chunk: bytes::Bytes = chunk.context("failed to read responses chunk")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(event_end) = buffer.find("\n\n") {
                let raw_event = buffer[..event_end].to_string();
                buffer.drain(..event_end + 2);
                let data = raw_event
                    .lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() {
                    continue;
                }
                if data.trim() == "[DONE]" {
                    break;
                }

                let event: Value = serde_json::from_str(&data)
                    .with_context(|| format!("failed to parse responses stream event: {data}"))?;
                match event
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                {
                    "response.output_text.delta" => {
                        if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                            on_text_delta(delta);
                            fallback_text.push_str(delta);
                        }
                    }
                    "response.completed" => {
                        if let Some(response_value) = event.get("response").cloned() {
                            final_response = Some(
                                serde_json::from_value(response_value)
                                    .context("failed to decode completed responses payload")?,
                            );
                        }
                    }
                    "response.error" => {
                        let message = event
                            .get("error")
                            .and_then(|value| value.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("responses API stream error");
                        bail!("{message}");
                    }
                    _ => {}
                }
            }
        }

        let response = match final_response {
            Some(response) => responses_to_model_response(response)?,
            None => ModelResponse {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: (!fallback_text.is_empty()).then_some(Value::String(fallback_text)),
                    tool_calls: None,
                    tool_call_id: None,
                },
                response_id: None,
                usage: None,
            },
        };
        Ok(response)
    }

    async fn send_json<T: Serialize>(&self, url: String, body: &T) -> Result<reqwest::Response> {
        let mut request = self.http.post(url).json(body);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request.send().await.context("model request failed")?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            let text = response.text().await.context("failed to read error body")?;
            let mut message = format!("model API returned {status}: {text}");
            if let Some(retry_after) = retry_after {
                message.push_str(&format!(" retry_after: {retry_after}"));
            }
            bail!("{message}");
        }
        Ok(response)
    }

    fn mock_chat_response(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        options: &RequestOptions,
    ) -> Option<Result<ChatResponse>> {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static RATE_LIMIT_THEN_SUCCESS_CALLS: AtomicUsize = AtomicUsize::new(0);

        if self.base_url == "mock://compression-error" {
            return Some(Err(anyhow::anyhow!("mock compression failure")));
        }

        if self.base_url == "mock://output-cap-retry" {
            let cap = options.max_output_tokens.unwrap_or(128_000);
            if cap > 9_936 {
                return Some(Err(anyhow::anyhow!(
                    "max_tokens: 32768 > context_window: 200000 - input_tokens: 190000 = available_tokens: 10000"
                )));
            }
            return Some(Ok(ChatResponse {
                choices: vec![crate::types::ChatChoice {
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content: Some(Value::String(
                            "mock final response after output retry".to_string(),
                        )),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                }],
                usage: None,
            }));
        }

        if self.base_url == "mock://context-overflow-retry" {
            let has_summary = messages.iter().any(|message| {
                message.role == "system" && message.content_text().contains("[CONTEXT COMPACTION]")
            });
            if !has_summary {
                return Some(Err(anyhow::anyhow!(
                    "prompt is too long: 205000 tokens > 200000 maximum"
                )));
            }
            return Some(Ok(ChatResponse {
                choices: vec![crate::types::ChatChoice {
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content: Some(Value::String(
                            "mock final response after context retry".to_string(),
                        )),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                }],
                usage: None,
            }));
        }

        if self.base_url == "mock://rate-limit-then-success" {
            let call = RATE_LIMIT_THEN_SUCCESS_CALLS.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                return Some(Err(anyhow::anyhow!(
                    "model API returned 429 Too Many Requests: rate limit exceeded retry_after_ms: 1"
                )));
            }
            return Some(Ok(ChatResponse {
                choices: vec![crate::types::ChatChoice {
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content: Some(Value::String(
                            "mock final response after rate limit retry".to_string(),
                        )),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                }],
                usage: None,
            }));
        }

        if self.base_url == "mock://endless-tool-loop" {
            if tools.is_empty() {
                return Some(Ok(ChatResponse {
                    choices: vec![crate::types::ChatChoice {
                        message: ChatMessage {
                            role: "assistant".to_string(),
                            content: Some(Value::String(
                                "mock final response after tool budget".to_string(),
                            )),
                            tool_calls: None,
                            tool_call_id: None,
                        },
                    }],
                    usage: None,
                }));
            }
            return Some(Ok(ChatResponse {
                choices: vec![crate::types::ChatChoice {
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content: None,
                        tool_calls: Some(vec![ToolCall {
                            id: format!("call-list-files-{}", messages.len()),
                            kind: "function".to_string(),
                            function: ToolFunctionCall {
                                name: "list_files".to_string(),
                                arguments: json!({
                                    "path": ".",
                                    "recursive": false,
                                    "max_results": 5
                                })
                                .to_string(),
                            },
                        }]),
                        tool_call_id: None,
                    },
                }],
                usage: None,
            }));
        }

        if self.base_url == "mock://invalid-tool-arguments" {
            let tool_message = messages.iter().any(|message| message.role == "tool");
            if tool_message {
                return Some(Ok(ChatResponse {
                    choices: vec![crate::types::ChatChoice {
                        message: ChatMessage {
                            role: "assistant".to_string(),
                            content: Some(Value::String(
                                "recovered after invalid tool arguments".to_string(),
                            )),
                            tool_calls: None,
                            tool_call_id: None,
                        },
                    }],
                    usage: None,
                }));
            }
            return Some(Ok(ChatResponse {
                choices: vec![crate::types::ChatChoice {
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content: None,
                        tool_calls: Some(vec![ToolCall {
                            id: "call-read-file-invalid-json".to_string(),
                            kind: "function".to_string(),
                            function: ToolFunctionCall {
                                name: "read_file".to_string(),
                                arguments: r#"{"path":"README.md""#.to_string(),
                            },
                        }]),
                        tool_call_id: None,
                    },
                }],
                usage: None,
            }));
        }

        if self.base_url == "mock://final-response"
            || self.base_url == "mock://codex-final-response"
            || self.base_url == "mock://auxiliary-summary-title"
            || self.base_url == "mock://compression-long-summary"
        {
            let system_prompt = messages
                .first()
                .map(ChatMessage::content_text)
                .unwrap_or_default();
            let user_prompt = messages
                .iter()
                .find(|message| message.role == "user")
                .map(ChatMessage::content_text)
                .unwrap_or_default();
            let content = if system_prompt
                .contains("You are refining aggregated agent meta-patterns")
            {
                r#"{"patterns":[{"id":"pattern:build","model_summary":"Shared trait changes tend to fan out across implementors.","match_hints":["trait errors across modules","shared signature changed recently"],"strategy_template":{"applicable_when":["compiler errors cluster around shared trait changes"],"preferred_actions":["compare trait signature with implementors"],"avoid":["patching symptoms first"],"escalate_when":["signature ownership is unclear"]},"confidence":0.86}]}"#.to_string()
            } else if system_prompt.contains("Generate a short, descriptive conversation title") {
                if self.base_url == "mock://auxiliary-summary-title" {
                    "Title: Auxiliary Session Title".to_string()
                } else {
                    "Title: Mock Session Title".to_string()
                }
            } else if system_prompt
                .contains("You are reconciling an agent's goal-state after a tool outcome")
            {
                if user_prompt.contains("status=approval_required")
                    || user_prompt.contains("approval denied")
                    || user_prompt.contains("not found")
                    || user_prompt.contains("no file found")
                    || user_prompt.contains("status=done\nresult=no ")
                {
                    r#"{"current_focus_goal_id":"goal-current","goals":[{"id":"goal-current","title":"Current goal","level":"current","status":"blocked","confidence":0.42,"summary":"Blocked after tool outcome","evidence":"Tool outcome requires follow-up or clarification.","evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"Latest tool outcome introduced a blocker or missing dependency.","relation":"supports","observed_at_unix":1735689600}]}],"cognition":[{"id":"reconcile:blocker","kind":"risk","content":"The current goal is blocked pending clarification or a missing dependency.","confidence":0.88,"evidence":"Derived from the latest tool outcome.","evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"Latest tool outcome introduced a blocker or missing dependency.","relation":"supports","observed_at_unix":1735689600}]}],"hot_data":[{"id":"reconcile:next-step","content":"Resolve the blocker before continuing the current goal.","confidence":0.84,"source":"goal_state_reconcile","goal_id":"goal-current","evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"Need to resolve the blocker before the next action.","relation":"context","observed_at_unix":1735689600}]}]}"#.to_string()
                } else {
                    r#"{"current_focus_goal_id":"goal-current","goals":[{"id":"goal-current","title":"Current goal","level":"current","status":"in_progress","confidence":0.83,"summary":"Tool outcome supports the current path","evidence":"Latest tool result was consistent with the plan.","evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"Latest tool result was consistent with the plan.","relation":"supports","observed_at_unix":1735689600}]}],"cognition":[{"id":"reconcile:progress","kind":"fact","content":"The latest tool outcome supports the current goal and keeps it in progress.","confidence":0.86,"evidence":"Derived from the latest tool outcome.","evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"The latest tool outcome supports the current goal.","relation":"supports","observed_at_unix":1735689600}]}],"hot_data":[{"id":"reconcile:next-step","content":"Continue with the next action under the current goal.","confidence":0.8,"source":"goal_state_reconcile","goal_id":"goal-current","evidence_items":[{"source_type":"tool_output","source":"goal_state_reconcile","summary":"The next action remains valid after the tool outcome.","relation":"context","observed_at_unix":1735689600}]}]}"#.to_string()
                }
            } else if system_prompt
                .contains("You are compressing a tool result for goal-state maintenance")
            {
                if user_prompt.contains("tool_error:")
                    || user_prompt.contains("not found")
                    || user_prompt.contains("approval_required")
                    || user_prompt.contains("denied")
                {
                    r#"{"summary":"Tool outcome indicates a blocker or missing dependency.","key_evidence":["The result contains an error or missing-resource signal."],"candidate_hot_data":["The latest tool outcome needs follow-up before continuing."],"candidate_risks":["Proceeding without resolving this blocker may waste effort."]}"#.to_string()
                } else {
                    r#"{"summary":"Tool outcome supports the current path.","key_evidence":["The result is consistent with the current plan."],"candidate_hot_data":["The latest tool outcome can inform the next step."],"candidate_risks":[]}"#.to_string()
                }
            } else if system_prompt
                .contains("You are planning an agent's goal-state from a new user request")
            {
                if user_prompt.contains("implement")
                    || user_prompt.contains("Implement")
                    || user_prompt.contains("feature")
                {
                    r#"{"current_focus_goal_id":"goal-implement","goals":[{"id":"goal-implement","title":"Implement requested feature","level":"current","status":"in_progress","confidence":0.78,"summary":"Deliver the user-requested implementation","evidence":"Directly stated in the user request.","evidence_items":[{"source_type":"user_input","source":"goal_state_plan","summary":"The user explicitly requested implementation work.","relation":"supports","observed_at_unix":1735689600}]},{"id":"goal-implement-sub-1","title":"Inspect relevant code and constraints","level":"subgoal","status":"pending","confidence":0.74,"parent_id":"goal-implement","summary":"Understand the current implementation surface","evidence":"Needed before changing code.","evidence_items":[{"source_type":"model_inference","source":"goal_state_plan","summary":"Inspection is needed before making code changes.","relation":"supports","observed_at_unix":1735689600}]},{"id":"goal-implement-sub-2","title":"Apply and verify the change","level":"subgoal","status":"pending","confidence":0.76,"parent_id":"goal-implement","summary":"Implement and validate the requested behavior","evidence":"Needed to complete the request.","evidence_items":[{"source_type":"model_inference","source":"goal_state_plan","summary":"Implementation and verification are needed to complete the request.","relation":"supports","observed_at_unix":1735689600}]}],"cognition":[{"id":"plan-current-request","kind":"assumption","content":"The user expects direct implementation work rather than discussion only.","confidence":0.72,"evidence":"The request asks to continue building the system.","evidence_items":[{"source_type":"user_input","source":"goal_state_plan","summary":"The request says to continue building the system.","relation":"supports","observed_at_unix":1735689600}]}],"hot_data":[{"id":"plan-next-step","content":"Start by inspecting the code paths that control the requested feature.","confidence":0.77,"source":"goal_state_plan","goal_id":"goal-implement","evidence_items":[{"source_type":"model_inference","source":"goal_state_plan","summary":"Inspection should happen before implementation.","relation":"context","observed_at_unix":1735689600}]}]}"#.to_string()
                } else {
                    r#"{"current_focus_goal_id":"goal-current","goals":[{"id":"goal-current","title":"Handle current request","level":"current","status":"in_progress","confidence":0.7,"summary":"Work the latest user request","evidence":"Directly stated in the user request.","evidence_items":[{"source_type":"user_input","source":"goal_state_plan","summary":"The user explicitly stated the current request.","relation":"supports","observed_at_unix":1735689600}]}],"cognition":[{"id":"plan-current-request","kind":"assumption","content":"The current request should stay as the active focus.","confidence":0.68,"evidence":"No stronger alternative focus was implied.","evidence_items":[{"source_type":"model_inference","source":"goal_state_plan","summary":"No better alternative focus is visible from the current request.","relation":"supports","observed_at_unix":1735689600}]}],"hot_data":[{"id":"plan-next-step","content":"Continue with the active request using the latest context.","confidence":0.7,"source":"goal_state_plan","goal_id":"goal-current","evidence_items":[{"source_type":"model_inference","source":"goal_state_plan","summary":"The next step is to continue the active request.","relation":"context","observed_at_unix":1735689600}]}]}"#.to_string()
                }
            } else if system_prompt
                .contains("You are reconciling an agent's goal-state at the end of a turn")
            {
                if user_prompt.contains("Implemented")
                    || user_prompt.contains("Verified")
                    || user_prompt.contains("Completed")
                    || user_prompt.contains("verified")
                    || user_prompt.contains("completed")
                {
                    r#"{"current_focus_goal_id":"goal-current","goals":[{"id":"goal-current","title":"Current goal","level":"current","status":"succeeded","confidence":0.91,"summary":"The turn resolved the current goal","evidence":"Final assistant response indicates the requested work is complete.","evidence_items":[{"source_type":"assistant_response","source":"turn_end_reconcile","summary":"The final assistant response indicates completion.","relation":"supports","observed_at_unix":1735689600}]}],"cognition":[{"id":"turn-end:completion","kind":"decision","content":"The current goal is complete for this turn.","confidence":0.9,"evidence":"Derived from the final assistant response.","evidence_items":[{"source_type":"assistant_response","source":"turn_end_reconcile","summary":"The turn ended with a completion signal.","relation":"supports","observed_at_unix":1735689600}]}],"hot_data":[{"id":"turn-end:result","content":"Implementation completed and the current goal can be treated as done.","confidence":0.88,"source":"turn_end_reconcile","goal_id":"goal-current","evidence_items":[{"source_type":"assistant_response","source":"turn_end_reconcile","summary":"Completion should stay hot for the next turn.","relation":"context","observed_at_unix":1735689600}]}]}"#.to_string()
                } else {
                    r#"{"current_focus_goal_id":"goal-current","goals":[{"id":"goal-current","title":"Current goal","level":"current","status":"in_progress","confidence":0.79,"summary":"The current goal remains active after the turn","evidence":"Final assistant response did not indicate completion.","evidence_items":[{"source_type":"assistant_response","source":"turn_end_reconcile","summary":"The final assistant response did not indicate completion.","relation":"supports","observed_at_unix":1735689600}]}],"cognition":[{"id":"turn-end:progress","kind":"decision","content":"The current goal remains active after this turn.","confidence":0.78,"evidence":"Derived from the final assistant response.","evidence_items":[{"source_type":"assistant_response","source":"turn_end_reconcile","summary":"The turn ended without completion.","relation":"supports","observed_at_unix":1735689600}]}],"hot_data":[{"id":"turn-end:next-step","content":"Continue working the current goal in the next turn.","confidence":0.76,"source":"turn_end_reconcile","goal_id":"goal-current","evidence_items":[{"source_type":"assistant_response","source":"turn_end_reconcile","summary":"The current goal should stay active for the next turn.","relation":"context","observed_at_unix":1735689600}]}]}"#.to_string()
                }
            } else if system_prompt.contains(
                "You are distilling an agent's current goal-state into a short execution brief",
            ) {
                r#"{"focus":"[goal-current] Current goal | status=in_progress | confidence=0.83","next_step":"Inspect the relevant code path before editing.","watch":"Keep changes aligned to the active goal and avoid unrelated cleanup.","operating_rule":"Advance the current goal directly; if evidence conflicts, update goal_state before branching.","confidence":0.84}"#.to_string()
            } else if system_prompt.contains(
                "You are proposing a small, justified state update for an agent's goal-state",
            ) {
                r#"{"mission":"Implement the current request cleanly","phase":"investigate","current_focus_goal_id":"goal-current","reflection":"The next useful update is to confirm the relevant code path before editing.","rationale":"The latest request points to implementation work, but the safest immediate move is still targeted inspection."}"#.to_string()
            } else if system_prompt
                .contains("You are compressing an agent's goal-state working memory")
            {
                r#"{"cognition":"Compacted cognition from auxiliary model","hot_data":"Compacted hot data from auxiliary model","confidence":0.82}"#.to_string()
            } else if system_prompt
                .contains("You are compressing an ongoing coding-agent conversation")
            {
                if self.base_url == "mock://auxiliary-summary-title" {
                    "## Goal\nAuxiliary summary generated by sidecar model\n\n## Constraints & Preferences\nKeep the main chat model free for foreground turns\n\n## Progress\n### Done\nCompacted older turns with auxiliary lane\n### In Progress\nContinue implementation\n### Blocked\nNone\n\n## Key Decisions\nUse auxiliary model for background summarization\n\n## Relevant Files\nsrc/agent.rs\n\n## Next Steps\nResume foreground chat\n\n## Critical Context\nSummary came from auxiliary model\n\n## Tools & Patterns\nPrefer sidecar model for non-user-visible background work".to_string()
                } else if self.base_url == "mock://compression-long-summary" {
                    format!(
                        "## Goal\n{}\n\n## Constraints & Preferences\n{}\n\n## Progress\n### Done\n{}\n### In Progress\n{}\n### Blocked\nNone\n\n## Key Decisions\n{}\n\n## Relevant Files\n{}\n\n## Next Steps\n{}\n\n## Critical Context\n{}\n\n## Tools & Patterns\n{}",
                        "L".repeat(12_000),
                        "C".repeat(6_000),
                        "D".repeat(12_000),
                        "I".repeat(6_000),
                        "K".repeat(6_000),
                        "F".repeat(6_000),
                        "N".repeat(6_000),
                        "X".repeat(6_000),
                        "T".repeat(6_000),
                    )
                } else if user_prompt.contains("PREVIOUS SUMMARY:") {
                    "## Goal\nUpdated summary\n\n## Constraints & Preferences\nNone\n\n## Progress\n### Done\nUpdated prior work\n### In Progress\nContinue implementation\n### Blocked\nNone\n\n## Key Decisions\nKeep prior summary and add new work\n\n## Relevant Files\nsrc/context_compression.rs\n\n## Next Steps\nProceed with latest state\n\n## Critical Context\nPrevious compacted work preserved\n\n## Tools & Patterns\nUse structured summaries".to_string()
                } else {
                    "## Goal\nMock summary\n\n## Constraints & Preferences\nNone\n\n## Progress\n### Done\nSummarized older turns\n### In Progress\nContinue implementation\n### Blocked\nNone\n\n## Key Decisions\nUse compact summary handoff\n\n## Relevant Files\nsrc/context_compression.rs\n\n## Next Steps\nKeep working from current state\n\n## Critical Context\nImportant prior steps preserved\n\n## Tools & Patterns\nUse concise tool traces".to_string()
                }
            } else {
                "mock final response".to_string()
            };
            return Some(Ok(ChatResponse {
                choices: vec![crate::types::ChatChoice {
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content: Some(Value::String(content)),
                        tool_calls: None,
                        tool_call_id: None,
                    },
                }],
                usage: Some(mock_usage()),
            }));
        }

        if self.base_url != "mock://terminal-approval" {
            return None;
        }

        let tool_message = messages
            .iter()
            .rev()
            .find(|message| message.role == "tool")
            .map(ChatMessage::content_text);

        let message = match tool_message {
            Some(text) if text.contains("approval denied for command") => ChatMessage {
                role: "assistant".to_string(),
                content: Some(Value::String("approval denied acknowledged".to_string())),
                tool_calls: None,
                tool_call_id: None,
            },
            Some(_) => ChatMessage {
                role: "assistant".to_string(),
                content: Some(Value::String("command completed".to_string())),
                tool_calls: None,
                tool_call_id: None,
            },
            None => ChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call-terminal".to_string(),
                    kind: "function".to_string(),
                    function: ToolFunctionCall {
                        name: "terminal".to_string(),
                        arguments: json!({ "command": "rm -rf hello.txt" }).to_string(),
                    },
                }]),
                tool_call_id: None,
            },
        };

        Some(Ok(ChatResponse {
            choices: vec![crate::types::ChatChoice { message }],
            usage: None,
        }))
    }
}

fn responses_request_parts(
    messages: &[ChatMessage],
    previous_response_id: Option<String>,
) -> (Option<String>, Vec<Value>, Option<String>) {
    let instructions = responses_instructions(messages);
    if previous_response_id.is_none() {
        return (instructions, responses_input(messages), None);
    }

    let last_assistant_idx = messages
        .iter()
        .rposition(|message| message.role == "assistant");
    let Some(last_assistant_idx) = last_assistant_idx else {
        return (
            instructions,
            responses_input(messages),
            previous_response_id,
        );
    };

    let delta = responses_input(&messages[last_assistant_idx + 1..]);
    if delta.is_empty() {
        (instructions, responses_input(messages), None)
    } else {
        (instructions, delta, previous_response_id)
    }
}

fn responses_input(messages: &[ChatMessage]) -> Vec<Value> {
    let mut items = Vec::new();
    for message in messages {
        if message.role == "system" {
            continue;
        }
        let text = message.content_text();
        if message.role != "tool" && !text.trim().is_empty() {
            items.push(json!({
                "role": message.role,
                "content": text,
            }));
        }
        if let Some(tool_calls) = &message.tool_calls {
            for tool_call in tool_calls {
                items.push(json!({
                    "type": "function_call",
                    "call_id": tool_call.id,
                    "name": tool_call.function.name,
                    "arguments": tool_call.function.arguments,
                }));
            }
        }
        if message.role == "tool" {
            items.push(json!({
                "type": "function_call_output",
                "call_id": message.tool_call_id.clone().unwrap_or_default(),
                "output": text,
            }));
        }
    }
    items
}

fn responses_instructions(messages: &[ChatMessage]) -> Option<String> {
    let instructions = messages
        .iter()
        .filter(|message| message.role == "system")
        .map(ChatMessage::content_text)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    if instructions.is_empty() {
        None
    } else {
        Some(instructions.join("\n\n"))
    }
}

fn responses_tools(tools: &[ToolDefinition]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.function.name,
                "description": tool.function.description,
                "parameters": tool.function.parameters,
            })
        })
        .collect()
}

fn responses_to_model_response(response: ResponsesResponse) -> Result<ModelResponse> {
    let response_id = response.id.clone();
    let usage = response.usage.map(TokenUsage::normalized);
    let mut content = String::new();
    let mut tool_calls = Vec::new();

    for item in response.output {
        match item.kind.as_str() {
            "message" => {
                for part in item.content {
                    if matches!(part.kind.as_str(), "output_text" | "text" | "input_text") {
                        if let Some(text) = part.text {
                            content.push_str(&text);
                        }
                    }
                }
            }
            "function_call" => {
                let name = item.name.unwrap_or_default();
                if name.trim().is_empty() {
                    return Err(anyhow!(
                        "responses payload had function_call without a name"
                    ));
                }
                tool_calls.push(ToolCall {
                    id: item.call_id.or(item.id).unwrap_or_else(|| name.clone()),
                    kind: "function".to_string(),
                    function: ToolFunctionCall {
                        name,
                        arguments: item.arguments.unwrap_or_else(|| "{}".to_string()),
                    },
                });
            }
            _ => {}
        }
    }

    Ok(ModelResponse {
        message: ChatMessage {
            role: "assistant".to_string(),
            content: (!content.is_empty()).then_some(Value::String(content)),
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
            tool_call_id: None,
        },
        response_id,
        usage,
    })
}

fn mock_usage() -> TokenUsage {
    TokenUsage {
        prompt_tokens: Some(11),
        completion_tokens: Some(7),
        total_tokens: Some(18),
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiMode, OpenAiCompatClient, RequestOptions};
    use crate::types::{ChatMessage, ChatRequest, ToolDefinition, object_schema};

    #[tokio::test]
    async fn respond_returns_message_without_chat_choice_wrapper() {
        let client = OpenAiCompatClient::new("mock://final-response", None, ApiMode::Responses)
            .expect("client");

        let response = client
            .respond("gpt-test", &[ChatMessage::user("hello")], &[])
            .await
            .expect("respond");

        assert_eq!(response.message.role, "assistant");
        assert_eq!(response.message.content_text(), "mock final response");
        assert!(response.response_id.is_none());
        assert_eq!(
            response.usage.as_ref().and_then(|usage| usage.total_tokens),
            Some(18)
        );
    }

    #[test]
    fn responses_usage_maps_to_token_usage() {
        let response: super::ResponsesResponse = serde_json::from_value(serde_json::json!({
            "id": "resp_123",
            "usage": {
                "input_tokens": 123,
                "output_tokens": 45
            },
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": "hello"
                }]
            }]
        }))
        .expect("response json");

        let model_response = super::responses_to_model_response(response).expect("model response");
        let usage = model_response.usage.expect("usage");

        assert_eq!(usage.prompt_tokens, Some(123));
        assert_eq!(usage.completion_tokens, Some(45));
        assert_eq!(usage.total_tokens, Some(168));
    }

    #[tokio::test]
    async fn legacy_chat_wrapper_still_wraps_respond_output() {
        let client =
            OpenAiCompatClient::new("mock://endless-tool-loop", None, ApiMode::ChatCompletions)
                .expect("client");
        let tools = vec![ToolDefinition::function(
            "list_files",
            "List files",
            object_schema(serde_json::json!({}), &[]),
        )];

        let response = client
            .chat_with_options(
                "gpt-test",
                &[ChatMessage::user("inspect")],
                &tools,
                RequestOptions::default(),
            )
            .await
            .expect("chat_with_options");

        let message = response.first_message().expect("first message");
        let tool_calls = message.tool_calls.expect("tool calls");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].function.name, "list_files");
    }

    #[test]
    fn chat_request_type_stays_serializable_for_compatibility() {
        let request = ChatRequest {
            model: "gpt-test".to_string(),
            messages: vec![ChatMessage::user("hello")],
            tools: Vec::new(),
            max_tokens: None,
            tool_choice: None,
            stream: false,
        };

        let json = serde_json::to_string(&request).expect("serialize chat request");
        assert!(json.contains("\"messages\""));
    }

    #[test]
    fn responses_continuation_uses_only_delta_after_latest_assistant() {
        let messages = vec![
            ChatMessage::system("system"),
            ChatMessage::user("first user"),
            ChatMessage::assistant("first assistant"),
            ChatMessage::tool("call-1", "tool output"),
            ChatMessage::user("next user"),
        ];

        let (instructions, items, previous_response_id) =
            super::responses_request_parts(&messages, Some("resp_123".to_string()));

        assert_eq!(instructions.as_deref(), Some("system"));
        assert_eq!(previous_response_id.as_deref(), Some("resp_123"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["type"], "function_call_output");
        assert_eq!(items[1]["role"], "user");
        assert_eq!(items[1]["content"], "next user");
    }

    #[test]
    fn responses_extract_system_messages_into_instructions() {
        let messages = vec![
            ChatMessage::system("system a"),
            ChatMessage::system("system b"),
            ChatMessage::user("hello"),
        ];

        let (instructions, items, previous_response_id) =
            super::responses_request_parts(&messages, None);

        assert_eq!(instructions.as_deref(), Some("system a\n\nsystem b"));
        assert!(previous_response_id.is_none());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["role"], "user");
        assert_eq!(items[0]["content"], "hello");
    }
}
