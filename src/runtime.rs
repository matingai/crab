mod manager;

use anyhow::Result;
use serde::Serialize;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use tokio::time::Duration;

use crate::runtime_profile::{RuntimeBackend, RuntimeProfile};
use crate::tools::ToolContext;

pub use manager::RuntimeManager;

#[derive(Debug, Clone)]
pub enum RuntimeCommand {
    Shell(String),
    Program {
        program: OsString,
        args: Vec<OsString>,
    },
}

#[derive(Debug, Clone)]
pub struct RuntimeExecRequest {
    pub command: RuntimeCommand,
    pub workdir: PathBuf,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
}

#[derive(Debug)]
pub struct RuntimeExecOutcome {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub canceled: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub backend: RuntimeBackend,
    pub profile_id: String,
    pub profile_slug: String,
    pub display_name: String,
    pub workspace_root: PathBuf,
    pub ready: bool,
    pub last_error: Option<String>,
    pub detail: RuntimeStatusDetail,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeStatusDetail {
    Local { available: bool },
}

pub async fn execute_shell(
    ctx: &ToolContext,
    command: &str,
    workdir: &Path,
    timeout_limit: Option<Duration>,
) -> Result<RuntimeExecOutcome> {
    RuntimeManager::for_context(ctx)
        .await?
        .execute_request(RuntimeExecRequest {
            command: RuntimeCommand::Shell(command.to_string()),
            workdir: workdir.to_path_buf(),
            stdin: None,
            timeout: timeout_limit,
        })
        .await
}

pub async fn execute_program(
    ctx: &ToolContext,
    program: impl AsRef<OsStr>,
    args: Vec<OsString>,
    workdir: &Path,
    stdin: Option<Vec<u8>>,
    timeout_limit: Option<Duration>,
) -> Result<RuntimeExecOutcome> {
    RuntimeManager::for_context(ctx)
        .await?
        .execute_request(RuntimeExecRequest {
            command: RuntimeCommand::Program {
                program: program.as_ref().to_os_string(),
                args,
            },
            workdir: workdir.to_path_buf(),
            stdin,
            timeout: timeout_limit,
        })
        .await
}

pub async fn ensure_runtime_ready(ctx: &ToolContext) -> Result<RuntimeProfile> {
    let manager = RuntimeManager::for_context(ctx).await?;
    Ok(manager.profile().clone())
}

pub async fn inspect_runtime(ctx: &ToolContext) -> Result<RuntimeStatus> {
    let manager = RuntimeManager::new(ctx);
    Ok(manager.inspect_status().await)
}

pub async fn start_runtime(ctx: &ToolContext) -> Result<RuntimeStatus> {
    let manager = RuntimeManager::new(ctx);
    manager.ensure_started().await?;
    Ok(manager.inspect_status().await)
}

pub async fn repair_runtime(ctx: &ToolContext) -> Result<RuntimeStatus> {
    let manager = RuntimeManager::new(ctx);
    manager.repair().await?;
    Ok(manager.inspect_status().await)
}

pub async fn reset_runtime(ctx: &ToolContext) -> Result<RuntimeStatus> {
    let manager = RuntimeManager::new(ctx);
    manager.reset().await?;
    Ok(manager.inspect_status().await)
}

pub fn map_path_for_runtime(
    profile: &RuntimeProfile,
    workspace_root: &Path,
    path: &Path,
) -> PathBuf {
    let _ = profile;
    let _ = workspace_root;
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
