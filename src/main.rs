use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use clap::Parser;
use hermes_agent_rs::browser_backend::{
    agent_browser_stream_port, agent_browser_stream_relay_port, agent_browser_target_for_session,
};
use hermes_agent_rs::browser_state::BrowserStateStore;
use hermes_agent_rs::tools::ToolContext;
use hermes_agent_rs::{
    Agent, AgentBridge, AppConfig, BridgeEventEnvelope, BridgeEventSink, Cli, Commands,
    RecordingBridgeEventSink, ResolveProviderStatusRequest, RetryDelegateRunRequest,
    RunAgentRequest, RunCronJobRequest, RuntimeProfile, RuntimeStatus as RuntimeStatusView,
    SaveCronJobRequest, SessionCommandRequest, build_doctor_report, build_semantic_memory_digest,
    load_session_for_semantic_digest,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    let config = AppConfig::load(&cli.global)?;

    match cli.command {
        Some(Commands::Profile) => {
            println!("{}", serde_json::to_string_pretty(&config.runtime_profile)?);
        }
        Some(Commands::RuntimeStatus) => {
            let status =
                hermes_agent_rs::runtime::inspect_runtime(&tool_context_from_config(&config))
                    .await?;
            println!(
                "{}",
                serde_json::to_string_pretty::<RuntimeStatusView>(&status)?
            );
        }
        Some(Commands::RuntimeStart) => {
            let status =
                hermes_agent_rs::runtime::start_runtime(&tool_context_from_config(&config)).await?;
            println!(
                "{}",
                serde_json::to_string_pretty::<RuntimeStatusView>(&status)?
            );
        }
        Some(Commands::RuntimeRepair) => {
            let status =
                hermes_agent_rs::runtime::repair_runtime(&tool_context_from_config(&config))
                    .await?;
            println!(
                "{}",
                serde_json::to_string_pretty::<RuntimeStatusView>(&status)?
            );
        }
        Some(Commands::RuntimeReset) => {
            let status =
                hermes_agent_rs::runtime::reset_runtime(&tool_context_from_config(&config)).await?;
            println!(
                "{}",
                serde_json::to_string_pretty::<RuntimeStatusView>(&status)?
            );
        }
        Some(Commands::Doctor(args)) => {
            let status =
                hermes_agent_rs::runtime::inspect_runtime(&tool_context_from_config(&config))
                    .await?;
            let report = build_doctor_report(&config, status);
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.render_text());
            }
        }
        Some(Commands::DesktopBridge) => {
            run_desktop_bridge().await?;
        }
        Some(Commands::Office2PdfRender(render)) => {
            render_office2pdf(render.input, render.output)?;
        }
        Some(Commands::MemoryCompress(args)) => {
            let session =
                load_session_for_semantic_digest(&config.data_dir, args.session_id.as_deref())?
                    .ok_or_else(|| anyhow!("no session found to compress"))?;
            let digest = build_semantic_memory_digest(&config.data_dir, &session, &args.query)?;
            match args.format.trim().to_lowercase().as_str() {
                "json" => println!("{}", serde_json::to_string_pretty(&digest)?),
                "markdown" | "md" => println!("{}", digest.render_markdown()),
                other => {
                    return Err(anyhow!(
                        "unsupported memory-compress format `{other}`; use `markdown` or `json`"
                    ));
                }
            }
        }
        Some(Commands::Chat(chat)) => {
            let mut agent = Agent::new(config)?;
            if let Some(prompt) = chat.prompt {
                let reply = agent.run_prompt(&prompt).await?;
                println!("{reply}");
            } else {
                run_repl(&mut agent).await?;
            }
        }
        Some(Commands::DebugContext(debug)) => {
            let mut agent = Agent::new(config)?;
            let preview = agent.debug_context_preview(&debug.prompt).await?;
            if debug.execute {
                let assistant_response = agent.run_prompt(&debug.prompt).await?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "preview": preview,
                        "assistant_response": assistant_response,
                    }))?
                );
            } else {
                println!("{}", serde_json::to_string_pretty(&preview)?);
            }
        }
        None => {
            let mut agent = Agent::new(config)?;
            run_repl(&mut agent).await?;
        }
    }

    Ok(())
}

