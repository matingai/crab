use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tokio::time::Duration;
use uuid::Uuid;

use crate::approval::{consume_approved_request, request_approval};
use crate::runtime;
use crate::runtime_profile::RuntimeProfile;
use crate::tools::{
    Tool, ToolContext, classify_shell_risk, relative_display, resolve_existing_path, truncated,
};
use crate::types::{ToolDefinition, object_schema};

pub struct ExecuteCodeTool;

#[derive(Debug, Deserialize)]
struct ExecuteCodeArgs {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    argv: Vec<String>,
    workdir: Option<String>,
    timeout_seconds: Option<u64>,
}

#[derive(Debug)]
struct ExecutionSpec {
    language: &'static str,
    program: &'static str,
    args_prefix: Vec<OsString>,
    extension: &'static str,
}

const DEFAULT_TIMEOUT_SECONDS: u64 = 20;
const MAX_TIMEOUT_SECONDS: u64 = 300;

enum ExecutionSource {
    Inline { script_path: PathBuf },
    ExistingFile { script_path: PathBuf },
}

#[async_trait]
impl Tool for ExecuteCodeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "execute_code",
            "Execute a small code snippet using a supported local interpreter inside the workspace.",
            object_schema(
                json!({
                    "language": {
                        "type": "string",
                        "enum": ["python", "javascript", "bash", "sh"],
                        "description": "Interpreter to use. Optional when `path` points to a supported script extension such as `.py`, `.js`, or `.sh`."
                    },
                    "code": {
                        "type": "string",
                        "description": "Inline code snippet to execute. Provide either `code` or `path`."
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional path to an existing script file relative to the workspace root. Provide either `code` or `path`."
                    },
                    "argv": {
                        "type": "array",
                        "description": "Optional positional arguments passed to the script.",
                        "items": {
                            "type": "string"
                        }
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Optional working directory relative to the workspace root."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 300,
                        "description": "Execution timeout in seconds."
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        if !ctx.shell_enabled {
            bail!(
                "execute_code is disabled; enable shell access first because code execution can run local interpreters"
            );
        }

        let args: ExecuteCodeArgs =
            serde_json::from_value(args).context("invalid execute_code arguments")?;
        if let Some(response) = maybe_request_execution_approval(&args, ctx)? {
            return Ok(response);
        }
        let timeout_seconds = args
            .timeout_seconds
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
            .clamp(1, MAX_TIMEOUT_SECONDS);
        let workdir = resolve_workdir(ctx, args.workdir.as_deref())?;
        let (spec, source) = prepare_execution(&args, ctx)?;
        let script_path = match &source {
            ExecutionSource::Inline { script_path }
            | ExecutionSource::ExistingFile { script_path } => script_path.clone(),
        };

        let outcome = run_script(
            &spec,
            &script_path,
            &args.argv,
            &workdir,
            timeout_seconds,
            ctx,
        )
        .await;
        if let ExecutionSource::Inline { script_path } = &source {
            let _ = std::fs::remove_file(script_path);
        }
        let outcome = outcome?;

        let stdout = truncated(String::from_utf8_lossy(&outcome.stdout).to_string(), 16_000);
        let stderr = truncated(String::from_utf8_lossy(&outcome.stderr).to_string(), 8_000);
        let source_label = match &source {
            ExecutionSource::Inline { .. } => "inline",
            ExecutionSource::ExistingFile { .. } => "file",
        };
        let status_line = if outcome.canceled {
            "status: canceled\nexit_code: -1".to_string()
        } else if outcome.timed_out {
            format!("status: timeout\ntimeout_seconds: {timeout_seconds}")
        } else {
            format!(
                "status: completed\nexit_code: {}",
                outcome.exit_code.unwrap_or(-1)
            )
        };

        Ok(format!(
            "language: {}\nprogram: {}\nsource: {}\nworkdir: {}\nscript_path: {}\nargv: {:?}\n{}\nstdout:\n{}\nstderr:\n{}",
            spec.language,
            spec.program,
            source_label,
            relative_display(&ctx.workspace_root, &workdir),
            relative_display(&ctx.workspace_root, &script_path),
            args.argv,
            status_line,
            stdout,
            stderr
        ))
    }
}

fn maybe_request_execution_approval(
    args: &ExecuteCodeArgs,
    ctx: &ToolContext,
) -> Result<Option<String>> {
    let Some((approval_command, reason)) = classify_execution_risk(args, ctx)? else {
        return Ok(None);
    };
    if consume_approved_request(&ctx.data_dir, &ctx.current_session_id, &approval_command)?
        .is_some()
    {
        return Ok(None);
    }

    let approval = request_approval(
        &ctx.data_dir,
        &ctx.current_session_id,
        &approval_command,
        reason,
    )?;
    Ok(Some(format!(
        "approval_required\napproval_id: {}\nsession_id: {}\nreason: {}\ncommand: {}",
        approval.id, approval.session_id, approval.reason, approval.command
    )))
}

fn classify_execution_risk(
    args: &ExecuteCodeArgs,
    ctx: &ToolContext,
) -> Result<Option<(String, &'static str)>> {
    let code = args
        .code
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let path = args
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match (code, path) {
        (Some(_), Some(_)) | (None, None) => Ok(None),
        (Some(code), None) => {
            let Some(language) = args
                .language
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            else {
                return Ok(None);
            };
            let spec = execution_spec(language)?;
            let Some(reason) = classify_shell_risk(code) else {
                return Ok(None);
            };
            Ok(Some((
                format!(
                    "execute_code {} inline: {}",
                    spec.language,
                    one_line_preview(code, 320)
                ),
                reason,
            )))
        }
        (None, Some(path)) => {
            let script_path = resolve_existing_path(&ctx.workspace_root, path)?;
            if !script_path.is_file() {
                return Ok(None);
            }
            let spec = match args
                .language
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                Some(language) => execution_spec(language)?,
                None => execution_spec_from_path(&script_path)?,
            };
            let Ok(script) = std::fs::read_to_string(&script_path) else {
                return Ok(None);
            };
            let Some(reason) = classify_shell_risk(&script) else {
                return Ok(None);
            };
            Ok(Some((
                format!(
                    "execute_code {} file {}: {}",
                    spec.language,
                    relative_display(&ctx.workspace_root, &script_path),
                    one_line_preview(&script, 320)
                ),
                reason,
            )))
        }
    }
}

fn one_line_preview(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let mut clipped = collapsed
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    clipped.push_str("...");
    clipped
}

fn execution_spec(language: &str) -> Result<ExecutionSpec> {
    let normalized = language.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "python" | "python3" => Ok(ExecutionSpec {
            language: "python",
            program: "python3",
            args_prefix: Vec::new(),
            extension: "py",
        }),
        "javascript" | "js" | "node" => Ok(ExecutionSpec {
            language: "javascript",
            program: "node",
            args_prefix: Vec::new(),
            extension: "js",
        }),
        "bash" => Ok(ExecutionSpec {
            language: "bash",
            program: "bash",
            args_prefix: Vec::new(),
            extension: "sh",
        }),
        "sh" | "shell" => Ok(ExecutionSpec {
            language: "sh",
            program: "sh",
            args_prefix: Vec::new(),
            extension: "sh",
        }),
        other => bail!("unsupported execute_code language `{other}`"),
    }
}

