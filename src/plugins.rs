use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

#[derive(Debug, Clone, Serialize)]
pub struct PluginSummary {
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: String,
    pub enabled: bool,
    pub tool_names: Vec<String>,
    pub hook_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PluginToolSpec {
    pub plugin_name: String,
    pub tool_name: String,
    pub description: String,
    pub schema: Value,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct PluginHookSpec {
    pub plugin_name: String,
    pub event: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Default)]
pub struct PluginHookRegistry {
    hooks: BTreeMap<String, Vec<PluginHookSpec>>,
}

impl PluginHookRegistry {
    pub async fn run(&self, event: &str, payload: &Value) -> Vec<String> {
        let mut outputs = Vec::new();
        let Some(hooks) = self.hooks.get(event) else {
            return outputs;
        };

        for hook in hooks {
            if let Ok(output) = run_plugin_process(
                &hook.command,
                &hook.args,
                &hook.cwd,
                payload,
                hook.timeout_seconds,
                &hook.plugin_name,
            )
            .await
            {
                let trimmed = output.trim();
                if !trimmed.is_empty() {
                    outputs.push(trimmed.to_string());
                }
            }
        }
        outputs
    }

    pub fn hook_count(&self) -> usize {
        self.hooks.values().map(Vec::len).sum()
    }

    pub fn insert(&mut self, spec: PluginHookSpec) {
        self.hooks.entry(spec.event.clone()).or_default().push(spec);
    }
}

#[derive(Debug, Clone, Default)]
pub struct PluginCatalog {
    pub summaries: Vec<PluginSummary>,
    pub tools: Vec<PluginToolSpec>,
    pub hooks: PluginHookRegistry,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RootConfig {
    #[serde(default)]
    plugins: PluginConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PluginConfig {
    #[serde(default)]
    dirs: Vec<String>,
    #[serde(default)]
    disabled: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PluginManifestFile {
    name: String,
    version: Option<String>,
    description: Option<String>,
    #[serde(default)]
    tools: Vec<PluginToolEntry>,
    #[serde(default)]
    hooks: Vec<PluginHookEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PluginToolEntry {
    name: String,
    description: Option<String>,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    schema: Option<Value>,
    timeout_seconds: Option<u64>,
    enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PluginHookEntry {
    event: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    timeout_seconds: Option<u64>,
    enabled: Option<bool>,
}

pub fn load_plugin_catalog(data_dir: &Path) -> Result<PluginCatalog> {
    let root = load_root_config(data_dir)?;
    let disabled = root
        .plugins
        .disabled
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let mut catalog = PluginCatalog::default();

    for dir in root.plugins.dirs {
        let dir = expand_home(&dir);
        if !dir.is_dir() {
            continue;
        }
        let mut entries = fs::read_dir(&dir)
            .with_context(|| format!("failed to read plugin dir {}", dir.display()))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let plugin_root = entry.path();
            if !plugin_root.is_dir() {
                continue;
            }
            let manifest_path = plugin_root.join("plugin.yaml");
            if !manifest_path.is_file() {
                continue;
            }
            let raw = fs::read_to_string(&manifest_path)
                .with_context(|| format!("failed to read {}", manifest_path.display()))?;
            let manifest: PluginManifestFile = serde_yaml::from_str(&raw)
                .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
            if manifest.name.trim().is_empty() {
                continue;
            }

            let enabled = !disabled.contains(&manifest.name);
            let mut tool_names = Vec::new();
            let mut hook_names = Vec::new();

            for tool in manifest.tools {
                if !enabled || !tool.enabled.unwrap_or(true) || tool.name.trim().is_empty() {
                    continue;
                }
                tool_names.push(tool.name.clone());
                catalog.tools.push(PluginToolSpec {
                    plugin_name: manifest.name.clone(),
                    tool_name: tool.name,
                    description: tool.description.unwrap_or_default(),
                    schema: tool.schema.unwrap_or_else(default_tool_schema),
                    command: resolve_plugin_command(&plugin_root, &tool.command),
                    args: tool.args,
                    cwd: plugin_root.clone(),
                    timeout_seconds: tool.timeout_seconds.unwrap_or(30),
                });
            }

            for hook in manifest.hooks {
                if !enabled || !hook.enabled.unwrap_or(true) || hook.event.trim().is_empty() {
                    continue;
                }
                hook_names.push(hook.event.clone());
                catalog.hooks.insert(PluginHookSpec {
                    plugin_name: manifest.name.clone(),
                    event: hook.event,
                    command: resolve_plugin_command(&plugin_root, &hook.command),
                    args: hook.args,
                    cwd: plugin_root.clone(),
                    timeout_seconds: hook.timeout_seconds.unwrap_or(15),
                });
            }

            catalog.summaries.push(PluginSummary {
                name: manifest.name,
                version: manifest.version.unwrap_or_default(),
                description: manifest.description.unwrap_or_default(),
                path: plugin_root.display().to_string(),
                enabled,
                tool_names,
                hook_names,
            });
        }
    }

    catalog.summaries.sort_by(|a, b| a.name.cmp(&b.name));
    catalog.tools.sort_by(|a, b| a.tool_name.cmp(&b.tool_name));
    Ok(catalog)
}

pub async fn execute_plugin_tool(spec: &PluginToolSpec, payload: &Value) -> Result<String> {
    run_plugin_process(
        &spec.command,
        &spec.args,
        &spec.cwd,
        payload,
        spec.timeout_seconds,
        &spec.plugin_name,
    )
    .await
}

async fn run_plugin_process(
    command: &Path,
    args: &[String],
    cwd: &Path,
    payload: &Value,
    timeout_seconds: u64,
    plugin_name: &str,
) -> Result<String> {
    if !command.exists() && command.components().count() > 1 {
        bail!(
            "plugin `{plugin_name}` command does not exist: {}",
            command.display()
        );
    }

    let payload_raw = serde_json::to_string(payload).context("failed to encode plugin payload")?;
    let mut child = Command::new(command)
        .args(args)
        .current_dir(cwd)
        .env("HERMES_PLUGIN_NAME", plugin_name)
        .env("HERMES_PLUGIN_PAYLOAD", &payload_raw)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn plugin command {}", command.display()))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload_raw.as_bytes())
            .await
            .context("failed to write plugin stdin")?;
        stdin.shutdown().await.ok();
    }

    let output = timeout(
        Duration::from_secs(timeout_seconds.max(1)),
        child.wait_with_output(),
    )
    .await
    .with_context(|| format!("plugin `{plugin_name}` timed out after {timeout_seconds}s"))?
    .context("failed to wait for plugin process")?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let detail = if stderr.is_empty() { stdout } else { stderr };
        bail!(
            "plugin `{plugin_name}` command failed: {}",
            detail.trim().trim_matches('"')
        );
    }

    Ok(stdout)
}

fn resolve_plugin_command(root: &Path, command: &str) -> PathBuf {
    let command_path = Path::new(command);
    if command_path.is_absolute()
        || command.contains(std::path::MAIN_SEPARATOR)
        || command.starts_with('.')
    {
        return root.join(command_path);
    }
    PathBuf::from(command)
}

fn default_tool_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
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

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn home_dir() -> PathBuf {
    env::var("HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"))
}

#[cfg(test)]
mod tests {
    use super::{PluginHookRegistry, execute_plugin_tool, load_plugin_catalog};
    use serde_json::json;
    use std::fs;