fn render_office2pdf(input_path: PathBuf, output_path: PathBuf) -> Result<()> {
    let result = office2pdf::convert(&input_path)
        .with_context(|| format!("office2pdf failed to convert {}", input_path.display()))?;
    let temp_path = output_path.with_extension("pdf.tmp");
    fs::write(&temp_path, &result.pdf)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, &output_path)
        .with_context(|| format!("failed to move pdf to {}", output_path.display()))?;
    Ok(())
}

fn tool_context_from_config(config: &AppConfig) -> ToolContext {
    ToolContext {
        workspace_root: config.workspace_root.clone(),
        data_dir: config.data_dir.clone(),
        shell_enabled: config.enable_shell_tool,
        skill_platform: config.skill_platform.clone(),
        provider_id: config.provider_id.clone(),
        model: config.model.clone(),
        base_url: config.base_url.clone(),
        api_key: config.api_key.clone(),
        max_iterations: config.max_iterations,
        current_session_id: config
            .session_id
            .clone()
            .unwrap_or_else(|| "runtime-status".to_string()),
        current_delegate_run_id: None,
        delegate_depth: 0,
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("hermes_agent_rs=info,warn"));

    fmt()
        .with_env_filter(filter)
        .without_time()
        .with_ansi(false)
        .with_writer(io::stderr)
        .init();
}