fn execution_spec_from_path(path: &Path) -> Result<ExecutionSpec> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "py" => execution_spec("python"),
        "js" | "mjs" | "cjs" => execution_spec("javascript"),
        "sh" | "bash" => execution_spec("bash"),
        other if !other.is_empty() => {
            bail!(
                "unsupported execute_code script extension `.{other}`; pass `language` explicitly or use a supported script file"
            )
        }
        _ => bail!(
            "execute_code could not infer a language from the script path; pass `language` explicitly"
        ),
    }
}

fn prepare_execution(
    args: &ExecuteCodeArgs,
    ctx: &ToolContext,
) -> Result<(ExecutionSpec, ExecutionSource)> {
    let code = args
        .code
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let path = args
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match (code, path) {
        (Some(_), Some(_)) => bail!("execute_code accepts either `code` or `path`, not both"),
        (None, None) => bail!("execute_code requires either non-empty `code` or `path`"),
        (Some(code), None) => {
            let language = args
                .language
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    anyhow!("execute_code requires `language` when using inline `code`")
                })?;
            let spec = execution_spec(language)?;
            let runtime_root = execute_code_root(ctx);
            std::fs::create_dir_all(&runtime_root)
                .with_context(|| format!("failed to create {}", runtime_root.display()))?;
            let script_path = runtime_root.join(format!(
                "{}-{}.{}",
                ctx.current_session_id,
                Uuid::new_v4().simple(),
                spec.extension
            ));
            std::fs::write(&script_path, code)
                .with_context(|| format!("failed to write {}", script_path.display()))?;
            Ok((spec, ExecutionSource::Inline { script_path }))
        }
        (None, Some(path)) => {
            let script_path = resolve_existing_path(&ctx.workspace_root, path)?;
            if !script_path.is_file() {
                bail!(
                    "execute_code path must point to a file: {}",
                    script_path.display()
                );
            }
            let spec = match args
                .language
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                Some(language) => execution_spec(language)?,
                None => execution_spec_from_path(&script_path)?,
            };
            Ok((spec, ExecutionSource::ExistingFile { script_path }))
        }
    }
}

