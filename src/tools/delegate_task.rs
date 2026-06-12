use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::agent::Agent;
use crate::config::AppConfig;
use crate::delegate_runs::{finalize_record, new_record, save_record};
use crate::providers::infer_api_mode_for_endpoint;
use crate::runtime_profile::RuntimeProfile;
use crate::tools::{Tool, ToolContext, emit_delegate_run_update, truncated};
use crate::types::{ToolDefinition, object_schema};

pub struct DelegateTaskTool;

#[derive(Debug, Deserialize)]
struct DelegateTaskArgs {
    prompt: String,
    max_iterations: Option<usize>,
}

#[async_trait]
impl Tool for DelegateTaskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "delegate_task",
            "Run a bounded delegated subtask in a child agent session and return its result.",
            object_schema(
                json!({
                    "prompt": {
                        "type": "string",
                        "description": "Concrete delegated task for the subagent."
                    },
                    "max_iterations": {
                        "type": "integer",
                        "description": "Optional max iterations for the delegated run. Defaults to a low bounded value."
                    }
                }),
                &["prompt"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: DelegateTaskArgs =
            serde_json::from_value(args).context("invalid delegate_task arguments")?;
        let prompt = args.prompt.trim();
        if prompt.is_empty() {
            bail!("delegate_task prompt cannot be empty");
        }
        if ctx.delegate_depth >= 2 {
            bail!("delegate_task recursion limit reached");
        }

        let session_id = format!(
            "{}.delegate.{}",
            ctx.current_session_id,
            Uuid::new_v4().simple()
        );
        let max_iterations = args
            .max_iterations
            .unwrap_or(4)
            .clamp(1, ctx.max_iterations.min(6));
        let mut record = new_record(
            &ctx.current_session_id,
            ctx.current_delegate_run_id.as_deref(),
            &session_id,
            prompt,
            max_iterations,
            1,
            ctx.current_delegate_run_id.as_deref(),
        );
        save_record(&ctx.data_dir, &record)?;
        emit_delegate_run_update(&ctx.current_session_id, &record, "delegate_task");
        let provider_kind = if ctx.base_url.contains("/backend-api/codex") {
            "openai-codex".to_string()
        } else {
            "openai".to_string()
        };
        let mut child = Agent::new(AppConfig {
            provider_id: ctx.provider_id.clone(),
            provider_label: ctx.provider_id.clone(),
            provider_kind: provider_kind.clone(),
            model: ctx.model.clone(),
            base_url: ctx.base_url.clone(),
            api_key: ctx.api_key.clone(),
            api_mode: infer_api_mode_for_endpoint(&provider_kind, &ctx.base_url, None),
            skill_platform: ctx.skill_platform.clone(),
            workspace_root: ctx.workspace_root.clone(),
            data_dir: ctx.data_dir.clone(),
            session_id: Some(session_id.clone()),
            max_iterations,
            system_prompt_override: Some(
                "You are a delegated Crab subagent. Focus on the requested subtask only and return a concise result for the parent agent.".to_string(),
            ),
            tool_allowlist: None,
            enable_shell_tool: ctx.shell_enabled,
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
        let response = match child.run_prompt(prompt).await {
            Ok(response) => {
                let status = if response == "approval_pending" {
                    "awaiting_approval"
                } else {
                    "completed"
                };
                finalize_record(&mut record, status, &response);
                save_record(&ctx.data_dir, &record)?;
                emit_delegate_run_update(&ctx.current_session_id, &record, "delegate_task");
                response
            }
            Err(error) => {
                finalize_record(&mut record, "failed", &error.to_string());
                save_record(&ctx.data_dir, &record)?;
                emit_delegate_run_update(&ctx.current_session_id, &record, "delegate_task");
                return Err(error);
            }
        };

        Ok(format!(
            "delegate_session: {}\ndelegate_status: {}\nmax_iterations: {}\nresult:\n{}",
            session_id,
            record.status,
            max_iterations,
            truncated(response, 12_000)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::DelegateTaskTool;
    use crate::delegate_runs::list_records;
    use crate::tools::{
        Tool, ToolContext, ToolRuntimeEvent, clear_tool_event_sender, register_tool_event_sender,
        with_tool_runtime_scope,
    };
    use serde_json::json;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn rejects_excessive_delegate_depth() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = DelegateTaskTool;
        let error = tool
            .execute(
                json!({ "prompt": "inspect the project" }),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: tmp.path().join(".data"),
                    shell_enabled: false,
                    skill_platform: "cli".to_string(),
                    provider_id: "openai".to_string(),
                    model: "test-model".to_string(),
                    base_url: "https://example.invalid/v1".to_string(),
                    api_key: None,
                    max_iterations: 4,
                    current_session_id: "test-session".to_string(),
                    current_delegate_run_id: None,
                    delegate_depth: 2,
                },
            )
            .await
            .expect_err("expected recursion limit");

        assert!(error.to_string().contains("recursion limit"));
    }

    #[tokio::test]
    async fn records_completed_delegate_run() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = DelegateTaskTool;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        register_tool_event_sender("parent-session", sender);
        let ctx = ToolContext {
            workspace_root: tmp.path().to_path_buf(),
            data_dir: tmp.path().join(".data"),
            shell_enabled: false,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "mock://final-response".to_string(),
            api_key: None,
            max_iterations: 4,
            current_session_id: "parent-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        };
        let output = with_tool_runtime_scope(
            "call-delegate".to_string(),
            tool.execute(json!({ "prompt": "summarize", "max_iterations": 2 }), &ctx),
        )
        .await
        .expect("delegate output");
        clear_tool_event_sender("parent-session");

        let mut runtime_events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            runtime_events.push(event);
        }
        let delegate_events = runtime_events
            .iter()
            .filter_map(|event| match event {
                ToolRuntimeEvent::DelegateRunUpdated {
                    tool_call_id,
                    status,
                    objective_preview,
                    result_preview,
                    ..
                } => Some((
                    tool_call_id.as_str(),
                    status.as_str(),
                    objective_preview.as_str(),
                    result_preview.as_str(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            delegate_events.first(),
            Some(("call-delegate", "running", objective, _)) if objective.contains("summarize")
        ));
        assert!(matches!(
            delegate_events.last(),
            Some(("call-delegate", "completed", objective, result))
                if objective.contains("summarize") && result.contains("mock final response")
        ));

        assert!(output.contains("delegate_status: completed"));
        let records =
            list_records(&tmp.path().join(".data"), Some("parent-session")).expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, "completed");
        assert_eq!(records[0].max_iterations, 2);
        assert_eq!(records[0].attempt, 1);
    }
}