async fn run_repl(agent: &mut Agent) -> Result<()> {
    println!("Crab");
    println!("session: {}", agent.session_id());
    println!("Type /exit to quit, /clear to reset the in-memory conversation.");

    let stdin = io::stdin();
    let mut line = String::new();

    loop {
        print!("you> ");
        io::stdout().flush()?;

        line.clear();
        if stdin.read_line(&mut line)? == 0 {
            println!();
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input == "/exit" || input == "/quit" {
            break;
        }
        if input == "/clear" {
            agent.clear_history()?;
            println!("conversation cleared");
            continue;
        }
        if input == "/session" {
            println!("session: {}", agent.session_id());
            continue;
        }

        let reply = agent.run_prompt(input).await?;
        println!("assistant> {reply}");
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopBridgeRequest {
    command: String,
    #[serde(default)]
    args: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopBridgeEventResult<T> {
    result: T,
    events: Vec<hermes_agent_rs::BridgeEventEnvelope>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserStreamEndpoint {
    ws_url: String,
    port: u16,
    session_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserStreamEndpointRequest {
    workspace_root: PathBuf,
    data_dir: PathBuf,
    session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeProfileRequest {
    workspace_root: PathBuf,
    data_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceFilePreviewRequest {
    workspace_root: PathBuf,
    data_dir: Option<PathBuf>,
    file_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceFilePreview {
    path: String,
    display_path: String,
    file_name: String,
    file_type: String,
    mime_type: String,
    kind: String,
    size_bytes: u64,
    content: Option<String>,
    source_url: Option<String>,
    is_binary: bool,
    truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DueCronJobsRequest {
    data_dir: PathBuf,
    now_unix: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserCurrentUrlRequest {
    data_dir: PathBuf,
    session_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserCurrentUrlResponse {
    url: String,
}

async fn run_desktop_bridge() -> Result<()> {
    let mut stdin = String::new();
    io::stdin().read_to_string(&mut stdin)?;
    let request: DesktopBridgeRequest = serde_json::from_str(&stdin)?;
    match request.command.as_str() {
        "run_agent" => {
            let run_request = parse_arg::<RunAgentRequest>(&request.args, "request")?;
            let stdout = Arc::new(Mutex::new(io::stdout()));
            let mut sink = StreamingDesktopBridgeEventSink::new(stdout.clone());
            let result = AgentBridge::run_with_event_sink(run_request, &mut sink).await?;
            sink.finish()?;
            write_desktop_bridge_stream_frame(stdout, "result", &result)?;
            Ok(())
        }
        "resume_approval" => {
            let session_request = parse_arg::<SessionCommandRequest>(&request.args, "request")?;
            let approval_id = parse_arg::<String>(&request.args, "approvalId")?;
            let stdout = Arc::new(Mutex::new(io::stdout()));
            let mut sink = StreamingDesktopBridgeEventSink::new(stdout.clone());
            let result = AgentBridge::resume_approval_with_event_sink(
                session_request,
                approval_id,
                &mut sink,
            )
            .await?;
            sink.finish()?;
            write_desktop_bridge_stream_frame(stdout, "result", &result)?;
            Ok(())
        }
        "run_cron_job" => {
            let cron_request = parse_arg::<RunCronJobRequest>(&request.args, "request")?;
            let stdout = Arc::new(Mutex::new(io::stdout()));
            let mut sink = StreamingDesktopBridgeEventSink::new(stdout.clone());
            let result = AgentBridge::run_cron_job_with_event_sink(cron_request, &mut sink).await?;
            sink.finish()?;
            write_desktop_bridge_stream_frame(stdout, "result", &result)?;
            Ok(())
        }
        "retry_delegate_run" => {
            let retry_request = parse_arg::<RetryDelegateRunRequest>(&request.args, "request")?;
            let stdout = Arc::new(Mutex::new(io::stdout()));
            let mut sink = StreamingDesktopBridgeEventSink::new(stdout.clone());
            let result =
                AgentBridge::retry_delegate_run_with_event_sink(retry_request, &mut sink).await?;
            sink.finish()?;
            write_desktop_bridge_stream_frame(stdout, "result", &result)?;
            Ok(())
        }
        _ => {
            let response = dispatch_desktop_bridge(request).await?;
            println!("{}", serde_json::to_string(&response)?);
            Ok(())
        }
    }
}

struct StreamingDesktopBridgeEventSink {
    stdout: Arc<Mutex<io::Stdout>>,
    first_error: Option<anyhow::Error>,
}

impl StreamingDesktopBridgeEventSink {
    fn new(stdout: Arc<Mutex<io::Stdout>>) -> Self {
        Self {
            stdout,
            first_error: None,
        }
    }

    fn finish(&mut self) -> Result<()> {
        if let Some(error) = self.first_error.take() {
            return Err(error);
        }
        Ok(())
    }
}

impl BridgeEventSink for StreamingDesktopBridgeEventSink {
    fn push(&mut self, event: BridgeEventEnvelope) {
        if self.first_error.is_some() {
            return;
        }
        if let Err(error) = write_desktop_bridge_stream_frame(self.stdout.clone(), "event", &event)
        {
            self.first_error = Some(error);
        }
    }
}

fn write_desktop_bridge_stream_frame<T: Serialize>(
    stdout: Arc<Mutex<io::Stdout>>,
    frame_type: &str,
    payload: &T,
) -> Result<()> {
    let frame = json!({
        "type": frame_type,
        "payload": payload,
    });
    let mut handle = stdout.lock().map_err(|_| anyhow!("stdout lock poisoned"))?;
    writeln!(handle, "{}", serde_json::to_string(&frame)?)?;
    handle.flush()?;
    Ok(())
}

async fn dispatch_desktop_bridge(request: DesktopBridgeRequest) -> Result<Value> {
    match request.command.as_str() {
        "list_sessions" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            Ok(serde_json::to_value(AgentBridge::list_sessions(data_dir)?)?)
        }
        "load_session" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            let session_id = parse_arg::<String>(&request.args, "sessionId")?;
            Ok(serde_json::to_value(AgentBridge::load_session(
                data_dir, session_id,
            )?)?)
        }
        "search_sessions" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            let query = parse_arg::<String>(&request.args, "query")?;
            let limit = optional_arg::<usize>(&request.args, "limit")?;
            Ok(serde_json::to_value(AgentBridge::search_sessions(
                data_dir, query, limit,
            )?)?)
        }
        "list_skills" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            Ok(serde_json::to_value(AgentBridge::list_skills(data_dir)?)?)
        }
        "view_skill" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            let name = parse_arg::<String>(&request.args, "name")?;
            let category = optional_arg::<String>(&request.args, "category")?;
            let file_path = optional_arg::<String>(&request.args, "filePath")?;
            Ok(serde_json::to_value(AgentBridge::view_skill(
                data_dir, name, category, file_path,
            )?)?)
        }
        "extensions_overview" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            Ok(serde_json::to_value(AgentBridge::extensions_overview(
                data_dir,
            )?)?)
        }
        "list_providers" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            Ok(serde_json::to_value(AgentBridge::list_providers(
                data_dir,
            )?)?)
        }
        "resolve_provider_status" => {
            let bridge_request =
                parse_arg::<ResolveProviderStatusRequest>(&request.args, "request")?;
            Ok(serde_json::to_value(AgentBridge::resolve_provider_status(
                bridge_request,
            )?)?)
        }
        "load_shared_provider_config" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            Ok(serde_json::to_value(
                AgentBridge::load_shared_provider_config(data_dir)?,
            )?)
        }
        "save_shared_provider_config" => {
            let bridge_request = parse_arg::<hermes_agent_rs::SharedProviderConfigRequest>(
                &request.args,
                "request",
            )?;
            Ok(serde_json::to_value(
                AgentBridge::save_shared_provider_config(bridge_request)?,
            )?)
        }
        "list_approvals" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            Ok(serde_json::to_value(AgentBridge::list_approvals(
                data_dir,
            )?)?)
        }
        "resolve_approval" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            let approval_id = parse_arg::<String>(&request.args, "approvalId")?;
            let approved = parse_arg::<bool>(&request.args, "approved")?;
            Ok(serde_json::to_value(AgentBridge::resolve_approval(
                data_dir,
                approval_id,
                approved,
            )?)?)
        }
        "list_delegate_runs" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            let parent_session_id = optional_arg::<String>(&request.args, "parentSessionId")?;
            Ok(serde_json::to_value(AgentBridge::list_delegate_runs(
                data_dir,
                parent_session_id,
            )?)?)
        }
        "cancel_delegate_run" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            let run_id = parse_arg::<String>(&request.args, "runId")?;
            Ok(serde_json::to_value(AgentBridge::cancel_delegate_run(
                data_dir, run_id,
            )?)?)
        }
        "save_cron_job" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            let save_request = parse_arg::<SaveCronJobRequest>(&request.args, "request")?;
            Ok(serde_json::to_value(AgentBridge::save_cron_job(
                data_dir,
                save_request,
            )?)?)
        }
        "delete_cron_job" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            let job_id = parse_arg::<String>(&request.args, "jobId")?;
            Ok(serde_json::to_value(AgentBridge::delete_cron_job(
                data_dir, job_id,
            )?)?)
        }
        "clear_session" => {
            let session_request = parse_arg::<SessionCommandRequest>(&request.args, "request")?;
            Ok(serde_json::to_value(AgentBridge::clear_session(
                session_request,
            )?)?)
        }
        "stop_session" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            let session_id = parse_arg::<String>(&request.args, "sessionId")?;
            Ok(serde_json::to_value(AgentBridge::stop_session(
                data_dir, session_id,
            )?)?)
        }
        "inspect_mcp_server" => {
            let data_dir = parse_arg::<PathBuf>(&request.args, "dataDir")?;
            let server_name = parse_arg::<String>(&request.args, "serverName")?;
            Ok(serde_json::to_value(
                AgentBridge::inspect_mcp_server(data_dir, server_name).await?,
            )?)
        }
        "run_agent" => {
            let run_request = parse_arg::<RunAgentRequest>(&request.args, "request")?;
            let mut sink = RecordingBridgeEventSink::new();
            let result = AgentBridge::run_with_event_sink(run_request, &mut sink).await?;
            Ok(serde_json::to_value(DesktopBridgeEventResult {
                result,
                events: sink.into_events(),
            })?)
        }
        "resume_approval" => {
            let session_request = parse_arg::<SessionCommandRequest>(&request.args, "request")?;
            let approval_id = parse_arg::<String>(&request.args, "approvalId")?;
            let mut sink = RecordingBridgeEventSink::new();
            let result = AgentBridge::resume_approval_with_event_sink(
                session_request,
                approval_id,
                &mut sink,
            )
            .await?;
            Ok(serde_json::to_value(DesktopBridgeEventResult {
                result,
                events: sink.into_events(),
            })?)
        }
        "run_cron_job" => {
            let cron_request = parse_arg::<RunCronJobRequest>(&request.args, "request")?;
            let mut sink = RecordingBridgeEventSink::new();
            let result = AgentBridge::run_cron_job_with_event_sink(cron_request, &mut sink).await?;
            Ok(serde_json::to_value(DesktopBridgeEventResult {
                result,
                events: sink.into_events(),
            })?)
        }
        "retry_delegate_run" => {
            let retry_request = parse_arg::<RetryDelegateRunRequest>(&request.args, "request")?;
            let mut sink = RecordingBridgeEventSink::new();
            let result =
                AgentBridge::retry_delegate_run_with_event_sink(retry_request, &mut sink).await?;
            Ok(serde_json::to_value(DesktopBridgeEventResult {
                result,
                events: sink.into_events(),
            })?)
        }
        "resolve_runtime_profile" => {
            let profile_request = serde_json::from_value::<RuntimeProfileRequest>(request.args)?;
            let data_dir = profile_request
                .data_dir
                .clone()
                .unwrap_or_else(|| profile_request.workspace_root.join(".hermes-agent-rs"));
            Ok(serde_json::to_value(RuntimeProfile::resolve(
                &data_dir,
                &profile_request.workspace_root,
            )?)?)
        }
        "resolve_runtime_status" => {
            let profile_request = serde_json::from_value::<RuntimeProfileRequest>(request.args)?;
            let ctx = runtime_status_context(profile_request);
            Ok(serde_json::to_value(
                hermes_agent_rs::runtime::inspect_runtime(&ctx).await?,
            )?)
        }
        "start_runtime" => {
            let profile_request = serde_json::from_value::<RuntimeProfileRequest>(request.args)?;
            let ctx = runtime_status_context(profile_request);
            Ok(serde_json::to_value(
                hermes_agent_rs::runtime::start_runtime(&ctx).await?,
            )?)
        }
        "repair_runtime" => {
            let profile_request = serde_json::from_value::<RuntimeProfileRequest>(request.args)?;
            let ctx = runtime_status_context(profile_request);
            Ok(serde_json::to_value(
                hermes_agent_rs::runtime::repair_runtime(&ctx).await?,
            )?)
        }
        "reset_runtime" => {
            let profile_request = serde_json::from_value::<RuntimeProfileRequest>(request.args)?;
            let ctx = runtime_status_context(profile_request);
            Ok(serde_json::to_value(
                hermes_agent_rs::runtime::reset_runtime(&ctx).await?,
            )?)
        }
        "browser_stream_endpoint" => {
            let endpoint_request =
                serde_json::from_value::<BrowserStreamEndpointRequest>(request.args)?;
            Ok(serde_json::to_value(
                browser_stream_endpoint(endpoint_request).await?,
            )?)
        }
        "view_workspace_file" => {
            let preview_request =
                serde_json::from_value::<WorkspaceFilePreviewRequest>(request.args)?;
            Ok(serde_json::to_value(
                preview_workspace_file(preview_request).await?,
            )?)
        }
        "list_due_cron_jobs" => {
            let due_request = serde_json::from_value::<DueCronJobsRequest>(request.args)?;
            let now = due_request.now_unix.unwrap_or_else(unix_now);
            let due_ids = hermes_agent_rs::cron::list_due_jobs(&due_request.data_dir, now)?
                .into_iter()
                .map(|job| job.id)
                .collect::<Vec<_>>();
            Ok(serde_json::to_value(due_ids)?)
        }
        "browser_current_url" => {
            let url_request = serde_json::from_value::<BrowserCurrentUrlRequest>(request.args)?;
            let url = preferred_browser_url(&url_request.data_dir, &url_request.session_id)?
                .unwrap_or_else(|| "https://www.baidu.com".to_string());
            Ok(serde_json::to_value(BrowserCurrentUrlResponse { url })?)
        }
        _ => anyhow::bail!(
            "desktop bridge command `{}` is not implemented",
            request.command
        ),
    }
}

