use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use regex::Regex;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use tokio::time::Duration;

use crate::browser_state::{BrowserElement, BrowserImage, BrowserPageState};
use crate::runtime;
use crate::runtime_profile::{BrowserBackend, RuntimeProfile};
use crate::tools::{ToolContext, truncated};

const AGENT_BROWSER_TIMEOUT_PADDING_SECONDS: u64 = 25;
const AGENT_BROWSER_CONTENT_TYPE: &str = "application/x-agent-browser-snapshot";
const MAX_IMAGES: usize = 64;
const AGENT_BROWSER_STREAM_BLOCK_SIZE: u16 = 32;
const AGENT_BROWSER_STREAM_BASE_PORT: u16 = 46000;
const AGENT_BROWSER_STREAM_RELAY_BASE_PORT: u16 = 49000;
const AGENT_BROWSER_STREAM_PROFILE_BUCKETS: u16 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveBrowserBackend {
    AgentBrowser,
    ElectronDevtools,
}

#[derive(Debug, Deserialize)]
struct AgentBrowserBatchResult {
    success: bool,
    result: Option<Value>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ElectronDevtoolsState {
    url: String,
    final_url: Option<String>,
    content_type: Option<String>,
    title: Option<String>,
    content: String,
    #[serde(default)]
    elements: Vec<BrowserElement>,
    #[serde(default)]
    images: Vec<BrowserImage>,
    #[serde(default)]
    truncated_body: bool,
}

#[derive(Debug)]
pub struct AgentBrowserTarget {
    pub session_name: String,
    pub profile_dir: PathBuf,
}

pub async fn resolve_active_backend(ctx: &ToolContext) -> Result<ActiveBrowserBackend> {
    let profile = RuntimeProfile::resolve(&ctx.data_dir, &ctx.workspace_root)
        .unwrap_or_else(|_| RuntimeProfile::fallback(&ctx.workspace_root));
    match profile.browser_backend {
        BrowserBackend::AgentBrowser => {
            if agent_browser_available(ctx).await? {
                Ok(ActiveBrowserBackend::AgentBrowser)
            } else {
                bail!(
                    "browser backend is configured as `agent_browser`, but `agent-browser` is not available in the runtime"
                )
            }
        }
        BrowserBackend::ElectronDevtools => Ok(ActiveBrowserBackend::ElectronDevtools),
        _ => bail!(
            "browser fallback has been removed; configure `browser_backend: agent_browser` or `browser_backend: electron_devtools` for this workspace"
        ),
    }
}

pub async fn agent_browser_navigate(
    ctx: &ToolContext,
    url: &str,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    run_agent_browser_and_capture(
        ctx,
        vec![vec!["open".to_string(), url.to_string()]],
        max_chars,
        timeout_seconds,
        Some(url),
    )
    .await
}

pub async fn agent_browser_click(
    ctx: &ToolContext,
    reference: &str,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    run_agent_browser_and_capture(
        ctx,
        vec![vec!["click".to_string(), reference.to_string()]],
        max_chars,
        timeout_seconds,
        None,
    )
    .await
}

pub async fn agent_browser_fill(
    ctx: &ToolContext,
    reference: &str,
    text: &str,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    run_agent_browser_and_capture(
        ctx,
        vec![vec![
            "fill".to_string(),
            reference.to_string(),
            text.to_string(),
        ]],
        max_chars,
        timeout_seconds,
        None,
    )
    .await
}

pub async fn agent_browser_press(
    ctx: &ToolContext,
    key: &str,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    run_agent_browser_and_capture(
        ctx,
        vec![vec!["press".to_string(), key.to_string()]],
        max_chars,
        timeout_seconds,
        None,
    )
    .await
}

pub async fn agent_browser_scroll(
    ctx: &ToolContext,
    direction: &str,
    amount: usize,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    run_agent_browser_and_capture(
        ctx,
        vec![vec![
            "scroll".to_string(),
            direction.to_string(),
            amount.to_string(),
        ]],
        max_chars,
        timeout_seconds,
        None,
    )
    .await
}

pub async fn agent_browser_back(
    ctx: &ToolContext,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    run_agent_browser_and_capture(
        ctx,
        vec![vec!["back".to_string()]],
        max_chars,
        timeout_seconds,
        None,
    )
    .await
}

pub async fn agent_browser_forward(
    ctx: &ToolContext,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    run_agent_browser_and_capture(
        ctx,
        vec![vec![
            "eval".to_string(),
            "history.forward(); 'ok'".to_string(),
        ]],
        max_chars,
        timeout_seconds,
        None,
    )
    .await
}

pub async fn agent_browser_snapshot(
    ctx: &ToolContext,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    run_agent_browser_and_capture(ctx, Vec::new(), max_chars, timeout_seconds, None).await
}

pub async fn agent_browser_eval(
    ctx: &ToolContext,
    expression: &str,
    timeout_seconds: u64,
) -> Result<Value> {
    let results = run_agent_browser_batch(
        ctx,
        vec![vec!["eval".to_string(), expression.to_string()]],
        timeout_seconds,
    )
    .await?;
    let result = result_object(
        results
            .first()
            .ok_or_else(|| anyhow!("agent-browser eval returned no result"))?,
        "eval",
    )?;
    Ok(result.get("result").cloned().unwrap_or(Value::Null))
}

pub async fn agent_browser_screenshot(
    ctx: &ToolContext,
    output_path: &std::path::Path,
    full_page: bool,
    annotate: bool,
    timeout_seconds: u64,
) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create browser screenshot directory {}",
                parent.display()
            )
        })?;
    }

    let profile = RuntimeProfile::resolve(&ctx.data_dir, &ctx.workspace_root)
        .unwrap_or_else(|_| RuntimeProfile::fallback(&ctx.workspace_root));
    let target = agent_browser_target(ctx, &profile);
    ensure_agent_browser_profile_dir(ctx, &target.profile_dir, timeout_seconds).await?;

    let mut args = vec![
        OsString::from("--json"),
        OsString::from("--session"),
        OsString::from(target.session_name),
        OsString::from("--profile"),
        target.profile_dir.into_os_string(),
    ];
    if let Some(path) = agent_browser_executable_path(&profile) {
        args.push(OsString::from("--executable-path"));
        args.push(path);
    }
    args.push(OsString::from("screenshot"));
    if annotate {
        args.push(OsString::from("--annotate"));
    }
    if full_page {
        args.push(OsString::from("--full"));
    }
    args.push(output_path.as_os_str().to_os_string());

    let outcome = runtime::execute_program(
        ctx,
        "agent-browser",
        args,
        &ctx.workspace_root,
        None,
        Some(Duration::from_secs(
            timeout_seconds.saturating_add(AGENT_BROWSER_TIMEOUT_PADDING_SECONDS),
        )),
    )
    .await
    .context("failed to execute agent-browser screenshot")?;

    if outcome.canceled {
        bail!("agent-browser screenshot canceled");
    }
    if outcome.timed_out {
        bail!("agent-browser screenshot timed out");
    }
    if outcome.exit_code != Some(0) {
        let stderr = String::from_utf8_lossy(&outcome.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!(
                "agent-browser screenshot exited with status {:?}",
                outcome.exit_code
            );
        }
        bail!("agent-browser screenshot failed: {}", stderr);
    }
    if !output_path.is_file() {
        bail!(
            "agent-browser screenshot did not create {}",
            output_path.display()
        );
    }
    Ok(())
}

