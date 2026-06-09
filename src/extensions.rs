use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::cron::{CronJobSummary, load_cron_job_summaries};
use crate::mcp::{cache_staleness, load_cached_inspection};
use crate::plugins::PluginSummary;
use crate::providers::ProviderSummary;

#[derive(Debug, Clone, Serialize, Default)]
pub struct ExtensionsOverview {
    pub plugin_dirs: Vec<String>,
    pub plugins: Vec<PluginSummary>,
    pub providers: Vec<ProviderSummary>,
    pub mcp_servers: Vec<McpServerSummary>,
    pub cron_jobs: Vec<CronJobSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerSummary {
    pub name: String,
    pub transport: String,
    pub target: String,
    pub enabled: bool,
    pub cache_ttl_seconds: u64,
    pub cache_stale: bool,
    pub discovered_tools_count: usize,
    pub discovered_tool_names: Vec<String>,
    pub last_inspected_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RootConfig {
    #[serde(default)]
    plugins: PluginConfig,
    #[serde(default)]
    mcp: McpConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PluginConfig {
    #[serde(default)]
    dirs: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct McpConfig {
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

pub fn load_extensions_overview(data_dir: &Path) -> Result<ExtensionsOverview> {
    let config = load_root_config(data_dir)?;
    Ok(ExtensionsOverview {
        plugin_dirs: config.plugins.dirs.clone(),
        plugins: crate::plugins::load_plugin_catalog(data_dir)
            .map(|catalog| catalog.summaries)
            .unwrap_or_default(),
        providers: crate::providers::load_provider_summaries(data_dir).unwrap_or_default(),
        mcp_servers: config
            .mcp
            .servers
            .into_iter()
            .filter(|server| !server.name.trim().is_empty())
            .map(|server| {
                let name = server.name;
                let cached = load_cached_inspection(data_dir, &name).ok().flatten();
                let cache_staleness = cache_staleness(data_dir, &name).ok().flatten();
                McpServerSummary {
                    name,
                    transport: server.transport.unwrap_or_else(|| {
                        if server.url.as_deref().is_some() {
                            "http".to_string()
                        } else {
                            "stdio".to_string()
                        }
                    }),
                    target: server
                        .url
                        .or_else(|| {
                            server.command.map(|command| {
                                if server.args.is_empty() {
                                    command
                                } else {
                                    format!("{} {}", command, server.args.join(" "))
                                }
                            })
                        })
                        .unwrap_or_else(|| "unconfigured".to_string()),
                    enabled: server.enabled.unwrap_or(true),
                    cache_ttl_seconds: cache_staleness
                        .as_ref()
                        .map(|item| item.ttl_seconds)
                        .unwrap_or(0),
                    cache_stale: cache_staleness
                        .as_ref()
                        .map(|item| item.is_stale)
                        .unwrap_or(false),
                    discovered_tools_count: cached
                        .as_ref()
                        .map(|item| item.tools.len())
                        .unwrap_or(0),
                    discovered_tool_names: cached
                        .as_ref()
                        .map(|item| item.tool_names.iter().take(6).cloned().collect())
                        .unwrap_or_default(),
                    last_inspected_at_unix: cached.as_ref().map(|item| item.updated_at_unix),
                }
            })
            .collect(),
        cron_jobs: load_cron_job_summaries(data_dir)?,
    })
}

fn load_root_config(data_dir: &Path) -> Result<RootConfig> {
    let config_path = ["config.yaml", "config.yml"]
        .iter()
        .map(|name| data_dir.join(name))
        .find(|path| path.is_file());
    let Some(config_path) = config_path else {
        return Ok(RootConfig::default());
    };

    let raw = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", config_path.display()))
}

#[cfg(test)]
mod tests {
    use super::load_extensions_overview;
    use crate::mcp::McpCachedInspection;
    use std::fs;

    #[test]
    fn loads_plugin_mcp_and_cron_sections() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"plugins:
  dirs:
    - ~/.hermes/plugins
mcp:
  servers:
    - name: local-docs
      command: uvx
      args: [docs-server]
cron:
  jobs:
    - id: nightly-audit
      schedule: "0 2 * * *"
      prompt: "Audit the workspace and summarize risky changes."
"#,
        )
        .expect("write config");
        fs::create_dir_all(tmp.path().join("runtime").join("mcp-inspections"))
            .expect("mkdir cache");
        fs::write(
            tmp.path()
                .join("runtime")
                .join("mcp-inspections")
                .join("local-docs.json"),
            serde_json::to_string_pretty(&McpCachedInspection {
                server_name: "local-docs".to_string(),
                transport: "stdio".to_string(),
                target: "uvx docs-server".to_string(),
                tool_names: vec!["search_docs".to_string()],
                tools: vec![],
                updated_at_unix: 42,
            })
            .expect("serialize cache"),
        )
        .expect("write cache");

        let overview = load_extensions_overview(tmp.path()).expect("overview");
        assert_eq!(overview.plugin_dirs.len(), 1);
        assert_eq!(overview.providers.len(), 1);
        assert_eq!(overview.mcp_servers.len(), 1);
        assert_eq!(overview.cron_jobs.len(), 1);
        assert_eq!(overview.mcp_servers[0].transport, "stdio");
        assert_eq!(overview.mcp_servers[0].discovered_tools_count, 0);
        assert_eq!(
            overview.mcp_servers[0].discovered_tool_names,
            vec!["search_docs".to_string()]
        );
        assert_eq!(overview.mcp_servers[0].last_inspected_at_unix, Some(42));
    }
}
