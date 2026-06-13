use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::agent::Agent;
use crate::config::AppConfig;
use crate::delegate_runs::{DelegateWorkerTask, finalize_record, new_record, save_record};
use crate::runtime_profile::RuntimeProfile;
use crate::tools::{Tool, ToolContext, ToolRegistry, emit_delegate_run_update, truncated};
use crate::types::{ToolDefinition, object_schema};

pub struct DelegateToWorkerTool;

#[derive(Debug, Deserialize)]
struct DelegateToWorkerArgs {
    objective: String,
    focus_goal_id: Option<String>,
    background_summary: Option<String>,
    relevant_state: Option<Value>,
    allowed_tools: Option<Vec<String>>,
    scope: Option<Vec<String>>,
    max_iterations: Option<usize>,
    worker_model: Option<String>,
    context_access: Option<String>,
    output_schema: Option<String>,
}

#[async_trait]
impl Tool for DelegateToWorkerTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "delegate_to_worker",
            "Run a bounded delegated worker task in a child agent session. The parent agent controls the worker objective, background, tool access, and context expansion policy.",
            object_schema(
                json!({
                    "objective": {
                        "type": "string",
                        "description": "Concrete delegated objective for the worker."
                    },
                    "focus_goal_id": {
                        "type": "string",
                        "description": "Optional focus goal id that the worker should align to."
                    },
                    "background_summary": {
                        "type": "string",
                        "description": "Compact parent-provided task background for the worker."
                    },
                    "relevant_state": {
                        "type": "object",
                        "description": "Structured parent state relevant to this worker, such as beliefs, risks, and open questions."
                    },
                    "allowed_tools": {
                        "type": "array",
                        "description": "Optional explicit tool allowlist for the worker. When omitted, the worker gets a conservative default set.",
                        "items": { "type": "string" }
                    },
                    "scope": {
                        "type": "array",
                        "description": "Optional path or topic scope the worker should stay within.",
                        "items": { "type": "string" }
                    },
                    "max_iterations": {
                        "type": "integer",
                        "description": "Optional max iterations for the worker.",
                        "minimum": 1,
                        "maximum": 12
                    },
                    "worker_model": {
                        "type": "string",
                        "description": "Optional model name override for the delegated worker. Uses the parent's endpoint and auth."
                    },
                    "context_access": {
                        "type": "string",
                        "enum": ["none", "brief", "expanded", "full"],
                        "description": "How much parent background the worker may request via read_delegate_context."
                    },
                    "output_schema": {
                        "type": "string",
                        "description": "Optional named result schema label for the worker to target."
                    }
                }),
                &["objective"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: DelegateToWorkerArgs =
            serde_json::from_value(args).context("invalid delegate_to_worker arguments")?;
        let objective = args.objective.trim();
        if objective.is_empty() {
            bail!("delegate_to_worker objective cannot be empty");
        }
        if ctx.delegate_depth >= 2 {
            bail!("delegate_to_worker recursion limit reached");
        }

        let context_access = normalize_context_access(args.context_access.as_deref());
        let available_tools = ToolRegistry::hermes_default(&ctx.data_dir).tool_names();
        let allowed_tools = resolve_allowed_tools(
            args.allowed_tools.unwrap_or_default(),
            &available_tools,
            ctx.shell_enabled,
            &context_access,
        );
        if allowed_tools.is_empty() {
            bail!("delegate_to_worker resolved an empty worker tool allowlist");
        }

        let worker_task = DelegateWorkerTask {
            objective: objective.to_string(),
            focus_goal_id: args.focus_goal_id.clone(),
            background_summary: args
                .background_summary
                .unwrap_or_default()
                .trim()
                .to_string(),
            relevant_state: args.relevant_state.unwrap_or_else(|| json!({})),
            allowed_tools: allowed_tools.clone(),
            scope: args
                .scope
                .unwrap_or_default()
                .into_iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect(),
            context_access: context_access.clone(),
            output_schema: args
                .output_schema
                .clone()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some("worker_result_v1".to_string())),
        };

        let prompt = build_worker_prompt(&worker_task);
        let session_id = format!(
            "{}.delegate.{}",
            ctx.current_session_id,
            Uuid::new_v4().simple()
        );
        let max_iterations = args
            .max_iterations
            .unwrap_or(5)
            .clamp(1, ctx.max_iterations.min(8));
        let worker_runtime = resolve_worker_runtime(args.worker_model.as_deref(), ctx);
        let mut record = new_record(
            &ctx.current_session_id,
            ctx.current_delegate_run_id.as_deref(),
            &session_id,
            &prompt,
            max_iterations,
            1,
            ctx.current_delegate_run_id.as_deref(),
        );
        record.worker_task = Some(worker_task.clone());
        save_record(&ctx.data_dir, &record)?;
        emit_delegate_run_update(&ctx.current_session_id, &record, "delegate_to_worker");
        let provider_kind = if ctx.base_url.contains("/backend-api/codex") {
            "openai-codex".to_string()
        } else {
            "openai".to_string()
        };

        let mut child = Agent::new(AppConfig {
            provider_id: ctx.provider_id.clone(),
            provider_label: ctx.provider_id.clone(),
            provider_kind: provider_kind.clone(),
            model: worker_runtime.model.clone(),
            base_url: worker_runtime.base_url.clone(),
            api_key: worker_runtime.api_key.clone(),
            api_mode: worker_runtime.api_mode,
            skill_platform: ctx.skill_platform.clone(),
            workspace_root: ctx.workspace_root.clone(),
            data_dir: ctx.data_dir.clone(),
            session_id: Some(session_id.clone()),
            max_iterations,
            system_prompt_override: Some(build_worker_identity(&worker_task)),
            tool_allowlist: Some(allowed_tools.clone()),
            enable_shell_tool: ctx.shell_enabled
                && allowed_tools.iter().any(|tool| tool == "terminal"),
            debug_context: false,
            enable_solve_trace_context: false,
            enable_meta_pattern_context: false,
            enable_experience_context: false,
            auxiliary_model: None,
            smart_model_routing: None,
            runtime_profile: RuntimeProfile::fallback(&ctx.workspace_root),
        })?;
        child.set_delegate_depth(ctx.delegate_depth + 1);
        child.set_delegate_run_id(Some(record.id.clone()));
        let response = match child.run_prompt(&prompt).await {
            Ok(response) => {
                let status = if response == "approval_pending" {
                    "awaiting_approval"
                } else {
                    "completed"
                };
                finalize_record(&mut record, status, &response);
                save_record(&ctx.data_dir, &record)?;
                emit_delegate_run_update(&ctx.current_session_id, &record, "delegate_to_worker");
                response
            }
            Err(error) => {
                finalize_record(&mut record, "failed", &error.to_string());
                save_record(&ctx.data_dir, &record)?;
                emit_delegate_run_update(&ctx.current_session_id, &record, "delegate_to_worker");
                return Err(error);
            }
        };

        serde_json::to_string_pretty(&json!({
            "delegate_run_id": record.id,
            "delegate_session_id": session_id,
            "status": record.status,
            "worker_model": worker_runtime.model,
            "worker_api_mode": worker_runtime.api_mode.as_str(),
            "objective": worker_task.objective,
            "focus_goal_id": worker_task.focus_goal_id,
            "allowed_tools": allowed_tools,
            "scope": worker_task.scope,
            "context_access": worker_task.context_access,
            "output_schema": worker_task.output_schema,
            "worker_result": truncated(response, 12_000),
        }))
        .context("failed to serialize delegate_to_worker response")
    }
}

