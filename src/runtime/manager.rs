use anyhow::{Context, Result, anyhow};
#[cfg(unix)]
use std::io;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

use super::{
    RuntimeCommand, RuntimeExecOutcome, RuntimeExecRequest, RuntimeStatus, RuntimeStatusDetail,
};
use crate::runtime_control::stop_requested;
use crate::runtime_profile::RuntimeProfile;
use crate::tools::{ToolContext, emit_tool_stderr, emit_tool_stdout};

const STOP_POLL_MS: u64 = 100;

#[cfg(unix)]
const SIGKILL: i32 = 9;

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn setpgid(pid: i32, pgid: i32) -> i32;
}

pub struct RuntimeManager<'a> {
    ctx: &'a ToolContext,
    profile: RuntimeProfile,
}

impl<'a> RuntimeManager<'a> {
    pub fn new(ctx: &'a ToolContext) -> Self {
        let profile = RuntimeProfile::resolve(&ctx.data_dir, &ctx.workspace_root)
            .unwrap_or_else(|_| RuntimeProfile::fallback(&ctx.workspace_root));
        Self { ctx, profile }
    }

    pub async fn for_context(ctx: &'a ToolContext) -> Result<Self> {
        let manager = Self::new(ctx);
        manager.ensure_ready().await?;
        Ok(manager)
    }

    pub fn profile(&self) -> &RuntimeProfile {
        &self.profile
    }

    pub async fn inspect_status(&self) -> RuntimeStatus {
        RuntimeStatus {
            backend: self.profile.backend,
            profile_id: self.profile.profile_id.clone(),
            profile_slug: self.profile.profile_slug.clone(),
            display_name: self.profile.display_name.clone(),
            workspace_root: self.profile.workspace_root.clone(),
            ready: true,
            last_error: None,
            detail: RuntimeStatusDetail::Local { available: true },
        }
    }

    pub async fn ensure_started(&self) -> Result<()> {
        self.ensure_ready().await
    }

    pub async fn repair(&self) -> Result<()> {
        Ok(())
    }

    pub async fn reset(&self) -> Result<()> {
        Ok(())
    }

    pub async fn execute_request(&self, request: RuntimeExecRequest) -> Result<RuntimeExecOutcome> {
        let mut command = self.build_command(&request).await?;
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        if request.stdin.is_some() {
            command.stdin(Stdio::piped());
        }

        let mut child = command.spawn().with_context(|| match &request.command {
            RuntimeCommand::Shell(command) => {
                format!("failed to execute runtime shell command `{command}`")
            }
            RuntimeCommand::Program { program, .. } => {
                format!(
                    "failed to execute runtime program `{}`",
                    program.to_string_lossy()
                )
            }
        })?;

        if let Some(stdin_bytes) = request.stdin {
            if let Some(mut stdin) = child.stdin.take() {
                tokio::spawn(async move {
                    let _ = stdin.write_all(&stdin_bytes).await;
                    let _ = stdin.shutdown().await;
                });
            }
        }

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to capture runtime stdout"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to capture runtime stderr"))?;

        let stdout_session_id = self.ctx.current_session_id.clone();
        let stderr_session_id = self.ctx.current_session_id.clone();
        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                match stdout.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        buf.extend_from_slice(&chunk[..read]);
                        emit_tool_stdout(
                            &stdout_session_id,
                            String::from_utf8_lossy(&chunk[..read]).to_string(),
                        );
                    }
                }
            }
            buf
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                match stderr.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        buf.extend_from_slice(&chunk[..read]);
                        emit_tool_stderr(
                            &stderr_session_id,
                            String::from_utf8_lossy(&chunk[..read]).to_string(),
                        );
                    }
                }
            }
            buf
        });

        let deadline = request
            .timeout
            .map(|limit| std::time::Instant::now() + limit);
        loop {
            if stop_requested(&self.ctx.data_dir, &self.ctx.current_session_id) {
                terminate_child(&mut child).await;
                return Ok(RuntimeExecOutcome {
                    exit_code: None,
                    timed_out: false,
                    canceled: true,
                    stdout: stdout_task.await.unwrap_or_default(),
                    stderr: stderr_task.await.unwrap_or_default(),
                });
            }

            if let Some(deadline) = deadline {
                let now = std::time::Instant::now();
                if now >= deadline {
                    terminate_child(&mut child).await;
                    return Ok(RuntimeExecOutcome {
                        exit_code: None,
                        timed_out: true,
                        canceled: false,
                        stdout: stdout_task.await.unwrap_or_default(),
                        stderr: stderr_task.await.unwrap_or_default(),
                    });
                }
                let remaining = deadline.saturating_duration_since(now);
                let poll_window = remaining.min(Duration::from_millis(STOP_POLL_MS));
                match timeout(poll_window, child.wait()).await {
                    Ok(status) => {
                        let status = status.context("runtime process wait failed")?;
                        return Ok(RuntimeExecOutcome {
                            exit_code: status.code(),
                            timed_out: false,
                            canceled: false,
                            stdout: stdout_task.await.unwrap_or_default(),
                            stderr: stderr_task.await.unwrap_or_default(),
                        });
                    }
                    Err(_) => continue,
                }
            }

            match timeout(Duration::from_millis(STOP_POLL_MS), child.wait()).await {
                Ok(status) => {
                    let status = status.context("runtime process wait failed")?;
                    return Ok(RuntimeExecOutcome {
                        exit_code: status.code(),
                        timed_out: false,
                        canceled: false,
                        stdout: stdout_task.await.unwrap_or_default(),
                        stderr: stderr_task.await.unwrap_or_default(),
                    });
                }
                Err(_) => continue,
            }
        }
    }

    async fn ensure_ready(&self) -> Result<()> {
        Ok(())
    }

    async fn build_command(&self, request: &RuntimeExecRequest) -> Result<Command> {
        Ok(build_local_command(request))
    }
}

fn build_local_command(request: &RuntimeExecRequest) -> Command {
    match &request.command {
        RuntimeCommand::Shell(command) => {
            let mut process = Command::new("sh");
            process
                .arg("-lc")
                .arg(command)
                .current_dir(&request.workdir);
            prepare_child_process(&mut process);
            process
        }
        RuntimeCommand::Program { program, args } => {
            let mut process = Command::new(program);
            process.args(args).current_dir(&request.workdir);
            prepare_child_process(&mut process);
            process
        }
    }
}

fn prepare_child_process(process: &mut Command) {
    #[cfg(unix)]
    unsafe {
        process.pre_exec(|| {
            if setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        });
    }
}

async fn terminate_child(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        unsafe {
            let _ = kill(-(pid as i32), SIGKILL);
        }
    }

    let _ = child.kill().await;
    let _ = child.wait().await;
}