fn parse_arg<T>(args: &Value, key: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let value = args
        .get(key)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing argument `{key}`"))?;
    Ok(serde_json::from_value(value)?)
}

fn optional_arg<T>(args: &Value, key: &str) -> Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    let Some(value) = args.get(key).cloned() else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_value(value)?))
}

fn runtime_status_context(request: RuntimeProfileRequest) -> ToolContext {
    let data_dir = request
        .data_dir
        .clone()
        .unwrap_or_else(|| request.workspace_root.join(".hermes-agent-rs"));
    ToolContext {
        workspace_root: request.workspace_root,
        data_dir,
        shell_enabled: true,
        skill_platform: "electron".to_string(),
        provider_id: "electron".to_string(),
        model: "runtime-status".to_string(),
        base_url: String::new(),
        api_key: None,
        max_iterations: 1,
        current_session_id: "runtime-status".to_string(),
        current_delegate_run_id: None,
        delegate_depth: 0,
    }
}

async fn browser_stream_endpoint(
    request: BrowserStreamEndpointRequest,
) -> Result<BrowserStreamEndpoint> {
    let ctx = ToolContext {
        workspace_root: request.workspace_root.clone(),
        data_dir: request.data_dir.clone(),
        shell_enabled: false,
        skill_platform: "electron".to_string(),
        provider_id: "desktop".to_string(),
        model: "workspace-browser".to_string(),
        base_url: String::new(),
        api_key: None,
        max_iterations: 1,
        current_session_id: request.session_id.clone(),
        current_delegate_run_id: None,
        delegate_depth: 0,
    };
    let profile = hermes_agent_rs::runtime::ensure_runtime_ready(&ctx).await?;
    let target = agent_browser_target_for_session(
        &request.data_dir,
        &request.workspace_root,
        &request.session_id,
    );
    let port = agent_browser_stream_port(&profile, &request.session_id);
    let relay_port = agent_browser_stream_relay_port(&profile, &request.session_id);
    if let Some(url) = preferred_browser_url(&request.data_dir, &request.session_id)? {
        let open_outcome = hermes_agent_rs::runtime::execute_program(
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
            &request.workspace_root,
            None,
            Some(std::time::Duration::from_secs(20)),
        )
        .await?;
        if open_outcome.canceled {
            anyhow::bail!("agent-browser open canceled");
        }
        if open_outcome.timed_out {
            anyhow::bail!("agent-browser open timed out");
        }
        if open_outcome.exit_code != Some(0) {
            let stdout = String::from_utf8_lossy(&open_outcome.stdout)
                .trim()
                .to_string();
            let stderr = String::from_utf8_lossy(&open_outcome.stderr)
                .trim()
                .to_string();
            let combined = if stdout.is_empty() {
                stderr
            } else if stderr.is_empty() {
                stdout
            } else {
                format!("{stdout}\n{stderr}")
            };
            if combined.is_empty() {
                anyhow::bail!(
                    "agent-browser open failed with status {:?}",
                    open_outcome.exit_code
                );
            }
            anyhow::bail!("agent-browser open failed: {combined}");
        }
    }
    let mut status_outcome = hermes_agent_rs::runtime::execute_program(
        &ctx,
        "agent-browser",
        vec![
            "--session".into(),
            target.session_name.clone().into(),
            "stream".into(),
            "status".into(),
        ],
        &request.workspace_root,
        None,
        Some(std::time::Duration::from_secs(10)),
    )
    .await?;
    let mut stdout = String::from_utf8_lossy(&status_outcome.stdout).to_string();
    let mut stderr = String::from_utf8_lossy(&status_outcome.stderr).to_string();
    let mut attempts = 0usize;
    while attempts < 6
        && (status_outcome.timed_out
            || status_outcome.canceled
            || status_outcome.exit_code != Some(0)
            || stdout.trim().is_empty())
    {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        status_outcome = hermes_agent_rs::runtime::execute_program(
            &ctx,
            "agent-browser",
            vec![
                "--session".into(),
                target.session_name.clone().into(),
                "stream".into(),
                "status".into(),
            ],
            &request.workspace_root,
            None,
            Some(std::time::Duration::from_secs(10)),
        )
        .await?;
        stdout = String::from_utf8_lossy(&status_outcome.stdout).to_string();
        stderr = String::from_utf8_lossy(&status_outcome.stderr).to_string();
        attempts += 1;
    }
    if status_outcome.canceled {
        anyhow::bail!("agent-browser stream status canceled");
    }
    if status_outcome.timed_out {
        anyhow::bail!("agent-browser stream status timed out");
    }
    if status_outcome.exit_code != Some(0) {
        let combined = if stdout.trim().is_empty() {
            stderr.trim().to_string()
        } else if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("{}\n{}", stdout.trim(), stderr.trim())
        };
        if combined.is_empty() {
            anyhow::bail!(
                "agent-browser stream status failed with status {:?}",
                status_outcome.exit_code
            );
        }
        anyhow::bail!("agent-browser stream status failed: {combined}");
    }

    ensure_agent_browser_stream_relay(&ctx, &target.session_name, port, relay_port).await?;

    Ok(BrowserStreamEndpoint {
        ws_url: format!("ws://127.0.0.1:{relay_port}"),
        port: relay_port,
        session_name: target.session_name,
    })
}