struct ResolvedWorkerRuntime {
    model: String,
    base_url: String,
    api_key: Option<String>,
    api_mode: crate::llm::ApiMode,
}

fn resolve_worker_runtime(
    worker_model_override: Option<&str>,
    ctx: &ToolContext,
) -> ResolvedWorkerRuntime {
    if let Some(worker_model) = worker_model_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return ResolvedWorkerRuntime {
            model: worker_model.to_string(),
            base_url: ctx.base_url.clone(),
            api_key: ctx.api_key.clone(),
            api_mode: ctx.api_mode,
        };
    }

    if let Some(worker_model) = ctx.worker_model.as_ref() {
        return ResolvedWorkerRuntime {
            model: worker_model.model.clone(),
            base_url: worker_model.base_url.clone(),
            api_key: worker_model.api_key.clone(),
            api_mode: worker_model.api_mode,
        };
    }

    ResolvedWorkerRuntime {
        model: ctx.model.clone(),
        base_url: ctx.base_url.clone(),
        api_key: ctx.api_key.clone(),
        api_mode: ctx.api_mode,
    }
}

fn resolve_allowed_tools(
    requested: Vec<String>,
    available_tools: &[String],
    shell_enabled: bool,
    context_access: &str,
) -> Vec<String> {
    let mut allowed = if requested.is_empty() {
        let mut defaults = vec![
            "list_files".to_string(),
            "read_file".to_string(),
            "search_files".to_string(),
        ];
        if shell_enabled {
            defaults.push("terminal".to_string());
        }
        defaults
    } else {
        requested
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    };

    if context_access != "none" {
        allowed.push("read_delegate_context".to_string());
    }

    let mut seen = std::collections::BTreeSet::new();
    allowed.retain(|tool| seen.insert(tool.clone()));
    allowed.retain(|tool| available_tools.iter().any(|available| available == tool));
    allowed
}

fn normalize_context_access(value: Option<&str>) -> String {
    match value
        .unwrap_or("expanded")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "none" => "none".to_string(),
        "brief" => "brief".to_string(),
        "full" => "full".to_string(),
        _ => "expanded".to_string(),
    }
}