fn resolve_workdir(ctx: &ToolContext, workdir: Option<&str>) -> Result<PathBuf> {
    match workdir {
        Some(value) if !value.trim().is_empty() => {
            let path = resolve_existing_path(&ctx.workspace_root, value)?;
            if !path.is_dir() {
                bail!(
                    "execute_code workdir must be a directory: {}",
                    path.display()
                );
            }
            Ok(path)
        }
        _ => Ok(ctx.workspace_root.clone()),
    }
}

async fn run_script(
    spec: &ExecutionSpec,
    script_path: &Path,
    argv: &[String],
    workdir: &Path,
    timeout_seconds: u64,
    ctx: &ToolContext,
) -> Result<runtime::RuntimeExecOutcome> {
    let profile = RuntimeProfile::resolve(&ctx.data_dir, &ctx.workspace_root)
        .unwrap_or_else(|_| RuntimeProfile::fallback(&ctx.workspace_root));
    let runtime_script_path =
        runtime::map_path_for_runtime(&profile, &ctx.workspace_root, script_path);
    let mut args = spec.args_prefix.clone();
    args.push(runtime_script_path.into_os_string());
    args.extend(argv.iter().map(OsString::from));
    runtime::execute_program(
        ctx,
        spec.program,
        args,
        workdir,
        None,
        Some(Duration::from_secs(timeout_seconds)),
    )
    .await
}

fn execute_code_root(ctx: &ToolContext) -> PathBuf {
    if ctx.data_dir.starts_with(&ctx.workspace_root) {
        return ctx.data_dir.join("runtime").join("execute-code");
    }
    ctx.workspace_root
        .join(".hermes-agent-rs")
        .join("runtime")
        .join("execute-code")
}

#[cfg(test)]
mod tests {
    use super::ExecuteCodeTool;
    use crate::approval::{ApprovalStatus, list_requests, resolve_request};
    use crate::runtime_control::request_stop;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;
    use std::fs;
    use tokio::time::{Duration, sleep};

    fn ctx(root: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace_root: root.to_path_buf(),
            data_dir: root.join(".data"),
            shell_enabled: true,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            api_mode: crate::llm::ApiMode::ChatCompletions,
            worker_model: None,
            max_iterations: 4,
            current_session_id: "test-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[tokio::test]
    async fn executes_bash_snippet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ExecuteCodeTool;
        let output = tool
            .execute(
                json!({
                    "language": "bash",
                    "code": "printf 'hello from bash'"
                }),
                &ctx(tmp.path()),
            )
            .await
            .expect("execute");

        assert!(output.contains("status: completed"));
        assert!(output.contains("hello from bash"));
    }

