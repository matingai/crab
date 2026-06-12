use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::Duration;

use crate::approval::{consume_approved_request, request_approval};
use crate::runtime;
use crate::tools::{Tool, ToolContext, classify_shell_risk, truncated};
use crate::types::{ToolDefinition, object_schema};

pub struct TerminalTool;

#[derive(Debug, Deserialize)]
struct TerminalArgs {
    command: String,
}

#[async_trait]
impl Tool for TerminalTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "terminal",
            "Run a shell command in the workspace when shell access is enabled.",
            object_schema(
                json!({
                    "command": {
                        "type": "string",
                        "description": "Shell command to run from the workspace root."
                    }
                }),
                &["command"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        if !ctx.shell_enabled {
            bail!(
                "terminal tool is disabled; start with --enable-shell or set HERMES_RS_ENABLE_SHELL=1"
            );
        }

        let args: TerminalArgs =
            serde_json::from_value(args).context("invalid terminal arguments")?;
        let command = args.command.trim();
        if command.is_empty() {
            bail!("terminal command cannot be empty");
        }

        if let Some(reason) = classify_shell_risk(command) {
            if consume_approved_request(&ctx.data_dir, &ctx.current_session_id, command)?.is_none()
            {
                let approval =
                    request_approval(&ctx.data_dir, &ctx.current_session_id, command, reason)?;
                return Ok(format!(
                    "approval_required\napproval_id: {}\nsession_id: {}\nreason: {}\ncommand: {}",
                    approval.id, approval.session_id, approval.reason, approval.command
                ));
            }
        }

        let outcome = runtime::execute_shell(
            ctx,
            command,
            &ctx.workspace_root,
            Some(Duration::from_secs(300)),
        )
        .await?;
        let stdout = truncated(String::from_utf8_lossy(&outcome.stdout).to_string(), 12_000);
        let stderr = truncated(String::from_utf8_lossy(&outcome.stderr).to_string(), 8_000);
        let status_line = if outcome.canceled {
            "status: canceled\nexit_code: -1".to_string()
        } else if outcome.timed_out {
            "status: timeout\nexit_code: -1".to_string()
        } else {
            format!(
                "status: completed\nexit_code: {}",
                outcome.exit_code.unwrap_or(-1)
            )
        };

        Ok(format!(
            "{}\nstdout:\n{}\nstderr:\n{}",
            status_line, stdout, stderr
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalTool;
    use crate::approval::{ApprovalStatus, list_requests, resolve_request};
    use crate::runtime_control::request_stop;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn dangerous_commands_require_approval() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = TerminalTool;
        let output = tool
            .execute(
                json!({ "command": "rm -rf build" }),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: tmp.path().join(".data"),
                    shell_enabled: true,
                    skill_platform: "cli".to_string(),
                    provider_id: "openai".to_string(),
                    model: "test-model".to_string(),
                    base_url: "https://example.invalid/v1".to_string(),
                    api_key: None,
                    max_iterations: 4,
                    current_session_id: "test-session".to_string(),
                    current_delegate_run_id: None,
                    delegate_depth: 0,
                },
            )
            .await
            .expect("approval response");

        assert!(output.contains("approval_required"));
        let requests = list_requests(&tmp.path().join(".data")).expect("requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].status, ApprovalStatus::Pending);
    }

    #[tokio::test]
    async fn approved_command_can_run() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("hello.txt"), "hello").expect("write");
        let tool = TerminalTool;
        let ctx = ToolContext {
            workspace_root: tmp.path().to_path_buf(),
            data_dir: tmp.path().join(".data"),
            shell_enabled: true,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            max_iterations: 4,
            current_session_id: "test-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        };

        let first = tool
            .execute(json!({ "command": "rm -rf hello.txt" }), &ctx)
            .await
            .expect("approval response");
        let approval_id = first
            .lines()
            .find_map(|line| line.strip_prefix("approval_id: "))
            .expect("approval id")
            .to_string();
        resolve_request(&ctx.data_dir, &approval_id, true).expect("approve");

        let second = tool
            .execute(json!({ "command": "rm -rf hello.txt" }), &ctx)
            .await
            .expect("command result");
        assert!(second.contains("status: completed"));
        assert!(second.contains("exit_code: 0"));
    }

    #[tokio::test]
    async fn stop_request_cancels_running_command() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = TerminalTool;
        let ctx = ToolContext {
            workspace_root: tmp.path().to_path_buf(),
            data_dir: tmp.path().join(".data"),
            shell_enabled: true,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            max_iterations: 4,
            current_session_id: "test-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        };

        let stop_data_dir = ctx.data_dir.clone();
        let stop_session_id = ctx.current_session_id.clone();
        let stop_task = tokio::spawn(async move {
            sleep(Duration::from_millis(150)).await;
            request_stop(&stop_data_dir, &stop_session_id).expect("request stop");
        });

        let output = tool
            .execute(json!({ "command": "sleep 5" }), &ctx)
            .await
            .expect("command result");
        stop_task.await.expect("stop task");

        assert!(output.contains("status: canceled"));
    }
}
