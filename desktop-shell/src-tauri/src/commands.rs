use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use hermes_agent_rs::browser_state::BrowserStateStore;
use hermes_agent_rs::runtime;
use hermes_agent_rs::{
    AgentBridge, ApprovalRequest, ApprovalStatus, BridgeDelegateRun, BridgeProviderRuntimeStatus,
    BridgeRunResult, BridgeSessionDetail, BridgeSessionSearchResult, BridgeSessionSummary,
    BridgeSkillDetail, BridgeSkillSummary, ExtensionsOverview, McpServerInspection,
    ProviderSummary, ResolveProviderStatusRequest, RetryDelegateRunRequest, RunAgentRequest,
    RunCronJobRequest, RuntimeProfile, RuntimeStatus, SaveCronJobRequest, SessionCommandRequest,
    SharedAgentConfig, SharedProviderConfigRequest, SimpleSessionResponse,
    TAURI_AGENT_CLEARED_EVENT, TAURI_AGENT_DONE_EVENT, TAURI_AGENT_EVENT, TauriEmitter,
    TauriEventBridge,
    browser_backend::{
        agent_browser_stream_port, agent_browser_stream_relay_port,
        agent_browser_target_for_session,
    },
    office, office_render, pdf, tauri_session_done_event_name, tauri_session_event_name,
};
use serde::Serialize;
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::time::{Duration, sleep};

#[derive(Debug, Default)]
pub struct DesktopState {
    last_session_id: Mutex<Option<String>>,
    cron_scheduler: Mutex<CronSchedulerRuntime>,
    interactive_runs: AtomicUsize,
}

