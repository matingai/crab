use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

#[derive(Debug, Clone, Serialize)]
pub struct McpConfiguredServer {
    pub name: String,
    pub transport: String,
    pub target: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerInspection {
    pub server: McpConfiguredServer,
    pub tools: Vec<McpToolDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCachedInspection {
    pub server_name: String,
    pub transport: String,
    pub target: String,
    pub tool_names: Vec<String>,
    pub tools: Vec<McpToolDescriptor>,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpCacheState {
    pub server_name: String,
    pub ttl_seconds: u64,
    pub updated_at_unix: Option<u64>,
    pub expires_at_unix: Option<u64>,
    pub is_stale: bool,
}

pub fn local_tool_name(server_name: &str, tool_name: &str) -> String {
    let sanitize = |value: &str| {
        value
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
    };

    let base = format!("mcp__{}__{}", sanitize(server_name), sanitize(tool_name));
    if base.len() <= 64 {
        return base;
    }

    let mut hasher = DefaultHasher::new();
    server_name.hash(&mut hasher);
    tool_name.hash(&mut hasher);
    let hash = format!("{:x}", hasher.finish());
    let keep = 64usize.saturating_sub(hash.len() + 3);
    format!("{}__{}", &base[..keep], hash)
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RootConfig {
    #[serde(default)]
    mcp: McpConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct McpConfig {
    cache_ttl_seconds: Option<u64>,
    #[serde(default)]
    servers: Vec<McpServerEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct McpServerEntry {
    name: String,
    transport: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    url: Option<String>,
    enabled: Option<bool>,
}

#[derive(Debug, Clone)]
struct McpServerConfig {
    name: String,
    transport: String,
    command: Option<String>,
    args: Vec<String>,
    url: Option<String>,
    enabled: bool,
}

pub fn load_configured_servers(data_dir: &Path) -> Result<Vec<McpConfiguredServer>> {
    Ok(load_server_configs(data_dir)?
        .into_iter()
        .map(|server| {
            let target = format_server_target(&server);
            McpConfiguredServer {
                name: server.name,
                transport: server.transport,
                target,
                enabled: server.enabled,
            }
        })
        .collect())
}

pub async fn inspect_server(data_dir: &Path, server_name: &str) -> Result<McpServerInspection> {
    let server = find_server_config(data_dir, server_name)?;
    let tools = list_tools_with_config(&server).await?;
    let inspection = McpServerInspection {
        server: McpConfiguredServer {
            name: server.name.clone(),
            transport: server.transport.clone(),
            target: format_server_target(&server),
            enabled: server.enabled,
        },
        tools,
    };
    save_cached_inspection(data_dir, &inspection)?;
    Ok(inspection)
}

pub async fn list_server_tools(
    data_dir: &Path,
    server_name: &str,
) -> Result<Vec<McpToolDescriptor>> {
    let server = find_server_config(data_dir, server_name)?;
    list_tools_with_config(&server).await
}

pub async fn call_server_tool(
    data_dir: &Path,
    server_name: &str,
    tool_name: &str,
    arguments: Value,
) -> Result<String> {
    let server = find_server_config(data_dir, server_name)?;
    call_tool_with_config(&server, tool_name, arguments).await
}

pub fn load_cached_inspection(
    data_dir: &Path,
    server_name: &str,
) -> Result<Option<McpCachedInspection>> {
    let path = inspection_cache_path(data_dir, server_name);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let record = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(record))
}

pub fn list_cached_inspections(data_dir: &Path) -> Result<Vec<McpCachedInspection>> {
    let root = inspection_cache_root(data_dir);
    std::fs::create_dir_all(&root)
        .with_context(|| format!("failed to create {}", root.display()))?;
    let mut items = Vec::new();
    for entry in
        std::fs::read_dir(&root).with_context(|| format!("failed to read {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let record = serde_json::from_str::<McpCachedInspection>(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        items.push(record);
    }
    items.sort_by(|a, b| {
        b.updated_at_unix
            .cmp(&a.updated_at_unix)
            .then_with(|| a.server_name.cmp(&b.server_name))
    });
    Ok(items)
}

pub fn cache_staleness(data_dir: &Path, server_name: &str) -> Result<Option<McpCacheState>> {
    let ttl = load_cache_ttl_seconds(data_dir)?;
    let cached = load_cached_inspection(data_dir, server_name)?;
    let updated_at_unix = cached.as_ref().map(|item| item.updated_at_unix);
    let expires_at_unix = updated_at_unix.map(|value| value.saturating_add(ttl));
    Ok(Some(McpCacheState {
        server_name: server_name.to_string(),
        ttl_seconds: ttl,
        updated_at_unix,
        expires_at_unix,
        is_stale: expires_at_unix.is_none_or(|value| value <= unix_now()),
    }))
}

fn load_server_configs(data_dir: &Path) -> Result<Vec<McpServerConfig>> {
    let config = load_root_config(data_dir)?;
    Ok(config
        .mcp
        .servers
        .into_iter()
        .filter(|server| !server.name.trim().is_empty())
        .map(|server| {
            let transport = server.transport.unwrap_or_else(|| {
                if server.url.is_some() {
                    "http".to_string()
                } else {
                    "stdio".to_string()
                }
            });
            McpServerConfig {
                name: server.name,
                transport,
                command: server.command,
                args: server.args,
                url: server.url,
                enabled: server.enabled.unwrap_or(true),
            }
        })
        .collect())
}

fn load_root_config(data_dir: &Path) -> Result<RootConfig> {
    let config_path = ["config.yaml", "config.yml"]
        .iter()
        .map(|name| data_dir.join(name))
        .find(|path| path.is_file());
    let Some(config_path) = config_path else {
        return Ok(RootConfig::default());
    };

    let raw = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", config_path.display()))
}

fn find_server_config(data_dir: &Path, server_name: &str) -> Result<McpServerConfig> {
    let server = load_server_configs(data_dir)?
        .into_iter()
        .find(|server| server.name == server_name)
        .ok_or_else(|| anyhow!("mcp server `{server_name}` not found"))?;
    if !server.enabled {
        bail!("mcp server `{server_name}` is disabled");
    }
    Ok(server)
}

async fn list_tools_with_config(server: &McpServerConfig) -> Result<Vec<McpToolDescriptor>> {
    #[cfg(test)]
    if let Some(tools) = mock_list_tools(server) {
        return Ok(tools);
    }

    match server.transport.as_str() {
        "stdio" => {
            let client = StdioMcpClient::connect(server).await?;
            client.list_tools().await
        }
        other => bail!(
            "unsupported MCP transport `{other}` for server `{}`",
            server.name
        ),
    }
}

async fn call_tool_with_config(
    server: &McpServerConfig,
    tool_name: &str,
    arguments: Value,
) -> Result<String> {
    #[cfg(test)]
    if let Some(result) = mock_call_tool(server, tool_name, &arguments) {
        return result;
    }

    match server.transport.as_str() {
        "stdio" => {
            let client = StdioMcpClient::connect(server).await?;
            client.call_tool(tool_name, arguments).await
        }
        other => bail!(
            "unsupported MCP transport `{other}` for server `{}`",
            server.name
        ),
    }
}

struct StdioMcpClient {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

impl StdioMcpClient {
    async fn connect(server: &McpServerConfig) -> Result<Self> {
        let command = server
            .command
            .as_ref()
            .ok_or_else(|| anyhow!("mcp server `{}` missing command", server.name))?;
        let mut child = Command::new(command)
            .args(&server.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn MCP server `{}`", server.name))?;
        let stdin = child.stdin.take().context("failed to capture MCP stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("failed to capture MCP stdout")?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn list_tools(mut self) -> Result<Vec<McpToolDescriptor>> {
        let response = self
            .request("tools/list", json!({}))
            .await
            .context("failed to list MCP tools")?;
        let tools = response
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|item| {
                let name = item.get("name")?.as_str()?.to_string();
                let description = item
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let input_schema = item
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object" }));
                Some(McpToolDescriptor {
                    name,
                    description,
                    input_schema,
                })
            })
            .collect();
        self.shutdown().await;
        Ok(tools)
    }

    async fn call_tool(mut self, tool_name: &str, arguments: Value) -> Result<String> {
        let response = self
            .request(
                "tools/call",
                json!({
                    "name": tool_name,
                    "arguments": arguments,
                }),
            )
            .await
            .with_context(|| format!("failed to call MCP tool `{tool_name}`"))?;

        let output = if let Some(items) = response.get("content").and_then(Value::as_array) {
            let joined = items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            if joined.trim().is_empty() {
                response.to_string()
            } else {
                joined
            }
        } else {
            response.to_string()
        };
        self.shutdown().await;
        Ok(output)
    }

    async fn initialize(&mut self) -> Result<()> {
        let _ = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "crab",
                        "version": "0.1.0"
                    }
                }),
            )
            .await
            .context("failed to initialize MCP server")?;
        self.notify("notifications/initialized", json!({})).await
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        write_message(&mut self.stdin, &payload).await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        write_message(&mut self.stdin, &payload).await?;
        let response = timeout(Duration::from_secs(5), read_message(&mut self.stdout))
            .await
            .context("timed out waiting for MCP response")??;
        if response.get("id").and_then(Value::as_u64) != Some(id) {
            bail!("unexpected MCP response id");
        }
        if let Some(error) = response.get("error") {
            bail!("MCP error: {error}");
        }
        response
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("missing MCP result"))
    }

    async fn shutdown(&mut self) {
        let _ = self.stdin.shutdown().await;
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

async fn write_message(stdin: &mut tokio::process::ChildStdin, value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value).context("failed to encode MCP message")?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    stdin
        .write_all(header.as_bytes())
        .await
        .context("failed to write MCP header")?;
    stdin
        .write_all(&body)
        .await
        .context("failed to write MCP body")?;
    stdin.flush().await.context("failed to flush MCP message")
}

async fn read_message(stdout: &mut BufReader<tokio::process::ChildStdout>) -> Result<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = stdout
            .read_line(&mut line)
            .await
            .context("failed to read MCP header")?;
        if bytes == 0 {
            bail!("MCP server closed the stream");
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .context("invalid MCP content length")?,
            );
        }
    }

    let content_length = content_length.ok_or_else(|| anyhow!("missing MCP content length"))?;
    let mut body = vec![0_u8; content_length];
    stdout
        .read_exact(&mut body)
        .await
        .context("failed to read MCP body")?;
    serde_json::from_slice(&body).context("failed to decode MCP payload")
}

fn format_server_target(server: &McpServerConfig) -> String {
    server
        .url
        .clone()
        .or_else(|| {
            server.command.clone().map(|command| {
                if server.args.is_empty() {
                    command
                } else {
                    format!("{} {}", command, server.args.join(" "))
                }
            })
        })
        .unwrap_or_else(|| "unconfigured".to_string())
}

fn save_cached_inspection(data_dir: &Path, inspection: &McpServerInspection) -> Result<()> {
    let root = inspection_cache_root(data_dir);
    std::fs::create_dir_all(&root)
        .with_context(|| format!("failed to create {}", root.display()))?;
    let record = McpCachedInspection {
        server_name: inspection.server.name.clone(),
        transport: inspection.server.transport.clone(),
        target: inspection.server.target.clone(),
        tool_names: inspection
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect(),
        tools: inspection.tools.clone(),
        updated_at_unix: unix_now(),
    };
    let path = inspection_cache_path(data_dir, &inspection.server.name);
    let raw =
        serde_json::to_string_pretty(&record).context("failed to serialize MCP inspection")?;
    std::fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn inspection_cache_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("mcp-inspections")
}

fn load_cache_ttl_seconds(data_dir: &Path) -> Result<u64> {
    let config = load_root_config(data_dir)?;
    Ok(config.mcp.cache_ttl_seconds.unwrap_or(900).max(60))
}

fn inspection_cache_path(data_dir: &Path, server_name: &str) -> PathBuf {
    inspection_cache_root(data_dir).join(format!("{server_name}.json"))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
fn mock_list_tools(server: &McpServerConfig) -> Option<Vec<McpToolDescriptor>> {
    if server.command.as_deref() != Some("__mock_mcp_server__") {
        return None;
    }
    Some(vec![
        McpToolDescriptor {
            name: "search_docs".to_string(),
            description: "Search the mocked docs index.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }),
        },
        McpToolDescriptor {
            name: "read_doc".to_string(),
            description: "Read one mocked document.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
    ])
}

#[cfg(test)]
fn mock_call_tool(
    server: &McpServerConfig,
    tool_name: &str,
    arguments: &Value,
) -> Option<Result<String>> {
    if server.command.as_deref() != Some("__mock_mcp_server__") {
        return None;
    }
    let result = match tool_name {
        "search_docs" => Ok(format!(
            "mocked search results for {}",
            arguments
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or_default()
        )),
        "read_doc" => Ok(format!(
            "mocked document {}",
            arguments
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
        )),
        other => Err(anyhow!("mock MCP tool `{other}` not found")),
    };
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::{
        call_server_tool, inspect_server, list_server_tools, load_cached_inspection,
        load_configured_servers,
    };
    use serde_json::json;

    #[tokio::test]
    async fn lists_and_calls_mock_mcp_tools() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            r#"mcp:
  servers:
    - name: docs
      command: __mock_mcp_server__
"#,
        )
        .expect("write config");

        let servers = load_configured_servers(tmp.path()).expect("servers");
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "docs");

        let inspection = inspect_server(tmp.path(), "docs").await.expect("inspect");
        assert_eq!(inspection.tools.len(), 2);
        let cached = load_cached_inspection(tmp.path(), "docs")
            .expect("cached")
            .expect("record");
        assert_eq!(
            cached.tool_names,
            vec!["search_docs".to_string(), "read_doc".to_string()]
        );

        let tools = list_server_tools(tmp.path(), "docs").await.expect("tools");
        assert_eq!(tools[0].name, "search_docs");

        let output = call_server_tool(
            tmp.path(),
            "docs",
            "search_docs",
            json!({ "query": "agent loop" }),
        )
        .await
        .expect("call");
        assert!(output.contains("agent loop"));
    }
}