pub async fn electron_devtools_navigate(
    ctx: &ToolContext,
    url: &str,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    let state: ElectronDevtoolsState = call_electron_devtools(
        ctx,
        "navigate",
        json!({
            "url": url,
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    Ok(browser_page_from_electron_state(state, max_chars))
}

pub async fn electron_devtools_click(
    ctx: &ToolContext,
    reference: &str,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    let state: ElectronDevtoolsState = call_electron_devtools(
        ctx,
        "click",
        json!({
            "reference": reference,
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    Ok(browser_page_from_electron_state(state, max_chars))
}

pub async fn electron_devtools_fill(
    ctx: &ToolContext,
    reference: &str,
    text: &str,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    let state: ElectronDevtoolsState = call_electron_devtools(
        ctx,
        "fill",
        json!({
            "reference": reference,
            "text": text,
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    Ok(browser_page_from_electron_state(state, max_chars))
}

pub async fn electron_devtools_press(
    ctx: &ToolContext,
    key: &str,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    let state: ElectronDevtoolsState = call_electron_devtools(
        ctx,
        "press",
        json!({
            "key": key,
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    Ok(browser_page_from_electron_state(state, max_chars))
}

pub async fn electron_devtools_scroll(
    ctx: &ToolContext,
    direction: &str,
    amount: usize,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    let state: ElectronDevtoolsState = call_electron_devtools(
        ctx,
        "scroll",
        json!({
            "direction": direction,
            "amount": amount,
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    Ok(browser_page_from_electron_state(state, max_chars))
}

pub async fn electron_devtools_back(
    ctx: &ToolContext,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    let state: ElectronDevtoolsState = call_electron_devtools(
        ctx,
        "back",
        json!({
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    Ok(browser_page_from_electron_state(state, max_chars))
}

pub async fn electron_devtools_forward(
    ctx: &ToolContext,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    let state: ElectronDevtoolsState = call_electron_devtools(
        ctx,
        "forward",
        json!({
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    Ok(browser_page_from_electron_state(state, max_chars))
}

pub async fn electron_devtools_snapshot(
    ctx: &ToolContext,
    max_chars: usize,
) -> Result<BrowserPageState> {
    let state: ElectronDevtoolsState = call_electron_devtools(ctx, "snapshot", json!({})).await?;
    Ok(browser_page_from_electron_state(state, max_chars))
}

pub async fn electron_devtools_hover(
    ctx: &ToolContext,
    reference: &str,
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    let state: ElectronDevtoolsState = call_electron_devtools(
        ctx,
        "hover",
        json!({
            "reference": reference,
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    Ok(browser_page_from_electron_state(state, max_chars))
}

pub async fn electron_devtools_upload(
    ctx: &ToolContext,
    reference: &str,
    files: &[String],
    max_chars: usize,
    timeout_seconds: u64,
) -> Result<BrowserPageState> {
    let state: ElectronDevtoolsState = call_electron_devtools(
        ctx,
        "upload",
        json!({
            "reference": reference,
            "files": files,
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    Ok(browser_page_from_electron_state(state, max_chars))
}

pub async fn electron_devtools_eval(
    ctx: &ToolContext,
    expression: &str,
    timeout_seconds: u64,
) -> Result<Value> {
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct EvalResponse {
        result: Value,
    }

    let response: EvalResponse = call_electron_devtools(
        ctx,
        "eval",
        json!({
            "expression": expression,
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    Ok(response.result)
}

pub async fn electron_devtools_screenshot(
    ctx: &ToolContext,
    full_page: bool,
    annotate: bool,
    timeout_seconds: u64,
) -> Result<Vec<u8>> {
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ScreenshotResponse {
        data: String,
    }

    let response: ScreenshotResponse = call_electron_devtools(
        ctx,
        "screenshot",
        json!({
            "fullPage": full_page,
            "annotate": annotate,
            "timeoutSeconds": timeout_seconds,
        }),
    )
    .await?;
    BASE64_STANDARD
        .decode(response.data)
        .context("electron devtools screenshot returned invalid base64")
}

async fn agent_browser_available(ctx: &ToolContext) -> Result<bool> {
    let outcome = runtime::execute_program(
        ctx,
        "agent-browser",
        vec![OsString::from("--version")],
        &ctx.workspace_root,
        None,
        Some(Duration::from_secs(10)),
    )
    .await;

    match outcome {
        Ok(outcome) => Ok(!outcome.timed_out && !outcome.canceled && outcome.exit_code == Some(0)),
        Err(_) => Ok(false),
    }
}

async fn run_agent_browser_and_capture(
    ctx: &ToolContext,
    mut commands: Vec<Vec<String>>,
    max_chars: usize,
    timeout_seconds: u64,
    source_url: Option<&str>,
) -> Result<BrowserPageState> {
    commands.push(vec!["get".to_string(), "url".to_string()]);
    commands.push(vec!["get".to_string(), "title".to_string()]);
    commands.push(vec![
        "snapshot".to_string(),
        "--compact".to_string(),
        "--urls".to_string(),
    ]);
    commands.push(vec![
        "eval".to_string(),
        "Array.from(document.images || []).slice(0, 64).map((img) => ({ src: img.currentSrc || img.src || '', alt: img.alt || '' })).filter((img) => img.src)"
            .to_string(),
    ]);

    let results = run_agent_browser_batch(ctx, commands, timeout_seconds).await?;
    if results.len() < 4 {
        bail!("agent-browser returned an incomplete batch response");
    }

    let url_result = result_object(&results[results.len() - 4], "get url")?;
    let title_result = result_object(&results[results.len() - 3], "get title")?;
    let snapshot_result = result_object(&results[results.len() - 2], "snapshot")?;
    let images_result = result_object(&results[results.len() - 1], "eval")?;

    let final_url = url_result
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("agent-browser get url did not return a URL"))?;
    let title = title_result
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty());
    let snapshot = snapshot_result
        .get("snapshot")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("agent-browser snapshot did not return snapshot text"))?;
    let refs = snapshot_result
        .get("refs")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("agent-browser snapshot did not return refs metadata"))?;
    let images = parse_agent_browser_images(images_result.get("result"));
    let elements = parse_agent_browser_elements(snapshot, refs);

    let source_url = source_url.unwrap_or(&final_url).to_string();

    Ok(BrowserPageState::new(
        source_url,
        final_url,
        AGENT_BROWSER_CONTENT_TYPE,
        title,
        truncated(snapshot.to_string(), max_chars),
        elements,
        images,
        snapshot.chars().count() > max_chars,
    ))
}

async fn run_agent_browser_batch(
    ctx: &ToolContext,
    commands: Vec<Vec<String>>,
    timeout_seconds: u64,
) -> Result<Vec<AgentBrowserBatchResult>> {
    let profile = RuntimeProfile::resolve(&ctx.data_dir, &ctx.workspace_root)
        .unwrap_or_else(|_| RuntimeProfile::fallback(&ctx.workspace_root));
    let target = agent_browser_target(ctx, &profile);
    ensure_agent_browser_profile_dir(ctx, &target.profile_dir, timeout_seconds).await?;

    let stdin =
        serde_json::to_vec(&commands).context("failed to serialize agent-browser batch input")?;
    let mut args = vec![
        OsString::from("--json"),
        OsString::from("--session"),
        OsString::from(target.session_name),
        OsString::from("--profile"),
        target.profile_dir.into_os_string(),
    ];
    if let Some(path) = agent_browser_executable_path(&profile) {
        args.push(OsString::from("--executable-path"));
        args.push(path);
    }
    args.push(OsString::from("batch"));
    args.push(OsString::from("--bail"));
    let outcome = runtime::execute_program(
        ctx,
        "agent-browser",
        args,
        &ctx.workspace_root,
        Some(stdin),
        Some(Duration::from_secs(
            timeout_seconds.saturating_add(AGENT_BROWSER_TIMEOUT_PADDING_SECONDS),
        )),
    )
    .await
    .context("failed to execute agent-browser")?;

    if outcome.canceled {
        bail!("agent-browser command canceled");
    }
    if outcome.timed_out {
        bail!("agent-browser command timed out");
    }
    if outcome.exit_code != Some(0) {
        let stderr = String::from_utf8_lossy(&outcome.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("agent-browser exited with status {:?}", outcome.exit_code);
        }
        bail!("agent-browser failed: {}", stderr);
    }

    let stdout =
        String::from_utf8(outcome.stdout).context("agent-browser returned non-UTF8 stdout")?;
    serde_json::from_str(&stdout).with_context(|| {
        format!(
            "failed to parse agent-browser batch output: {}",
            truncated(stdout, 240)
        )
    })
}

async fn call_electron_devtools<T>(ctx: &ToolContext, action: &str, payload: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    let base_url = std::env::var("HERMES_RS_ELECTRON_DEVTOOLS_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:47712".to_string());
    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "{}/session/{}/{}",
            base_url.trim_end_matches('/'),
            ctx.current_session_id,
            action
        ))
        .json(&payload)
        .send()
        .await
        .with_context(|| format!("failed to call electron devtools backend for {action}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if body.trim().is_empty() {
            bail!("electron devtools {} failed with status {}", action, status);
        }
        bail!("electron devtools {} failed: {}", action, body.trim());
    }
    response
        .json::<T>()
        .await
        .with_context(|| format!("failed to parse electron devtools response for {action}"))
}

fn browser_page_from_electron_state(
    state: ElectronDevtoolsState,
    max_chars: usize,
) -> BrowserPageState {
    let ElectronDevtoolsState {
        url,
        final_url,
        content_type,
        title,
        content,
        elements,
        images,
        truncated_body,
    } = state;
    let content_chars = content.chars().count();
    let truncated_content = truncated(content, max_chars);
    let final_url = final_url.unwrap_or_else(|| url.clone());
    BrowserPageState::new(
        url,
        final_url,
        content_type.unwrap_or_else(|| "text/html".to_string()),
        title.filter(|value| !value.trim().is_empty()),
        truncated_content,
        elements,
        images,
        truncated_body || content_chars > max_chars,
    )
}

async fn ensure_agent_browser_profile_dir(
    ctx: &ToolContext,
    profile_dir: &PathBuf,
    timeout_seconds: u64,
) -> Result<()> {
    let outcome = runtime::execute_program(
        ctx,
        "mkdir",
        vec![OsString::from("-p"), profile_dir.as_os_str().to_os_string()],
        &ctx.workspace_root,
        None,
        Some(Duration::from_secs(timeout_seconds.saturating_add(5))),
    )
    .await
    .with_context(|| {
        format!(
            "failed to prepare browser profile dir {}",
            profile_dir.display()
        )
    })?;
    if outcome.exit_code != Some(0) {
        bail!(
            "failed to prepare browser profile dir {}",
            profile_dir.display()
        );
    }
    Ok(())
}

pub fn agent_browser_stream_port_range(profile: &RuntimeProfile) -> (u16, u16) {
    let mut hasher = DefaultHasher::new();
    profile.profile_slug.hash(&mut hasher);
    let bucket = (hasher.finish() as u16) % AGENT_BROWSER_STREAM_PROFILE_BUCKETS;
    let start = AGENT_BROWSER_STREAM_BASE_PORT + bucket * AGENT_BROWSER_STREAM_BLOCK_SIZE;
    let end = start + AGENT_BROWSER_STREAM_BLOCK_SIZE - 1;
    (start, end)
}

pub fn agent_browser_stream_port(profile: &RuntimeProfile, session_id: &str) -> u16 {
    let (start, _) = agent_browser_stream_port_range(profile);
    let _ = session_id;
    start
}

pub fn agent_browser_stream_relay_port_range(profile: &RuntimeProfile) -> (u16, u16) {
    let mut hasher = DefaultHasher::new();
    profile.profile_slug.hash(&mut hasher);
    let bucket = (hasher.finish() as u16) % AGENT_BROWSER_STREAM_PROFILE_BUCKETS;
    let start = AGENT_BROWSER_STREAM_RELAY_BASE_PORT + bucket * AGENT_BROWSER_STREAM_BLOCK_SIZE;
    let end = start + AGENT_BROWSER_STREAM_BLOCK_SIZE - 1;
    (start, end)
}

pub fn agent_browser_stream_relay_port(profile: &RuntimeProfile, session_id: &str) -> u16 {
    let (start, _) = agent_browser_stream_relay_port_range(profile);
    let _ = session_id;
    start
}

fn agent_browser_shared_session_name(profile: &RuntimeProfile) -> String {
    format!("hermes-{}-shared", profile.profile_slug)
}

pub fn agent_browser_target_for_session(
    data_dir: &std::path::Path,
    workspace_root: &std::path::Path,
    session_id: &str,
) -> AgentBrowserTarget {
    let _ = session_id;
    let profile = RuntimeProfile::resolve(data_dir, workspace_root)
        .unwrap_or_else(|_| RuntimeProfile::fallback(workspace_root));
    let session_name = agent_browser_shared_session_name(&profile);
    let profile_dir = agent_browser_profile_dir(&profile, data_dir, &session_name);
    AgentBrowserTarget {
        session_name,
        profile_dir,
    }
}

fn agent_browser_target(ctx: &ToolContext, profile: &RuntimeProfile) -> AgentBrowserTarget {
    let _ = ctx;
    let session_name = agent_browser_shared_session_name(profile);
    let profile_dir = agent_browser_profile_dir(profile, &ctx.data_dir, &session_name);
    AgentBrowserTarget {
        session_name,
        profile_dir,
    }
}

fn agent_browser_profile_dir(
    profile: &RuntimeProfile,
    data_dir: &std::path::Path,
    session_name: &str,
) -> PathBuf {
    let _ = profile;
    data_dir
        .join("browser-runtime")
        .join("profile")
        .join(session_name)
}

fn agent_browser_executable_path(profile: &RuntimeProfile) -> Option<OsString> {
    if let Some(path) = profile
        .env
        .get("HERMES_RS_AGENT_BROWSER_EXECUTABLE_PATH")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return Some(OsString::from(path));
    }

    std::env::var("HERMES_RS_AGENT_BROWSER_EXECUTABLE_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(OsString::from)
}

fn result_object<'a>(result: &'a AgentBrowserBatchResult, label: &str) -> Result<&'a Value> {
    if !result.success {
        bail!(
            "agent-browser {} failed: {}",
            label,
            result
                .error
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("unknown error")
        );
    }
    result
        .result
        .as_ref()
        .ok_or_else(|| anyhow!("agent-browser {} did not return a result payload", label))
}

fn parse_agent_browser_elements(
    snapshot: &str,
    refs: &serde_json::Map<String, Value>,
) -> Vec<BrowserElement> {
    let line_pattern = Regex::new(
        r#"^\s*(?P<ref>@e\d+)\s+\[(?P<role>[^\]]+)\](?:\s+"(?P<label>[^"]*)")?(?:\s+(?P<rest>.*))?$"#,
    )
    .expect("valid snapshot line regex");
    let url_pattern = Regex::new(r#"https?://\S+"#).expect("valid URL regex");
    let mut items = refs
        .iter()
        .filter_map(|(ref_id, value)| {
            let order = ref_id.trim_start_matches('e').parse::<usize>().ok()?;
            Some((order, ref_id, value))
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|(order, _, _)| *order);

    items
        .into_iter()
        .map(|(_, ref_id, value)| {
            let snapshot_ref = format!("@{ref_id}");
            let metadata_role = value
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("element");
            let metadata_name = value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let snapshot_line = snapshot
                .lines()
                .find(|line| line.trim_start().starts_with(&snapshot_ref))
                .unwrap_or_default()
                .trim();
            let captures = line_pattern.captures(snapshot_line);
            let parsed_role = captures
                .as_ref()
                .and_then(|captures| captures.name("role"))
                .map(|value| value.as_str().trim().to_string());
            let parsed_label = captures
                .as_ref()
                .and_then(|captures| captures.name("label"))
                .map(|value| value.as_str().trim().to_string())
                .filter(|value| !value.is_empty());
            let parsed_rest = captures
                .as_ref()
                .and_then(|captures| captures.name("rest"))
                .map(|value| value.as_str().trim().to_string())
                .filter(|value| !value.is_empty());
            let role = parsed_role
                .as_deref()
                .unwrap_or(metadata_role)
                .trim()
                .to_string();
            let label = if !metadata_name.is_empty() {
                metadata_name
            } else if let Some(parsed_label) = parsed_label {
                parsed_label
            } else {
                extract_placeholder_label(parsed_rest.as_deref()).unwrap_or_else(|| role.clone())
            };
            let target = if role.eq_ignore_ascii_case("link") {
                parsed_rest.as_deref().and_then(|rest| {
                    url_pattern
                        .find(rest)
                        .map(|value| value.as_str().to_string())
                })
            } else {
                None
            };

            BrowserElement {
                ref_id: snapshot_ref,
                kind: map_role_to_kind(&role),
                label,
                target,
                role: Some(role),
                selector: None,
                bbox: None,
                disabled: None,
                checked: None,
                selected: None,
                required: None,
                field_name: None,
                value: None,
                form_id: None,
                form_action: None,
                form_method: None,
            }
        })
        .collect()
}

fn parse_agent_browser_images(result: Option<&Value>) -> Vec<BrowserImage> {
    result
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(MAX_IMAGES)
                .filter_map(|item| {
                    let src = item.get("src")?.as_str()?.trim();
                    if src.is_empty() {
                        return None;
                    }
                    Some(BrowserImage {
                        src: src.to_string(),
                        alt: item
                            .get("alt")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_placeholder_label(rest: Option<&str>) -> Option<String> {
    let rest = rest?;
    let captures = Regex::new(r#"placeholder="([^"]+)""#)
        .expect("valid placeholder regex")
        .captures(rest)?;
    let value = captures.get(1)?.as_str().trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn map_role_to_kind(role: &str) -> String {
    let lowered = role.trim().to_ascii_lowercase();
    if lowered.starts_with("input ") {
        return Regex::new(r#"type="([^"]+)""#)
            .expect("valid input type regex")
            .captures(&lowered)
            .and_then(|captures| captures.get(1))
            .map(|value| format!("input:{}", value.as_str()))
            .unwrap_or_else(|| "input:text".to_string());
    }
    match lowered.as_str() {
        "searchbox" => "input:search".to_string(),
        "textbox" | "textarea" | "combobox" => "input:text".to_string(),
        "link" => "link".to_string(),
        "button" => "button".to_string(),
        "checkbox" => "checkbox".to_string(),
        "radio" => "radio".to_string(),
        "heading" => "heading".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{map_role_to_kind, parse_agent_browser_elements, parse_agent_browser_images};
    use serde_json::json;

    #[test]
    fn parses_agent_browser_snapshot_refs() {
        let snapshot = r#"
@e1 [heading] "Example Domain" [level=1]
@e2 [input type="email"] placeholder="Email"
@e3 [link] "Learn more" https://example.com/docs
"#;
        let refs = json!({
            "e1": { "role": "heading", "name": "Example Domain" },
            "e2": { "role": "input type=\"email\"", "name": "" },
            "e3": { "role": "link", "name": "Learn more" }
        });
        let elements = parse_agent_browser_elements(snapshot, refs.as_object().expect("object"));
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0].ref_id, "@e1");
        assert_eq!(elements[1].kind, "input:email");
        assert_eq!(elements[1].label, "Email");
        assert_eq!(
            elements[2].target.as_deref(),
            Some("https://example.com/docs")
        );
    }

    #[test]
    fn parses_agent_browser_images() {
        let images = parse_agent_browser_images(Some(&json!([
            { "src": "https://example.com/logo.png", "alt": "Logo" },
            { "src": "", "alt": "Skip" }
        ])));
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].alt, "Logo");
    }

    #[test]
    fn maps_roles_to_existing_browser_kinds() {
        assert_eq!(map_role_to_kind("textbox"), "input:text");
        assert_eq!(map_role_to_kind("searchbox"), "input:search");
        assert_eq!(map_role_to_kind("input type=\"email\""), "input:email");
        assert_eq!(map_role_to_kind("button"), "button");
    }
}
