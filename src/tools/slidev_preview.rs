use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, sleep};

use crate::tools::{Tool, ToolContext, relative_display, resolve_workspace_path};
use crate::types::{ToolDefinition, object_schema};

pub struct SlidevPreviewTool;

#[derive(Debug, Clone)]
pub(crate) struct SlidevPreviewSession {
    pub url: String,
    pub port: u16,
    pub pid: u32,
    pub command: PathBuf,
}

#[derive(Debug, Deserialize)]
struct SlidevPreviewArgs {
    file_path: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

const DEFAULT_TIMEOUT_SECONDS: u64 = 20;

#[async_trait]
impl Tool for SlidevPreviewTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "slidev_preview",
            "Start a local Slidev preview server for a workspace .md/.mdx deck and return the local URL. Uses the desktop shell's bundled @slidev/cli when available.",
            object_schema(
                json!({
                    "file_path": {
                        "type": "string",
                        "description": "Path to the Slidev deck Markdown file, relative to the workspace root."
                    },
                    "port": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 65535,
                        "description": "Optional local port. If omitted, an available port is selected."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 60,
                        "description": "How long to wait for the preview server to become ready."
                    }
                }),
                &["file_path"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: SlidevPreviewArgs =
            serde_json::from_value(args).context("invalid slidev_preview arguments")?;
        let deck_path = resolve_workspace_path(&ctx.workspace_root, &args.file_path)?;
        let preview = start_slidev_preview(
            &ctx.workspace_root,
            &ctx.data_dir,
            &ctx.current_session_id,
            &deck_path,
            args.port,
            args.timeout_seconds,
        )
        .await?;

        Ok(format!(
            "slidev preview started\nfile: {}\nurl: {}\nport: {}\npid: {}\ncommand: {}",
            relative_display(&ctx.workspace_root, &deck_path),
            preview.url,
            preview.port,
            preview.pid,
            preview.command.display()
        ))
    }
}

pub(crate) async fn start_slidev_preview(
    workspace_root: &Path,
    data_dir: &Path,
    session_id: &str,
    deck_path: &Path,
    port: Option<u16>,
    timeout_seconds: Option<u64>,
) -> Result<SlidevPreviewSession> {
    validate_deck_path(deck_path)?;

    let slidev = resolve_slidev_binary(workspace_root)?;
    let port = match port {
        Some(port) => port,
        None => allocate_local_port()?,
    };
    let timeout_seconds = timeout_seconds
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
        .clamp(1, 60);
    let url = format!("http://127.0.0.1:{port}/");
    let logs = SlidevPreviewLogs::create(data_dir, session_id)?;
    let mut child = Command::new(&slidev)
        .arg(deck_path)
        .arg("--port")
        .arg(port.to_string())
        .arg("--remote")
        .arg("--bind")
        .arg("127.0.0.1")
        .current_dir(deck_path.parent().unwrap_or(workspace_root))
        .env("BROWSER", "none")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(logs.stdout_file.try_clone()?))
        .stderr(Stdio::from(logs.stderr_file.try_clone()?))
        .spawn()
        .with_context(|| format!("failed to start Slidev CLI at {}", slidev.display()))?;
    let pid = child.id();

    if let Err(error) = wait_for_preview(&url, &mut child, timeout_seconds).await {
        let _ = child.kill();
        let _ = child.wait();
        let output = logs.read_output();
        bail!("{error}{output}");
    }

    Ok(SlidevPreviewSession {
        url,
        port,
        pid,
        command: slidev,
    })
}

struct SlidevPreviewLogs {
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    stdout_file: File,
    stderr_file: File,
}

impl SlidevPreviewLogs {
    fn create(data_dir: &Path, session_id: &str) -> Result<Self> {
        let log_dir = data_dir.join("slidev-preview");
        fs::create_dir_all(&log_dir)
            .with_context(|| format!("failed to create {}", log_dir.display()))?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let safe_session = session_id
            .chars()
            .map(|value| {
                if value.is_ascii_alphanumeric() || value == '-' || value == '_' {
                    value
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let prefix = format!("{safe_session}-{timestamp}");
        let stdout_path = log_dir.join(format!("{prefix}.stdout.log"));
        let stderr_path = log_dir.join(format!("{prefix}.stderr.log"));
        let stdout_file = File::create(&stdout_path)
            .with_context(|| format!("failed to create {}", stdout_path.display()))?;
        let stderr_file = File::create(&stderr_path)
            .with_context(|| format!("failed to create {}", stderr_path.display()))?;
        Ok(Self {
            stdout_path,
            stderr_path,
            stdout_file,
            stderr_file,
        })
    }

    fn read_output(&self) -> String {
        let stdout = read_log_excerpt(&self.stdout_path);
        let stderr = read_log_excerpt(&self.stderr_path);
        let mut blocks = Vec::new();
        if !stderr.trim().is_empty() {
            blocks.push(format!("stderr:\n{}", stderr.trim()));
        }
        if !stdout.trim().is_empty() {
            blocks.push(format!("stdout:\n{}", stdout.trim()));
        }
        if blocks.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", blocks.join("\n\n"))
        }
    }
}

fn read_log_excerpt(path: &Path) -> String {
    let mut value = String::new();
    if let Ok(mut file) = File::open(path) {
        let _ = file.read_to_string(&mut value);
    }
    if value.len() > 4_000 {
        value[value.len() - 4_000..].to_string()
    } else {
        value
    }
}

fn validate_deck_path(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("Slidev deck does not exist: {}", path.display());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "md" | "mdx") {
        bail!("slidev_preview supports only .md and .mdx files");
    }
    Ok(())
}

fn resolve_slidev_binary(workspace_root: &Path) -> Result<PathBuf> {
    let binary_name = if cfg!(windows) {
        "slidev.cmd"
    } else {
        "slidev"
    };
    let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        workspace_root
            .join("node_modules")
            .join(".bin")
            .join(binary_name),
        workspace_root
            .join("desktop-shell")
            .join("node_modules")
            .join(".bin")
            .join(binary_name),
        manifest_root
            .join("desktop-shell")
            .join("node_modules")
            .join(".bin")
            .join(binary_name),
    ];

    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    bail!(
        "Slidev CLI is not available. Run `npm install` in desktop-shell so @slidev/cli is bundled."
    )
}

fn allocate_local_port() -> Result<u16> {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).context("failed to allocate local preview port")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn wait_for_preview(
    url: &str,
    child: &mut std::process::Child,
    timeout_seconds: u64,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .context("failed to build preview HTTP client")?;
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_seconds);
    let mut last_error = None;

    while std::time::Instant::now() < deadline {
        if let Some(status) = child
            .try_wait()
            .context("failed to inspect Slidev process")?
        {
            bail!("Slidev process exited before preview was ready: {status}");
        }
        match client.get(url).send().await {
            Ok(response) if response.status().as_u16() < 500 => return Ok(()),
            Ok(response) => {
                last_error = Some(format!("HTTP {}", response.status()));
            }
            Err(error) => {
                last_error = Some(error.to_string());
            }
        }
        sleep(Duration::from_millis(300)).await;
    }

    bail!(
        "timed out waiting for Slidev preview at {url}{}",
        last_error
            .map(|error| format!(": {error}"))
            .unwrap_or_default()
    )
}