    #[tokio::test]
    async fn respects_workdir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("nested")).expect("mkdir");
        let tool = ExecuteCodeTool;
        let output = tool
            .execute(
                json!({
                    "language": "bash",
                    "workdir": "nested",
                    "code": "pwd"
                }),
                &ctx(tmp.path()),
            )
            .await
            .expect("execute");

        assert!(output.contains("workdir: nested"));
        assert!(output.contains("/nested"));
    }

    #[tokio::test]
    async fn times_out_long_running_code() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ExecuteCodeTool;
        let output = tool
            .execute(
                json!({
                    "language": "bash",
                    "timeout_seconds": 1,
                    "code": "sleep 2"
                }),
                &ctx(tmp.path()),
            )
            .await
            .expect("execute");

        assert!(output.contains("status: timeout"));
    }

    #[tokio::test]
    async fn dangerous_inline_code_requires_approval_before_execution() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let victim = tmp.path().join("victim.txt");
        fs::write(&victim, "keep me").expect("write victim");
        let tool = ExecuteCodeTool;
        let output = tool
            .execute(
                json!({
                    "language": "bash",
                    "code": "rm -rf victim.txt"
                }),
                &ctx(tmp.path()),
            )
            .await
            .expect("approval response");

        assert!(output.contains("approval_required"));
        assert!(output.contains("destructive file deletion"));
        assert!(
            victim.exists(),
            "dangerous code must not execute before approval"
        );
        assert!(
            !tmp.path()
                .join(".data")
                .join("runtime")
                .join("execute-code")
                .exists(),
            "inline script should not be materialized before approval"
        );
        let requests = list_requests(&tmp.path().join(".data")).expect("requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].status, ApprovalStatus::Pending);
        assert!(requests[0].command.contains("execute_code bash inline"));
    }

    #[tokio::test]
    async fn approved_dangerous_inline_code_can_run_once() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let victim = tmp.path().join("victim.txt");
        fs::write(&victim, "remove me").expect("write victim");
        let tool = ExecuteCodeTool;
        let ctx = ctx(tmp.path());
        let first = tool
            .execute(
                json!({
                    "language": "bash",
                    "code": "rm -rf victim.txt"
                }),
                &ctx,
            )
            .await
            .expect("approval response");
        let approval_id = first
            .lines()
            .find_map(|line| line.strip_prefix("approval_id: "))
            .expect("approval id")
            .to_string();
        resolve_request(&ctx.data_dir, &approval_id, true).expect("approve");

        let second = tool
            .execute(
                json!({
                    "language": "bash",
                    "code": "rm -rf victim.txt"
                }),
                &ctx,
            )
            .await
            .expect("execution result");

        assert!(second.contains("status: completed"));
        assert!(!victim.exists());
        let requests = list_requests(&ctx.data_dir).expect("requests");
        assert_eq!(requests[0].status, ApprovalStatus::Consumed);
    }

    #[tokio::test]
    async fn stop_request_cancels_running_code() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ExecuteCodeTool;
        let ctx = ctx(tmp.path());

        let stop_data_dir = ctx.data_dir.clone();
        let stop_session_id = ctx.current_session_id.clone();
        let stop_task = tokio::spawn(async move {
            sleep(Duration::from_millis(150)).await;
            request_stop(&stop_data_dir, &stop_session_id).expect("request stop");
        });

        let output = tool
            .execute(
                json!({
                    "language": "bash",
                    "timeout_seconds": 30,
                    "code": "sleep 5"
                }),
                &ctx,
            )
            .await
            .expect("execute");
        stop_task.await.expect("stop task");

        assert!(output.contains("status: canceled"));
    }

    #[tokio::test]
    async fn rejects_unsupported_language() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ExecuteCodeTool;
        let error = tool
            .execute(
                json!({
                    "language": "ruby",
                    "code": "puts 'hi'"
                }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("unsupported");

        assert!(
            error
                .to_string()
                .contains("unsupported execute_code language")
        );
    }

    #[tokio::test]
    async fn executes_existing_python_script_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("scripts")).expect("mkdir");
        fs::write(
            tmp.path().join("scripts/hello.py"),
            "import sys\nprint('hello', sys.argv[1])\n",
        )
        .expect("write script");
        let tool = ExecuteCodeTool;
        let output = tool
            .execute(
                json!({
                    "path": "scripts/hello.py",
                    "argv": ["Ada"]
                }),
                &ctx(tmp.path()),
            )
            .await
            .expect("execute");

        assert!(output.contains("source: file"));
        assert!(output.contains("script_path: scripts/hello.py"));
        assert!(output.contains("hello Ada"));
    }

    #[tokio::test]
    async fn rejects_missing_code_and_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ExecuteCodeTool;
        let error = tool
            .execute(
                json!({
                    "language": "python"
                }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("missing source");

        assert!(
            error
                .to_string()
                .contains("requires either non-empty `code` or `path`")
        );
    }
}