fn preferred_browser_url(data_dir: &Path, session_id: &str) -> Result<Option<String>> {
    let store = BrowserStateStore::new(data_dir.to_path_buf())?;
    let Some(session) = store.load(session_id)? else {
        return Ok(Some("https://www.baidu.com".to_string()));
    };
    for candidate in [&session.current.final_url, &session.current.url] {
        let trimmed = candidate.trim();
        if !trimmed.is_empty() && trimmed != "about:blank" {
            return Ok(Some(trimmed.to_string()));
        }
    }
    Ok(Some("https://www.baidu.com".to_string()))
}

async fn ensure_agent_browser_stream_relay(
    ctx: &ToolContext,
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
    let outcome = hermes_agent_rs::runtime::execute_shell(
        ctx,
        &shell,
        &ctx.workspace_root,
        Some(std::time::Duration::from_secs(20)),
    )
    .await?;
    if outcome.canceled {
        anyhow::bail!("agent-browser stream relay startup canceled");
    }
    if outcome.timed_out {
        anyhow::bail!("agent-browser stream relay startup timed out");
    }
    if outcome.exit_code == Some(0) {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&outcome.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&outcome.stderr).trim().to_string();
    let combined = if stdout.is_empty() {
        stderr
    } else if stderr.is_empty() {
        stdout
    } else {
        format!("{stdout}\n{stderr}")
    };
    if combined.is_empty() {
        anyhow::bail!(
            "failed to start browser stream relay with exit code {:?}",
            outcome.exit_code
        );
    }
    anyhow::bail!("failed to start browser stream relay: {combined}");
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let escaped = value.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

const MAX_TEXT_PREVIEW_BYTES: usize = 512_000;
const MAX_BINARY_PREVIEW_BYTES: usize = 8_000_000;
const MAX_OFFICE_INLINE_PREVIEW_BYTES: usize = 64_000_000;

async fn preview_workspace_file(
    request: WorkspaceFilePreviewRequest,
) -> Result<WorkspaceFilePreview> {
    let workspace_root = request
        .workspace_root
        .canonicalize()
        .map_err(|error| anyhow!("failed to resolve workspace root: {error}"))?;
    let data_dir = request
        .data_dir
        .unwrap_or_else(|| workspace_root.join(".hermes-agent-rs"));
    let target = resolve_workspace_file(&workspace_root, &request.file_path)?;
    let metadata = fs::metadata(&target)
        .map_err(|error| anyhow!("failed to stat {}: {error}", target.display()))?;
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
    let render_ctx = ToolContext {
        workspace_root: workspace_root.clone(),
        data_dir,
        shell_enabled: false,
        skill_platform: "desktop-file-preview".to_string(),
        provider_id: "desktop".to_string(),
        model: "workspace-preview".to_string(),
        base_url: String::new(),
        api_key: None,
        max_iterations: 1,
        current_session_id: format!("workspace-preview:{display_path}"),
        current_delegate_run_id: None,
        delegate_depth: 0,
    };

    if kind == "spreadsheet" {
        let rendered =
            hermes_agent_rs::office_render::render_pdf_via_runtime(&render_ctx, &target).await?;
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
        let preview = hermes_agent_rs::office::preview_docx(&target, 24).ok();
        let preview_truncated = preview
            .as_ref()
            .and_then(|value| value.get("truncated"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let bytes = fs::read(&target)
            .map_err(|error| anyhow!("failed to read {}: {error}", target.display()))?;
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
            content: preview.as_ref().map(serde_json::to_string).transpose()?,
            source_url,
            is_binary: true,
            truncated: preview_truncated || bytes.len() > MAX_OFFICE_INLINE_PREVIEW_BYTES,
        });
    }

    if kind == "presentation" {
        let rendered =
            hermes_agent_rs::office_render::render_pdf_via_runtime(&render_ctx, &target).await?;
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
        let bytes = fs::read(&target)
            .map_err(|error| anyhow!("failed to read {}: {error}", target.display()))?;
        let preview = hermes_agent_rs::pdf::preview_pdf(&target, 8, 1_200).ok();
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
            content: preview.as_ref().map(serde_json::to_string).transpose()?,
            source_url,
            is_binary: true,
            truncated: preview_truncated || bytes.len() > MAX_BINARY_PREVIEW_BYTES,
        });
    }

    let bytes = fs::read(&target)
        .map_err(|error| anyhow!("failed to read {}: {error}", target.display()))?;
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
) -> Result<WorkspaceFilePreview> {
    let bytes = fs::read(pdf_path)
        .map_err(|error| anyhow!("failed to read {}: {error}", pdf_path.display()))?;
    let preview = hermes_agent_rs::pdf::preview_pdf(pdf_path, 8, 1_200).ok();
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
        content: preview.as_ref().map(serde_json::to_string).transpose()?,
        source_url,
        is_binary: true,
        truncated: preview_truncated || bytes.len() > MAX_BINARY_PREVIEW_BYTES,
    })
}

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

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