fn build_worker_identity(task: &DelegateWorkerTask) -> String {
    let output_schema = task.output_schema.as_deref().unwrap_or("worker_result_v1");
    format!(
        "You are a delegated Crab worker. Stay tightly scoped to the assigned objective, gather evidence with the allowed tools, and do not drift into unrelated work.\n\nWhen you need more parent-task background, use `read_delegate_context` instead of guessing.\n\nReturn a concise final result that targets `{output_schema}` with these fields when possible: summary, key_evidence, candidate_beliefs, candidate_risks, step_updates, recommended_next_actions, raw_refs.\n\nCurrent objective: {}\nCurrent focus goal: {}\nAllowed tools: {}\nScope: {}\nContext access: {}",
        task.objective,
        task.focus_goal_id.as_deref().unwrap_or("(none)"),
        if task.allowed_tools.is_empty() {
            "(none)".to_string()
        } else {
            task.allowed_tools.join(", ")
        },
        if task.scope.is_empty() {
            "(none)".to_string()
        } else {
            task.scope.join(", ")
        },
        task.context_access
    )
}

fn build_worker_prompt(task: &DelegateWorkerTask) -> String {
    let package = json!({
        "objective": task.objective,
        "focus_goal_id": task.focus_goal_id,
        "background_summary": task.background_summary,
        "relevant_state": task.relevant_state,
        "allowed_tools": task.allowed_tools,
        "scope": task.scope,
        "context_access": task.context_access,
        "output_schema": task.output_schema,
    });
    format!(
        "Worker task package:\n{}\n\nUse only the allowed tools above. If the initial task package is not enough, call `read_delegate_context` to fetch more parent background before making assumptions.",
        serde_json::to_string_pretty(&package).unwrap_or_else(|_| package.to_string())
    )
}

#[cfg(test)]
mod tests {
    use super::{DelegateToWorkerTool, resolve_worker_runtime};
    use crate::delegate_runs::list_records;
    use crate::tools::{Tool, ToolContext, WorkerModelConfig};
    use serde_json::json;

    #[tokio::test]
    async fn delegate_to_worker_records_task_metadata() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = DelegateToWorkerTool;
        let output = tool
            .execute(
                json!({
                    "objective": "Summarize the task",
                    "background_summary": "Parent agent already narrowed the issue to src/types.",
                    "relevant_state": {
                        "beliefs": ["trait bounds are suspicious"]
                    },
                    "allowed_tools": ["read_file", "search_files"],
                    "scope": ["src/types"],
                    "max_iterations": 2
                }),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: tmp.path().join(".data"),
                    shell_enabled: false,
                    skill_platform: "cli".to_string(),
                    provider_id: "openai".to_string(),
                    model: "test-model".to_string(),
                    base_url: "mock://final-response".to_string(),
                    api_key: None,
                    api_mode: crate::llm::ApiMode::ChatCompletions,
                    worker_model: None,
                    max_iterations: 4,
                    current_session_id: "parent-session".to_string(),
                    current_delegate_run_id: None,
                    delegate_depth: 0,
                },
            )
            .await
            .expect("delegate output");

        assert!(output.contains("\"status\": \"completed\""));
        assert!(output.contains("\"read_delegate_context\""));

        let records =
            list_records(&tmp.path().join(".data"), Some("parent-session")).expect("records");
        assert_eq!(records.len(), 1);
        let task = records[0].worker_task.as_ref().expect("worker task");
        assert_eq!(task.objective, "Summarize the task");
        assert_eq!(task.scope, vec!["src/types".to_string()]);
        assert!(task.allowed_tools.iter().any(|item| item == "read_file"));
        assert!(
            task.allowed_tools
                .iter()
                .any(|item| item == "read_delegate_context")
        );
    }

    #[test]
    fn worker_runtime_defaults_to_auxiliary_worker_model() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ToolContext {
            workspace_root: tmp.path().to_path_buf(),
            data_dir: tmp.path().join(".data"),
            shell_enabled: false,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "gpt-5.5".to_string(),
            base_url: "https://primary.example/v1".to_string(),
            api_key: Some("primary-key".to_string()),
            api_mode: crate::llm::ApiMode::Responses,
            worker_model: Some(WorkerModelConfig {
                model: "gpt-5.4-mini".to_string(),
                base_url: "https://worker.example/v1".to_string(),
                api_key: Some("worker-key".to_string()),
                api_mode: crate::llm::ApiMode::ChatCompletions,
            }),
            max_iterations: 4,
            current_session_id: "parent-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        };

        let runtime = resolve_worker_runtime(None, &ctx);

        assert_eq!(runtime.model, "gpt-5.4-mini");
        assert_eq!(runtime.base_url, "https://worker.example/v1");
        assert_eq!(runtime.api_key.as_deref(), Some("worker-key"));
        assert_eq!(runtime.api_mode, crate::llm::ApiMode::ChatCompletions);

        let overridden = resolve_worker_runtime(Some("gpt-worker-override"), &ctx);
        assert_eq!(overridden.model, "gpt-worker-override");
        assert_eq!(overridden.base_url, "https://primary.example/v1");
        assert_eq!(overridden.api_key.as_deref(), Some("primary-key"));
        assert_eq!(overridden.api_mode, crate::llm::ApiMode::Responses);
    }
}