    #[tokio::test]
    async fn loads_and_executes_directory_plugins() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plugins_root = tmp.path().join("plugins");
        let demo_root = plugins_root.join("demo");
        fs::create_dir_all(&demo_root).expect("mkdir");
        fs::write(
            tmp.path().join("config.yaml"),
            format!("plugins:\n  dirs:\n    - {}\n", plugins_root.display()),
        )
        .expect("write config");
        fs::write(
            demo_root.join("plugin.yaml"),
            r#"name: demo
description: Demo plugin
tools:
  - name: echo_plugin
    description: Echo JSON
    command: ./echo.sh
hooks:
  - event: pre_tool_call
    command: ./hook.sh
"#,
        )
        .expect("write manifest");
        fs::write(demo_root.join("echo.sh"), "#!/bin/sh\ncat\n").expect("write tool");
        fs::write(demo_root.join("hook.sh"), "#!/bin/sh\necho hook-ran\n").expect("write hook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(demo_root.join("echo.sh"), fs::Permissions::from_mode(0o755))
                .expect("chmod");
            fs::set_permissions(demo_root.join("hook.sh"), fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }

        let catalog = load_plugin_catalog(tmp.path()).expect("catalog");
        assert_eq!(catalog.summaries.len(), 1);
        assert_eq!(catalog.tools.len(), 1);
        assert_eq!(catalog.hooks.hook_count(), 1);

        let output = execute_plugin_tool(&catalog.tools[0], &json!({ "hello": "world" }))
            .await
            .expect("tool output");
        assert!(output.contains("hello"));

        let hook_output =
            PluginHookRegistry::run(&catalog.hooks, "pre_tool_call", &json!({})).await;
        assert_eq!(hook_output, vec!["hook-ran".to_string()]);
    }
}