#[derive(Debug, Default)]
struct CronSchedulerRuntime {
    stop_flag: Option<Arc<AtomicBool>>,
    status: CronSchedulerStatus,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CronSchedulerStatus {
    pub running: bool,
    pub paused_reason: Option<String>,
    pub tick_interval_seconds: u64,
    pub last_tick_at_unix: Option<u64>,
    pub last_due_job_ids: Vec<String>,
    pub last_error: Option<String>,
    pub workspace_root: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronSchedulerRequest {
    pub workspace_root: PathBuf,
    pub data_dir: Option<PathBuf>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub aux_provider: Option<String>,
    pub aux_model: Option<String>,
    pub aux_base_url: Option<String>,
    pub aux_api_key: Option<String>,
    pub max_iterations: Option<usize>,
    pub system_prompt_override: Option<String>,
    pub enable_shell_tool: bool,
    pub tick_interval_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct DesktopInfo {
    pub shell: String,
    pub platform: String,
    pub global_event_topic: String,
    pub global_done_topic: String,
    pub cleared_topic: String,
    pub session_event_topic_template: String,
    pub session_done_topic_template: String,
    pub last_session_id: Option<String>,
    pub current_working_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilePreview {
    pub path: String,
    pub display_path: String,
    pub file_name: String,
    pub file_type: String,
    pub mime_type: String,
    pub kind: String,
    pub size_bytes: u64,
    pub content: Option<String>,
    pub source_url: Option<String>,
    pub is_binary: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTreeNode {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub children: Vec<WorkspaceTreeNode>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTreeResponse {
    pub root_path: String,
    pub nodes: Vec<WorkspaceTreeNode>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStreamEndpoint {
    pub ws_url: String,
    pub port: u16,
    pub session_name: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileRequest {
    pub workspace_root: PathBuf,
    pub data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenWorkspaceFileRequest {
    pub workspace_root: PathBuf,
    pub file_path: String,
}

#[derive(Clone)]
struct AppHandleEmitter<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> AppHandleEmitter<R> {
    fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> TauriEmitter for AppHandleEmitter<R> {
    fn emit_json(&self, event_name: &str, payload: Value) -> Result<()> {
        self.app
            .emit(event_name, payload)
            .map_err(|error| anyhow!(error.to_string()))
    }
}

#[tauri::command]
pub fn desktop_info(state: State<'_, DesktopState>) -> Result<DesktopInfo, String> {
    let last_session_id = state
        .last_session_id
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let current_working_dir = env::current_dir()
        .map_err(format_error)?
        .display()
        .to_string();
    Ok(DesktopInfo {
        shell: "tauri".to_string(),
        platform: env::consts::OS.to_string(),
        global_event_topic: TAURI_AGENT_EVENT.to_string(),
        global_done_topic: TAURI_AGENT_DONE_EVENT.to_string(),
        cleared_topic: TAURI_AGENT_CLEARED_EVENT.to_string(),
        session_event_topic_template: tauri_session_event_name("<session_id>"),
        session_done_topic_template: tauri_session_done_event_name("<session_id>"),
        last_session_id,
        current_working_dir,
    })
}

#[tauri::command]
pub fn resolve_runtime_profile(request: RuntimeProfileRequest) -> Result<RuntimeProfile, String> {
    let data_dir = request
        .data_dir
        .clone()
        .unwrap_or_else(|| request.workspace_root.join(".hermes-agent-rs"));
    RuntimeProfile::resolve(&data_dir, &request.workspace_root).map_err(format_error)
}

#[tauri::command]
pub fn open_workspace_file(request: OpenWorkspaceFileRequest) -> Result<Value, String> {
    let workspace_root = request
        .workspace_root
        .canonicalize()
        .map_err(|error| format_error(anyhow!("failed to resolve workspace root: {error}")))?;
    let target = resolve_workspace_file(&workspace_root, &request.file_path).map_err(format_error)?;
    open_path_in_system_app(&target).map_err(format_error)?;
    Ok(serde_json::json!({
        "ok": true,
        "path": target.display().to_string(),
    }))
}

#[tauri::command]
pub async fn resolve_runtime_status(
    request: RuntimeProfileRequest,
) -> Result<RuntimeStatus, String> {
    let ctx = runtime_status_context(request);
    hermes_agent_rs::runtime::inspect_runtime(&ctx)
        .await
        .map_err(format_error)
}

#[tauri::command]
pub async fn start_runtime(request: RuntimeProfileRequest) -> Result<RuntimeStatus, String> {
    let ctx = runtime_status_context(request);
    hermes_agent_rs::runtime::start_runtime(&ctx)
        .await
        .map_err(format_error)
}

#[tauri::command]
pub async fn repair_runtime(request: RuntimeProfileRequest) -> Result<RuntimeStatus, String> {
    let ctx = runtime_status_context(request);
    hermes_agent_rs::runtime::repair_runtime(&ctx)
        .await
        .map_err(format_error)
}

#[tauri::command]
pub async fn reset_runtime(request: RuntimeProfileRequest) -> Result<RuntimeStatus, String> {
    let ctx = runtime_status_context(request);
    hermes_agent_rs::runtime::reset_runtime(&ctx)
        .await
        .map_err(format_error)
}

#[tauri::command]
pub async fn run_agent<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, DesktopState>,
    request: RunAgentRequest,
) -> Result<BridgeRunResult, String> {
    state.interactive_runs.fetch_add(1, Ordering::Relaxed);
    let bridge = TauriEventBridge::new(AppHandleEmitter::new(app));
    let result = bridge.run_agent(request).await;
    state.interactive_runs.fetch_sub(1, Ordering::Relaxed);
    let result = result.map_err(format_error)?;
    *state
        .last_session_id
        .lock()
        .map_err(|error| error.to_string())? = Some(result.session_id.clone());
    Ok(result)
}

#[tauri::command]
pub fn clear_session<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, DesktopState>,
    request: SessionCommandRequest,
) -> Result<SimpleSessionResponse, String> {
    let bridge = TauriEventBridge::new(AppHandleEmitter::new(app));
    let response = bridge.clear_session(request).map_err(format_error)?;
    *state
        .last_session_id
        .lock()
        .map_err(|error| error.to_string())? = Some(response.session_id.clone());
    Ok(response)
}

#[tauri::command]
pub async fn resume_approval<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, DesktopState>,
    request: SessionCommandRequest,
    approval_id: String,
) -> Result<BridgeRunResult, String> {
    state.interactive_runs.fetch_add(1, Ordering::Relaxed);
    let bridge = TauriEventBridge::new(AppHandleEmitter::new(app));
    let result = bridge.resume_approval(request, approval_id).await;
    state.interactive_runs.fetch_sub(1, Ordering::Relaxed);
    let result = result.map_err(format_error)?;
    *state
        .last_session_id
        .lock()
        .map_err(|error| error.to_string())? = Some(result.session_id.clone());
    Ok(result)
}

#[tauri::command]
pub async fn run_cron_job<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, DesktopState>,
    request: RunCronJobRequest,
) -> Result<BridgeRunResult, String> {
    let bridge = TauriEventBridge::new(AppHandleEmitter::new(app));
    let result = bridge.run_cron_job(request).await.map_err(format_error)?;
    *state
        .last_session_id
        .lock()
        .map_err(|error| error.to_string())? = Some(result.session_id.clone());
    Ok(result)
}

#[tauri::command]
pub fn cron_scheduler_status(
    state: State<'_, DesktopState>,
) -> Result<CronSchedulerStatus, String> {
    Ok(state
        .cron_scheduler
        .lock()
        .map_err(|error| error.to_string())?
        .status
        .clone())
}

#[tauri::command]
pub fn stop_cron_scheduler(state: State<'_, DesktopState>) -> Result<CronSchedulerStatus, String> {
    let mut scheduler = state
        .cron_scheduler
        .lock()
        .map_err(|error| error.to_string())?;
    if let Some(stop_flag) = scheduler.stop_flag.take() {
        stop_flag.store(true, Ordering::Relaxed);
    }
    scheduler.status.running = false;
    Ok(scheduler.status.clone())
}

#[tauri::command]
pub fn start_cron_scheduler<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, DesktopState>,
    request: CronSchedulerRequest,
) -> Result<CronSchedulerStatus, String> {
    let tick_interval_seconds = request.tick_interval_seconds.unwrap_or(60).max(15);
    let stop_flag = Arc::new(AtomicBool::new(false));
    {
        let mut scheduler = state
            .cron_scheduler
            .lock()
            .map_err(|error| error.to_string())?;
        if let Some(existing) = scheduler.stop_flag.take() {
            existing.store(true, Ordering::Relaxed);
        }
        scheduler.stop_flag = Some(stop_flag.clone());
        scheduler.status = CronSchedulerStatus {
            running: true,
            paused_reason: None,
            tick_interval_seconds,
            last_tick_at_unix: None,
            last_due_job_ids: Vec::new(),
            last_error: None,
            workspace_root: Some(request.workspace_root.display().to_string()),
        };
    }

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if stop_flag.load(Ordering::Relaxed) {
                update_scheduler_state(&app_handle, |status| {
                    status.running = false;
                });
                break;
            }

            let data_dir = request
                .data_dir
                .clone()
                .unwrap_or_else(|| request.workspace_root.join(".hermes-agent-rs"));
            let now = unix_now();
            if let Some(reason) = current_scheduler_pause_reason(&app_handle, &data_dir) {
                update_scheduler_state(&app_handle, |status| {
                    status.last_tick_at_unix = Some(now);
                    status.last_due_job_ids.clear();
                    status.last_error = None;
                    status.paused_reason = Some(reason.clone());
                });
                sleep(Duration::from_secs(tick_interval_seconds)).await;
                continue;
            }
            match hermes_agent_rs::cron::list_due_jobs(&data_dir, now) {
                Ok(due_jobs) => {
                    let due_ids = due_jobs
                        .iter()
                        .map(|job| job.id.clone())
                        .collect::<Vec<_>>();
                    update_scheduler_state(&app_handle, |status| {
                        status.last_tick_at_unix = Some(now);
                        status.last_due_job_ids = due_ids.clone();
                        status.last_error = None;
                        status.paused_reason = None;
                    });
                    for job in due_jobs {
                        if stop_flag.load(Ordering::Relaxed) {
                            break;
                        }
                        let bridge =
                            TauriEventBridge::new(AppHandleEmitter::new(app_handle.clone()));
                        let run_request = RunCronJobRequest {
                            workspace_root: request.workspace_root.clone(),
                            data_dir: request.data_dir.clone(),
                            job_id: job.id,
                            provider: request.provider.clone(),
                            model: request.model.clone(),
                            base_url: request.base_url.clone(),
                            api_key: request.api_key.clone(),
                            aux_provider: request.aux_provider.clone(),
                            aux_model: request.aux_model.clone(),
                            aux_base_url: request.aux_base_url.clone(),
                            aux_api_key: request.aux_api_key.clone(),
                            max_iterations: request.max_iterations,
                            system_prompt_override: request.system_prompt_override.clone(),
                            enable_shell_tool: request.enable_shell_tool,
                        };
                        match bridge.run_cron_job(run_request).await {
                            Ok(result) => update_last_session_id(&app_handle, result.session_id),
                            Err(error) => {
                                update_scheduler_state(&app_handle, |status| {
                                    status.last_error = Some(error.to_string());
                                });
                            }
                        }
                    }
                }
                Err(error) => {
                    update_scheduler_state(&app_handle, |status| {
                        status.last_tick_at_unix = Some(now);
                        status.last_error = Some(error.to_string());
                        status.paused_reason = None;
                    });
                }
            }

            sleep(Duration::from_secs(tick_interval_seconds)).await;
        }
    });

    cron_scheduler_status(state)
}

#[tauri::command]
pub fn stop_session(
    data_dir: PathBuf,
    session_id: String,
) -> Result<SimpleSessionResponse, String> {
    AgentBridge::stop_session(data_dir, session_id).map_err(format_error)
}

#[tauri::command]
pub fn list_approvals(data_dir: PathBuf) -> Result<Vec<ApprovalRequest>, String> {
    AgentBridge::list_approvals(data_dir).map_err(format_error)
}

#[tauri::command]
pub fn resolve_approval(
    data_dir: PathBuf,
    approval_id: String,
    approved: bool,
) -> Result<ApprovalRequest, String> {
    AgentBridge::resolve_approval(data_dir, approval_id, approved).map_err(format_error)
}

#[tauri::command]
pub fn list_skills(data_dir: PathBuf) -> Result<Vec<BridgeSkillSummary>, String> {
    AgentBridge::list_skills(data_dir).map_err(format_error)
}

#[tauri::command]
pub fn view_skill(
    data_dir: PathBuf,
    name: String,
    category: Option<String>,
    file_path: Option<String>,
) -> Result<BridgeSkillDetail, String> {
    AgentBridge::view_skill(data_dir, name, category, file_path).map_err(format_error)
}

#[tauri::command]
pub async fn view_workspace_file(
    workspace_root: PathBuf,
    data_dir: Option<PathBuf>,
    file_path: String,
) -> Result<WorkspaceFilePreview, String> {
    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|error| format_error(anyhow!("failed to resolve workspace root: {error}")))?;
    let data_dir = data_dir.unwrap_or_else(|| workspace_root.join(".hermes-agent-rs"));
    let target = resolve_workspace_file(&workspace_root, &file_path).map_err(format_error)?;
    let metadata = fs::metadata(&target)
        .map_err(|error| format_error(anyhow!("failed to stat {}: {error}", target.display())))?;
    let (kind, mime_type) = workspace_file_kind_and_mime(&target);
    let file_type = target
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let file_name = target
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| target.display().to_string());
    let display_path = target
        .strip_prefix(&workspace_root)
        .unwrap_or(&target)
        .display()
        .to_string();
    let size_bytes = metadata.len();
    let original_path = target.display().to_string();
    let render_ctx = hermes_agent_rs::tools::ToolContext {
        workspace_root: workspace_root.clone(),
        data_dir,
        shell_enabled: false,
        skill_platform: "desktop-file-preview".to_string(),
        provider_id: "desktop".to_string(),
        model: "workspace-preview".to_string(),
        base_url: String::new(),
        api_key: None,
        max_iterations: 1,
        current_session_id: format!("workspace-preview:{}", display_path),
        current_delegate_run_id: None,
        delegate_depth: 0,
    };
    if kind == "spreadsheet" {
        let rendered = office_render::render_pdf_via_runtime(&render_ctx, &target)
            .await
            .map_err(format_error)?;
        return workspace_pdf_preview(
            &original_path,
            &display_path,
            &file_name,
            &file_type,
            size_bytes,
            &rendered.output_path,
        );
    }
    if kind == "document" {
        let preview = office::preview_docx(&target, 24).ok();
        let preview_truncated = preview
            .as_ref()
            .and_then(|value| value.get("truncated"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let bytes = fs::read(&target).map_err(|error| {
            format_error(anyhow!("failed to read {}: {error}", target.display()))
        })?;
        let source_url = if bytes.len() > MAX_OFFICE_INLINE_PREVIEW_BYTES {
            None
        } else {
            Some(format!(
                "data:{};base64,{}",
                mime_type,
                BASE64_STANDARD.encode(&bytes)
            ))
        };
        return Ok(WorkspaceFilePreview {
            path: original_path.clone(),
            display_path,
            file_name,
            file_type,
            mime_type,
            kind,
            size_bytes,
            content: preview
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(format_error)?,
            source_url,
            is_binary: true,
            truncated: preview_truncated || bytes.len() > MAX_OFFICE_INLINE_PREVIEW_BYTES,
        });
    }
    if kind == "presentation" {
        let rendered = office_render::render_pdf_via_runtime(&render_ctx, &target)
            .await
            .map_err(format_error)?;
        return workspace_pdf_preview(
            &original_path,
            &display_path,
            &file_name,
            &file_type,
            size_bytes,
            &rendered.output_path,
        );
    }
    if kind == "pdf" {
        let bytes = fs::read(&target).map_err(|error| {
            format_error(anyhow!("failed to read {}: {error}", target.display()))
        })?;
        let preview = pdf::preview_pdf(&target, 8, 1_200).ok();
        let preview_truncated = preview
            .as_ref()
            .and_then(|value| value.get("truncated"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let source_url = if bytes.len() > MAX_BINARY_PREVIEW_BYTES {
            None
        } else {
            Some(format!(
                "data:{};base64,{}",
                mime_type,
                BASE64_STANDARD.encode(&bytes)
            ))
        };
        return Ok(WorkspaceFilePreview {
            path: original_path,
            display_path,
            file_name,
            file_type,
            mime_type,
            kind,
            size_bytes,
            content: preview
                .as_ref()
                .map(|value| serde_json::to_string(value))
                .transpose()
                .map_err(format_error)?,
            source_url,
            is_binary: true,
            truncated: preview_truncated || bytes.len() > MAX_BINARY_PREVIEW_BYTES,
        });
    }
    let bytes = fs::read(&target)
        .map_err(|error| format_error(anyhow!("failed to read {}: {error}", target.display())))?;
    let mut content = None;
    let mut source_url = None;
    let mut is_binary = false;
    let mut truncated = false;

    match kind.as_str() {
        "image" | "pdf" | "audio" | "video" => {
            if bytes.len() > MAX_BINARY_PREVIEW_BYTES {
                truncated = true;
                content = Some(format!("文件过大，暂不内嵌预览（{} bytes）", size_bytes));
            } else {
                source_url = Some(format!(
                    "data:{};base64,{}",
                    mime_type,
                    BASE64_STANDARD.encode(&bytes)
                ));
            }
            is_binary = true;
        }
        _ => match String::from_utf8(bytes) {
            Ok(text) => {
                if text.len() > MAX_TEXT_PREVIEW_BYTES {
                    truncated = true;
                    content = Some(
                        text.chars()
                            .take(MAX_TEXT_PREVIEW_BYTES)
                            .collect::<String>(),
                    );
                } else {
                    content = Some(text);
                }
            }
            Err(error) => {
                is_binary = true;
                let bytes = error.into_bytes();
                if bytes.len() <= MAX_BINARY_PREVIEW_BYTES
                    && matches!(kind.as_str(), "image" | "pdf" | "audio" | "video")
                {
                    source_url = Some(format!(
                        "data:{};base64,{}",
                        mime_type,
                        BASE64_STANDARD.encode(&bytes)
                    ));
                } else {
                    if bytes.len() > MAX_BINARY_PREVIEW_BYTES {
                        truncated = true;
                    }
                    content = Some(format!(
                        "[Binary file: {}, size: {} bytes]",
                        file_name, size_bytes
                    ));
                }
            }
        },
    }

    Ok(WorkspaceFilePreview {
        path: target.display().to_string(),
        display_path,
        file_name,
        file_type,
        mime_type,
        kind,
        size_bytes,
        content,
        source_url,
        is_binary,
        truncated,
    })
}

fn workspace_pdf_preview(
    original_path: &str,
    display_path: &str,
    file_name: &str,
    file_type: &str,
    size_bytes: u64,
    pdf_path: &Path,
) -> Result<WorkspaceFilePreview, String> {
    let bytes = fs::read(pdf_path)
        .map_err(|error| format_error(anyhow!("failed to read {}: {error}", pdf_path.display())))?;
    let preview = pdf::preview_pdf(pdf_path, 8, 1_200).ok();
    let preview_truncated = preview
        .as_ref()
        .and_then(|value| value.get("truncated"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let source_url = if bytes.len() > MAX_BINARY_PREVIEW_BYTES {
        None
    } else {
        Some(format!(
            "data:application/pdf;base64,{}",
            BASE64_STANDARD.encode(&bytes)
        ))
    };
    Ok(WorkspaceFilePreview {
        path: original_path.to_string(),
        display_path: display_path.to_string(),
        file_name: file_name.to_string(),
        file_type: file_type.to_string(),
        mime_type: "application/pdf".to_string(),
        kind: "pdf".to_string(),
        size_bytes,
        content: preview
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(format_error)?,
        source_url,
        is_binary: true,
        truncated: preview_truncated || bytes.len() > MAX_BINARY_PREVIEW_BYTES,
    })
}

const DEFAULT_SHARED_BROWSER_URL: &str = "https://www.baidu.com";

#[tauri::command]
pub async fn browser_stream_endpoint(
    workspace_root: PathBuf,
    data_dir: PathBuf,
    session_id: String,
) -> Result<BrowserStreamEndpoint, String> {
    let ctx = hermes_agent_rs::tools::ToolContext {
        workspace_root: workspace_root.clone(),
        data_dir: data_dir.clone(),
        shell_enabled: false,
        skill_platform: "desktop-shell".to_string(),
        provider_id: String::new(),
        model: String::new(),
        base_url: String::new(),
        api_key: None,
        max_iterations: 1,
        current_session_id: session_id.clone(),
        current_delegate_run_id: None,
        delegate_depth: 0,
    };
    let profile = runtime::ensure_runtime_ready(&ctx)
        .await
        .map_err(format_error)?;
    let target = agent_browser_target_for_session(&data_dir, &workspace_root, &session_id);
    let port = agent_browser_stream_port(&profile, &session_id);
    let relay_port = agent_browser_stream_relay_port(&profile, &session_id);
    if let Some(url) = preferred_browser_url(&data_dir, &session_id).map_err(format_error)? {
        let open_outcome = runtime::execute_program(
            &ctx,
            "agent-browser",
            vec![
                "--session".into(),
                target.session_name.clone().into(),
                "--profile".into(),
                target.profile_dir.clone().into_os_string(),
                "open".into(),
                url.into(),
            ],
            &workspace_root,
            None,
            Some(Duration::from_secs(20)),
        )
        .await
        .map_err(format_error)?;
        if open_outcome.canceled {
            return Err("agent-browser open canceled".to_string());
        }
        if open_outcome.timed_out {
            return Err("agent-browser open timed out".to_string());
        }
        if open_outcome.exit_code != Some(0) {
            let stdout = String::from_utf8_lossy(&open_outcome.stdout)
                .trim()
                .to_string();
            let stderr = String::from_utf8_lossy(&open_outcome.stderr)
                .trim()
                .to_string();
            let combined = if stdout.is_empty() {
                stderr.clone()
            } else if stderr.is_empty() {
                stdout.clone()
            } else {
                format!("{stdout}\n{stderr}")
            };
            return Err(if combined.is_empty() {
                format!(
                    "agent-browser open failed with status {:?}",
                    open_outcome.exit_code
                )
            } else {
                format!("agent-browser open failed: {combined}")
            });
        }
    }
    let mut status_outcome = runtime::execute_program(
        &ctx,
        "agent-browser",
        vec![
            "--session".into(),
            target.session_name.clone().into(),
            "stream".into(),
            "status".into(),
        ],
        &workspace_root,
        None,
        Some(Duration::from_secs(10)),
    )
    .await
    .map_err(format_error)?;
    let mut stdout = String::from_utf8_lossy(&status_outcome.stdout).to_string();
    let mut stderr = String::from_utf8_lossy(&status_outcome.stderr).to_string();
    let mut resolved_port = parse_agent_browser_stream_port(&stdout)
        .or_else(|| parse_agent_browser_stream_port(&stderr));

    if resolved_port.is_none() {
        let outcome = runtime::execute_program(
            &ctx,
            "agent-browser",
            vec![
                "--session".into(),
                target.session_name.clone().into(),
                "--profile".into(),
                target.profile_dir.clone().into_os_string(),
                "stream".into(),
                "enable".into(),
                "--port".into(),
                port.to_string().into(),
            ],
            &workspace_root,
            None,
            Some(Duration::from_secs(20)),
        )
        .await
        .map_err(format_error)?;
        if outcome.canceled {
            return Err("agent-browser stream enable canceled".to_string());
        }
        if outcome.timed_out {
            return Err("agent-browser stream enable timed out".to_string());
        }
        if outcome.exit_code != Some(0) {
            let stdout = String::from_utf8_lossy(&outcome.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&outcome.stderr).trim().to_string();
            let combined = if stdout.is_empty() {
                stderr.clone()
            } else if stderr.is_empty() {
                stdout.clone()
            } else {
                format!("{stdout}\n{stderr}")
            };
            let reusable_existing_stream = combined
                .contains("Streaming is already enabled for this session")
                || combined.contains("--profile ignored: daemon already running")
                || combined.contains("daemon already running");
            if !reusable_existing_stream {
                return Err(if stderr.is_empty() {
                    format!(
                        "agent-browser stream enable failed with status {:?}",
                        outcome.exit_code
                    )
                } else {
                    stderr
                });
            }
        }
        status_outcome = runtime::execute_program(
            &ctx,
            "agent-browser",
            vec![
                "--session".into(),
                target.session_name.clone().into(),
                "stream".into(),
                "status".into(),
            ],
            &workspace_root,
            None,
            Some(Duration::from_secs(10)),
        )
        .await
        .map_err(format_error)?;
        stdout = String::from_utf8_lossy(&status_outcome.stdout).to_string();
        stderr = String::from_utf8_lossy(&status_outcome.stderr).to_string();
        resolved_port = parse_agent_browser_stream_port(&stdout)
            .or_else(|| parse_agent_browser_stream_port(&stderr));
    }

    let resolved_port = resolved_port
        .or_else(|| {
            if status_outcome.exit_code == Some(0) {
                Some(port)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            let combined = if stdout.trim().is_empty() {
                stderr.trim().to_string()
            } else if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                format!("{}\n{}", stdout.trim(), stderr.trim())
            };
            if combined.is_empty() {
                format!(
                    "agent-browser stream status failed with status {:?}",
                    status_outcome.exit_code
                )
            } else {
                format!("agent-browser stream status failed: {combined}")
            }
        })?;

    ensure_agent_browser_stream_relay(&ctx, &target.session_name, resolved_port, relay_port)
    .await
    .map_err(format_error)?;

    Ok(BrowserStreamEndpoint {
        ws_url: format!("ws://127.0.0.1:{relay_port}"),
        port: relay_port,
        session_name: target.session_name,
    })
}

fn preferred_browser_url(data_dir: &Path, session_id: &str) -> Result<Option<String>> {
    let store = BrowserStateStore::new(data_dir)?;
    let Some(session) = store.load(session_id)? else {
        return Ok(Some(DEFAULT_SHARED_BROWSER_URL.to_string()));
    };
    for candidate in [&session.current.final_url, &session.current.url] {
        let trimmed = candidate.trim();
        if !trimmed.is_empty() && trimmed != "about:blank" {
            return Ok(Some(trimmed.to_string()));
        }
    }
    Ok(Some(DEFAULT_SHARED_BROWSER_URL.to_string()))
}

async fn ensure_agent_browser_stream_relay(
    ctx: &hermes_agent_rs::tools::ToolContext,
    relay_key: &str,
    target_port: u16,
    relay_port: u16,
) -> Result<()> {
    let relay_root = ctx.data_dir.join("browser-runtime").join("stream-relay");
    let pid_file = relay_root.join(format!("{relay_key}.pid"));
    let log_file = relay_root.join(format!("{relay_key}.log"));
    let python_code = r#"
import asyncio
import signal
import sys

HOST = "0.0.0.0"
TARGET_HOST = "127.0.0.1"
LISTEN_PORT = int(sys.argv[1])
TARGET_PORT = int(sys.argv[2])

async def pipe(reader, writer):
    try:
        while True:
            chunk = await reader.read(65536)
            if not chunk:
                break
            writer.write(chunk)
            await writer.drain()
    except Exception:
        pass
    finally:
        try:
            writer.close()
            await writer.wait_closed()
        except Exception:
            pass

async def handle(client_reader, client_writer):
    try:
        upstream_reader, upstream_writer = await asyncio.open_connection(TARGET_HOST, TARGET_PORT)
    except Exception:
        try:
            client_writer.close()
            await client_writer.wait_closed()
        except Exception:
            pass
        return

    await asyncio.gather(
        pipe(client_reader, upstream_writer),
        pipe(upstream_reader, client_writer),
    )

async def main():
    server = await asyncio.start_server(handle, HOST, LISTEN_PORT)
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stop.set)
    async with server:
        await stop.wait()

asyncio.run(main())
"#;
    let shell = format!(
        "set -eu\n\
mkdir -p {relay_root}\n\
if [ -f {pid_file} ]; then\n\
  old_pid=$(cat {pid_file} 2>/dev/null || true)\n\
  if [ -n \"$old_pid\" ] && kill -0 \"$old_pid\" 2>/dev/null; then\n\
    cmdline=\"\"\n\
    if [ -r \"/proc/$old_pid/cmdline\" ]; then\n\
      cmdline=$(tr '\\0' ' ' < \"/proc/$old_pid/cmdline\" 2>/dev/null || true)\n\
    else\n\
      cmdline=$(ps -o command= -p \"$old_pid\" 2>/dev/null || true)\n\
    fi\n\
    if echo \"$cmdline\" | grep -F \" {relay_port} {target_port}\" >/dev/null 2>&1; then\n\
      exit 0\n\
    fi\n\
    if echo \"$cmdline\" | grep -F \" {relay_port}\" >/dev/null 2>&1; then\n\
      kill \"$old_pid\" 2>/dev/null || true\n\
      i=0\n\
      while kill -0 \"$old_pid\" 2>/dev/null; do\n\
        i=$((i + 1))\n\
        if [ \"$i\" -ge 20 ]; then\n\
          kill -9 \"$old_pid\" 2>/dev/null || true\n\
        fi\n\
        if [ \"$i\" -ge 40 ]; then\n\
          echo \"timed out waiting for old relay process $old_pid to exit\" >&2\n\
          exit 1\n\
        fi\n\
        sleep 0.1\n\
      done\n\
    fi\n\
  fi\n\
  rm -f {pid_file}\n\
fi\n\
: > {log_file}\n\
nohup python3 -c {python_code} {relay_port} {target_port} > {log_file} 2>&1 &\n\
echo $! > {pid_file}\n\
sleep 1\n\
new_pid=$(cat {pid_file})\n\
if ! kill -0 \"$new_pid\" 2>/dev/null; then\n\
  cat {log_file} 2>/dev/null || true\n\
  exit 1\n\
fi\n",
        relay_root = shell_quote(&relay_root.display().to_string()),
        pid_file = shell_quote(&pid_file.display().to_string()),
        log_file = shell_quote(&log_file.display().to_string()),
        python_code = shell_quote(python_code),
        relay_port = relay_port,
        target_port = target_port,
    );
    let outcome = runtime::execute_program(
        ctx,
        "sh",
        vec!["-lc".into(), shell.into()],
        &ctx.workspace_root,
        None,
        Some(Duration::from_secs(10)),
    )
    .await
    .context("failed to start agent-browser stream relay")?;

    if outcome.canceled {
        bail!("agent-browser stream relay startup canceled");
    }
    if outcome.timed_out {
        bail!("agent-browser stream relay startup timed out");
    }
    if outcome.exit_code != Some(0) {
        let stdout = String::from_utf8_lossy(&outcome.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&outcome.stderr).trim().to_string();
        let combined = if stdout.is_empty() {
            stderr.clone()
        } else if stderr.is_empty() {
            stdout.clone()
        } else {
            format!("{stdout}\n{stderr}")
        };
        if combined.is_empty() {
            bail!(
                "agent-browser stream relay startup failed with status {:?}",
                outcome.exit_code
            );
        }
        bail!("agent-browser stream relay startup failed: {}", combined);
    }

    Ok(())
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn parse_agent_browser_stream_port(text: &str) -> Option<u16> {
    for marker in [
        "ws://127.0.0.1:",
        "ws://localhost:",
        "\"port\":",
        "\"port\": ",
        "port:",
        "port ",
    ] {
        let Some(start) = text.find(marker) else {
            continue;
        };
        let digits = text[start + marker.len()..]
            .chars()
            .skip_while(|ch| !ch.is_ascii_digit())
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if let Ok(port) = digits.parse::<u16>() {
            if port > 0 {
                return Some(port);
            }
        }
    }
    None
}

#[tauri::command]
pub fn list_workspace_tree(workspace_root: PathBuf) -> Result<WorkspaceTreeResponse, String> {
    let workspace_root = workspace_root
        .canonicalize()
        .map_err(|error| format_error(anyhow!("failed to resolve workspace root: {error}")))?;
    if !workspace_root.is_dir() {
        return Err(format_error(anyhow!(
            "{} is not a directory",
            workspace_root.display()
        )));
    }
    let mut remaining_entries = MAX_WORKSPACE_TREE_ENTRIES;
    let nodes =
        collect_workspace_tree_nodes(&workspace_root, &workspace_root, &mut remaining_entries)
            .map_err(format_error)?;
    Ok(WorkspaceTreeResponse {
        root_path: workspace_root.display().to_string(),
        nodes,
        truncated: remaining_entries == 0,
    })
}

#[tauri::command]
pub fn extensions_overview(data_dir: PathBuf) -> Result<ExtensionsOverview, String> {
    AgentBridge::extensions_overview(data_dir).map_err(format_error)
}

#[tauri::command]
pub async fn inspect_mcp_server(
    data_dir: PathBuf,
    server_name: String,
) -> Result<McpServerInspection, String> {
    AgentBridge::inspect_mcp_server(data_dir, server_name)
        .await
        .map_err(format_error)
}

#[tauri::command]
pub fn list_sessions(data_dir: PathBuf) -> Result<Vec<BridgeSessionSummary>, String> {
    AgentBridge::list_sessions(data_dir).map_err(format_error)
}

#[tauri::command]
pub fn list_providers(data_dir: PathBuf) -> Result<Vec<ProviderSummary>, String> {
    AgentBridge::list_providers(data_dir).map_err(format_error)
}

#[tauri::command]
pub fn resolve_provider_status(
    request: ResolveProviderStatusRequest,
) -> Result<BridgeProviderRuntimeStatus, String> {
    AgentBridge::resolve_provider_status(request).map_err(format_error)
}

#[tauri::command]
pub fn load_shared_provider_config(data_dir: PathBuf) -> Result<SharedAgentConfig, String> {
    AgentBridge::load_shared_provider_config(data_dir).map_err(format_error)
}

#[tauri::command]
pub fn save_shared_provider_config(
    request: SharedProviderConfigRequest,
) -> Result<SharedAgentConfig, String> {
    AgentBridge::save_shared_provider_config(request).map_err(format_error)
}

#[tauri::command]
pub fn save_cron_job(
    data_dir: PathBuf,
    request: SaveCronJobRequest,
) -> Result<hermes_agent_rs::CronJobSummary, String> {
    AgentBridge::save_cron_job(data_dir, request).map_err(format_error)
}

#[tauri::command]
pub fn delete_cron_job(data_dir: PathBuf, job_id: String) -> Result<SimpleSessionResponse, String> {
    AgentBridge::delete_cron_job(data_dir, job_id).map_err(format_error)
}

#[tauri::command]
pub fn load_session(
    data_dir: PathBuf,
    session_id: String,
) -> Result<Option<BridgeSessionDetail>, String> {
    AgentBridge::load_session(data_dir, session_id).map_err(format_error)
}

#[tauri::command]
pub fn search_sessions(
    data_dir: PathBuf,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<BridgeSessionSearchResult>, String> {
    AgentBridge::search_sessions(data_dir, query, limit).map_err(format_error)
}

#[tauri::command]
pub fn list_delegate_runs(
    data_dir: PathBuf,
    parent_session_id: Option<String>,
) -> Result<Vec<BridgeDelegateRun>, String> {
    AgentBridge::list_delegate_runs(data_dir, parent_session_id).map_err(format_error)
}

#[tauri::command]
pub fn cancel_delegate_run(data_dir: PathBuf, run_id: String) -> Result<BridgeDelegateRun, String> {
    AgentBridge::cancel_delegate_run(data_dir, run_id).map_err(format_error)
}

#[tauri::command]
pub async fn retry_delegate_run<R: Runtime>(
    _app: AppHandle<R>,
    request: RetryDelegateRunRequest,
) -> Result<BridgeDelegateRun, String> {
    AgentBridge::retry_delegate_run(request)
        .await
        .map_err(format_error)
}

#[tauri::command]
pub fn pick_workspace_folder(current_dir: Option<String>) -> Result<Option<String>, String> {
    pick_workspace_folder_impl(current_dir).map_err(format_error)
}

#[cfg(target_os = "macos")]
fn pick_workspace_folder_impl(current_dir: Option<String>) -> Result<Option<String>> {
    let mut script = String::from("POSIX path of (choose folder with prompt \"选择新线程目录\"");
    if let Some(dir) = current_dir
        .map(PathBuf::from)
        .filter(|path| path.exists() && path.is_dir())
    {
        let escaped = dir
            .display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        script.push_str(&format!(" default location POSIX file \"{}\"", escaped));
    }
    script.push(')');

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|error| anyhow!("failed to open folder picker: {error}"))?;

    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            return Ok(None);
        }
        return Ok(Some(value));
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("-128") {
        return Ok(None);
    }
    Err(anyhow!("failed to open folder picker: {}", stderr.trim()))
}

#[cfg(target_os = "windows")]
fn pick_workspace_folder_impl(current_dir: Option<String>) -> Result<Option<String>> {
    let initial_dir = current_dir
        .filter(|path| PathBuf::from(path).is_dir())
        .unwrap_or_default()
        .replace('\'', "''");
    let script = format!(
        concat!(
            "Add-Type -AssemblyName System.Windows.Forms; ",
            "$dialog = New-Object System.Windows.Forms.FolderBrowserDialog; ",
            "$dialog.Description = '选择新线程目录'; ",
            "$dialog.UseDescriptionForTitle = $true; ",
            "if ('{0}' -ne '') {{ $dialog.SelectedPath = '{0}'; }} ",
            "if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $dialog.SelectedPath }}"
        ),
        initial_dir
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map_err(|error| anyhow!("failed to open folder picker: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("failed to open folder picker: {}", stderr.trim()));
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

#[cfg(target_os = "linux")]
fn pick_workspace_folder_impl(current_dir: Option<String>) -> Result<Option<String>> {
    let initial_dir = current_dir
        .map(PathBuf::from)
        .filter(|path| path.exists() && path.is_dir());

    let zenity_result = {
        let mut command = Command::new("zenity");
        command.args(["--file-selection", "--directory", "--title=选择新线程目录"]);
        if let Some(dir) = &initial_dir {
            command.arg(format!("--filename={}/", dir.display()));
        }
        command.output()
    };

    if let Ok(output) = zenity_result {
        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(if value.is_empty() { None } else { Some(value) });
        }
        if output.status.code() == Some(1) {
            return Ok(None);
        }
    }

    let kdialog_result = {
        let mut command = Command::new("kdialog");
        command.args(["--getexistingdirectory", ".", "--title", "选择新线程目录"]);
        if let Some(dir) = &initial_dir {
            command.arg(dir);
        }
        command.output()
    };

    if let Ok(output) = kdialog_result {
        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(if value.is_empty() { None } else { Some(value) });
        }
        if output.status.code() == Some(1) {
            return Ok(None);
        }
    }

    Err(anyhow!(
        "failed to open folder picker: no supported dialog command found"
    ))
}

fn update_last_session_id<R: Runtime>(app: &AppHandle<R>, session_id: String) {
    let state = app.state::<DesktopState>();
    if let Ok(mut last_session_id) = state.last_session_id.lock() {
        *last_session_id = Some(session_id);
    }
}

fn current_scheduler_pause_reason<R: Runtime>(
    app: &AppHandle<R>,
    data_dir: &PathBuf,
) -> Option<String> {
    let state = app.state::<DesktopState>();
    if state.interactive_runs.load(Ordering::Relaxed) > 0 {
        return Some("对话运行中".to_string());
    }
    match hermes_agent_rs::approval::list_requests(data_dir) {
        Ok(items)
            if items
                .iter()
                .any(|item| item.status == ApprovalStatus::Pending) =>
        {
            Some("存在待处理审批".to_string())
        }
        Ok(_) => None,
        Err(error) => Some(format!("审批状态检查失败: {error}")),
    }
}

fn update_scheduler_state<R: Runtime, F>(app: &AppHandle<R>, mut update: F)
where
    F: FnMut(&mut CronSchedulerStatus),
{
    let state = app.state::<DesktopState>();
    if let Ok(mut scheduler) = state.cron_scheduler.lock() {
        update(&mut scheduler.status);
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

const MAX_TEXT_PREVIEW_BYTES: usize = 512_000;
const MAX_BINARY_PREVIEW_BYTES: usize = 8_000_000;
const MAX_OFFICE_INLINE_PREVIEW_BYTES: usize = 64_000_000;
const MAX_WORKSPACE_TREE_ENTRIES: usize = 5_000;
const WORKSPACE_TREE_SKIPPED_DIRS: &[&str] =
    &[".git", "node_modules", "target", "dist", "out", ".next"];

fn resolve_workspace_file(workspace_root: &Path, file_path: &str) -> Result<PathBuf> {
    let trimmed = file_path.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("file path is required"));
    }
    let candidate = PathBuf::from(trimmed);
    let target = if candidate.is_absolute() {
        candidate
    } else {
        workspace_root.join(candidate)
    };
    let resolved = target
        .canonicalize()
        .map_err(|error| anyhow!("failed to resolve {}: {error}", target.display()))?;
    if !resolved.starts_with(workspace_root) {
        return Err(anyhow!("file path escapes workspace root"));
    }
    if !resolved.is_file() {
        return Err(anyhow!("{} is not a file", resolved.display()));
    }
    Ok(resolved)
}

fn open_path_in_system_app(path: &Path) -> Result<()> {
    let status = if cfg!(target_os = "macos") {
        Command::new("open").arg(path).status()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", "start", "", &path.display().to_string()])
            .status()
    } else {
        Command::new("xdg-open").arg(path).status()
    }
    .with_context(|| format!("failed to launch system opener for {}", path.display()))?;

    if status.success() {
        return Ok(());
    }

    bail!(
        "system opener exited with status {:?} for {}",
        status.code(),
        path.display()
    )
}

fn collect_workspace_tree_nodes(
    workspace_root: &Path,
    current_dir: &Path,
    remaining_entries: &mut usize,
) -> Result<Vec<WorkspaceTreeNode>> {
    if *remaining_entries == 0 {
        return Ok(Vec::new());
    }

    let mut directories = Vec::new();
    let mut files = Vec::new();

    for entry in fs::read_dir(current_dir)
        .map_err(|error| anyhow!("failed to read {}: {error}", current_dir.display()))?
    {
        if *remaining_entries == 0 {
            break;
        }

        let entry =
            entry.map_err(|error| anyhow!("failed to read {}: {error}", current_dir.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| anyhow!("failed to inspect {}: {error}", entry.path().display()))?;
        if file_type.is_symlink() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if file_type.is_dir() && WORKSPACE_TREE_SKIPPED_DIRS.contains(&name.as_str()) {
            continue;
        }

        let path = entry.path();

        *remaining_entries -= 1;

        if file_type.is_dir() {
            let children = collect_workspace_tree_nodes(workspace_root, &path, remaining_entries)?;
            directories.push(WorkspaceTreeNode {
                path: path.display().to_string(),
                name,
                kind: "directory".to_string(),
                children,
            });
            continue;
        }

        if file_type.is_file() {
            files.push(WorkspaceTreeNode {
                path: path.display().to_string(),
                name,
                kind: "file".to_string(),
                children: Vec::new(),
            });
            continue;
        }
    }

    directories.sort_by_cached_key(|node| node.name.to_ascii_lowercase());
    files.sort_by_cached_key(|node| node.name.to_ascii_lowercase());
    directories.extend(files);
    Ok(directories)
}

fn workspace_file_kind_and_mime(path: &Path) -> (String, String) {
    let ext = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "md" | "mdx" => ("markdown".to_string(), "text/markdown".to_string()),
        "txt" | "rs" | "ts" | "tsx" | "js" | "jsx" | "css" | "scss" | "html" | "xml" | "json"
        | "yaml" | "yml" | "toml" | "csv" | "log" | "sh" | "py" | "java" | "go" | "c" | "cc"
        | "cpp" | "h" | "hpp" | "swift" | "kt" | "sql" => {
            ("text".to_string(), "text/plain".to_string())
        }
        "png" => ("image".to_string(), "image/png".to_string()),
        "jpg" | "jpeg" => ("image".to_string(), "image/jpeg".to_string()),
        "gif" => ("image".to_string(), "image/gif".to_string()),
        "webp" => ("image".to_string(), "image/webp".to_string()),
        "bmp" => ("image".to_string(), "image/bmp".to_string()),
        "ico" => ("image".to_string(), "image/x-icon".to_string()),
        "svg" => ("image".to_string(), "image/svg+xml".to_string()),
        "docx" => (
            "document".to_string(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string(),
        ),
        "xlsx" => (
            "spreadsheet".to_string(),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string(),
        ),
        "pptx" => (
            "presentation".to_string(),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation".to_string(),
        ),
        "pdf" => ("pdf".to_string(), "application/pdf".to_string()),
        "mp3" => ("audio".to_string(), "audio/mpeg".to_string()),
        "wav" => ("audio".to_string(), "audio/wav".to_string()),
        "ogg" => ("audio".to_string(), "audio/ogg".to_string()),
        "m4a" => ("audio".to_string(), "audio/mp4".to_string()),
        "flac" => ("audio".to_string(), "audio/flac".to_string()),
        "mp4" => ("video".to_string(), "video/mp4".to_string()),
        "webm" => ("video".to_string(), "video/webm".to_string()),
        "mov" => ("video".to_string(), "video/quicktime".to_string()),
        "m4v" => ("video".to_string(), "video/x-m4v".to_string()),
        _ => ("binary".to_string(), "application/octet-stream".to_string()),
    }
}

fn runtime_status_context(request: RuntimeProfileRequest) -> hermes_agent_rs::tools::ToolContext {
    let data_dir = request
        .data_dir
        .clone()
        .unwrap_or_else(|| request.workspace_root.join(".hermes-agent-rs"));
    hermes_agent_rs::tools::ToolContext {
        workspace_root: request.workspace_root,
        data_dir,
        shell_enabled: true,
        skill_platform: "desktop".to_string(),
        provider_id: "desktop".to_string(),
        model: "runtime-status".to_string(),
        base_url: String::new(),
        api_key: None,
        max_iterations: 1,
        current_session_id: "runtime-status".to_string(),
        current_delegate_run_id: None,
        delegate_depth: 0,
    }
}

fn format_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
