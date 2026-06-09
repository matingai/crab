use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBackend {
    #[default]
    Local,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserBackend {
    #[default]
    Auto,
    Simple,
    AgentBrowser,
    ElectronDevtools,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OfficeRuntimeProfile {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeProfile {
    pub workspace_root: PathBuf,
    pub profile_id: String,
    pub profile_slug: String,
    pub display_name: String,
    pub backend: RuntimeBackend,
    pub browser_backend: BrowserBackend,
    pub env: BTreeMap<String, String>,
    pub preinstalled_tools: Vec<String>,
    pub office: OfficeRuntimeProfile,
}

impl RuntimeProfile {
    pub fn resolve(data_dir: &Path, workspace_root: &Path) -> Result<Self> {
        let root = load_root_config(data_dir, workspace_root)?;
        Ok(Self::from_root_config(workspace_root, root))
    }

    pub fn fallback(workspace_root: &Path) -> Self {
        Self::from_root_config(workspace_root, RuntimeRootConfig::default())
    }

    fn from_root_config(workspace_root: &Path, root: RuntimeRootConfig) -> Self {
        let workspace_root = canonicalize_lossy(workspace_root);
        let workspace_key = workspace_root.display().to_string();
        let runtime = root.runtime.unwrap_or_default();
        let defaults = runtime.defaults.unwrap_or_default();
        let workspace_override = runtime
            .workspaces
            .unwrap_or_default()
            .into_iter()
            .find(|item| workspace_matches(item.path.as_deref(), &workspace_root));

        let backend = RuntimeBackend::Local;
        let browser_backend = env_value("HERMES_RS_BROWSER_BACKEND")
            .and_then(parse_browser_backend)
            .or_else(|| {
                workspace_override
                    .as_ref()
                    .and_then(|item| item.browser_backend)
            })
            .or(defaults.browser_backend)
            .unwrap_or_default();

        let display_name = workspace_override
            .as_ref()
            .and_then(|item| clean(item.name.clone()))
            .unwrap_or_else(|| workspace_display_name(&workspace_root));
        let profile_slug = clean(
            workspace_override
                .as_ref()
                .and_then(|item| item.profile_slug.clone()),
        )
        .unwrap_or_else(|| slug_for_workspace(&workspace_root, &display_name));

        let mut env_map = BTreeMap::new();
        if let Some(default_env) = defaults.env {
            merge_env_map(&mut env_map, default_env);
        }
        if let Some(override_env) = workspace_override
            .as_ref()
            .and_then(|item| item.env.clone())
        {
            merge_env_map(&mut env_map, override_env);
        }

        let uv_default_index = env_value("UV_INDEX_URL")
            .or_else(|| env_value("HERMES_RS_UV_INDEX_URL"))
            .or_else(|| {
                workspace_override
                    .as_ref()
                    .and_then(|item| clean(item.uv_index_url.clone()))
            })
            .or_else(|| clean(defaults.uv_index_url.clone()));
        if let Some(value) = uv_default_index {
            env_map.insert("UV_INDEX_URL".to_string(), value);
        }

        let bun_registry = env_value("BUN_REGISTRY")
            .or_else(|| env_value("HERMES_RS_BUN_REGISTRY"))
            .or_else(|| {
                workspace_override
                    .as_ref()
                    .and_then(|item| clean(item.bun_registry.clone()))
            })
            .or_else(|| clean(defaults.bun_registry.clone()));
        if let Some(value) = bun_registry.as_ref() {
            env_map.insert("NPM_CONFIG_REGISTRY".to_string(), value.clone());
        }

        let office_defaults = defaults.office.unwrap_or_default();
        let office_override = workspace_override
            .as_ref()
            .and_then(|item| item.office.clone())
            .unwrap_or_default();
        let office_enabled = env_flag("HERMES_RS_OFFICE_ENABLED")
            .or(office_override.enabled)
            .or(office_defaults.enabled)
            .unwrap_or(true);

        let preinstalled_tools = ordered_unique_strings(
            defaults
                .preinstalled_tools
                .into_iter()
                .flatten()
                .chain(
                    workspace_override
                        .as_ref()
                        .and_then(|item| item.preinstalled_tools.clone())
                        .into_iter()
                        .flatten(),
                )
                .chain(
                    ["python3", "uv", "bun", "agent-browser"]
                        .into_iter()
                        .map(str::to_string),
                ),
        );

        Self {
            workspace_root,
            profile_id: workspace_key,
            profile_slug,
            display_name,
            backend,
            browser_backend,
            env: env_map,
            preinstalled_tools,
            office: OfficeRuntimeProfile {
                enabled: office_enabled,
            },
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct RuntimeRootConfig {
    #[serde(default)]
    runtime: Option<RuntimeConfigFile>,
}

#[derive(Debug, Default, Deserialize)]
struct RuntimeConfigFile {
    #[serde(default)]
    defaults: Option<RuntimeDefaultsFile>,
    #[serde(default)]
    workspaces: Option<Vec<WorkspaceRuntimeFile>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RuntimeDefaultsFile {
    #[serde(default)]
    browser_backend: Option<BrowserBackend>,
    #[serde(default, alias = "uv_default_index")]
    uv_index_url: Option<String>,
    #[serde(default)]
    bun_registry: Option<String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    office: Option<OfficeRuntimeFile>,
    #[serde(default)]
    preinstalled_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct WorkspaceRuntimeFile {
    #[serde(default, alias = "workspace_root")]
    path: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    profile_slug: Option<String>,
    #[serde(default)]
    browser_backend: Option<BrowserBackend>,
    #[serde(default, alias = "uv_default_index")]
    uv_index_url: Option<String>,
    #[serde(default)]
    bun_registry: Option<String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    office: Option<OfficeRuntimeFile>,
    #[serde(default)]
    preinstalled_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct OfficeRuntimeFile {
    #[serde(default)]
    enabled: Option<bool>,
}

fn load_root_config(data_dir: &Path, workspace_root: &Path) -> Result<RuntimeRootConfig> {
    let config_path = config_search_paths(data_dir, workspace_root)
        .into_iter()
        .find(|path| path.is_file());
    let Some(config_path) = config_path else {
        return Ok(RuntimeRootConfig::default());
    };

    let raw = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", config_path.display()))
}

fn config_search_paths(data_dir: &Path, workspace_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for name in ["config.yaml", "config.yml"] {
        paths.push(data_dir.join(name));
    }

    let workspace_root = canonicalize_lossy(workspace_root);
    for ancestor in workspace_root.ancestors() {
        for name in ["config.yaml", "config.yml"] {
            let candidate = ancestor.join(".hermes-agent-rs").join(name);
            if paths.iter().any(|existing| existing == &candidate) {
                continue;
            }
            paths.push(candidate);
        }
    }

    paths
}

fn workspace_matches(rule: Option<&str>, workspace_root: &Path) -> bool {
    let Some(rule) = rule.and_then(|value| clean(Some(value.to_string()))) else {
        return false;
    };
    let workspace_root = canonicalize_lossy(workspace_root);
    let workspace_display = workspace_root.display().to_string();
    if rule == workspace_display {
        return true;
    }

    let rule_path = PathBuf::from(&rule);
    if rule_path.is_absolute() {
        return canonicalize_lossy(&rule_path) == workspace_root;
    }

    if workspace_root.file_name().and_then(|value| value.to_str()) == Some(rule.as_str()) {
        return true;
    }

    let workspace_parts = normalized_path_parts(&workspace_root);
    let rule_parts = normalized_path_parts(&rule_path);
    workspace_parts.ends_with(&rule_parts)
}

fn workspace_display_name(workspace_root: &Path) -> String {
    workspace_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "workspace".to_string())
}

fn slug_for_workspace(workspace_root: &Path, display_name: &str) -> String {
    let mut hasher = DefaultHasher::new();
    workspace_root.display().to_string().hash(&mut hasher);
    let hash = format!("{:08x}", hasher.finish());
    let slug = display_name
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        format!("workspace-{hash}")
    } else {
        format!("{slug}-{hash}")
    }
}

fn ordered_unique_strings<I>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut items = Vec::new();
    for value in values {
        let Some(value) = clean(Some(value)) else {
            continue;
        };
        if items.iter().any(|item| item == &value) {
            continue;
        }
        items.push(value);
    }
    items
}

fn merge_env_map(target: &mut BTreeMap<String, String>, incoming: BTreeMap<String, String>) {
    for (key, value) in incoming {
        let Some(key) = clean(Some(key)) else {
            continue;
        };
        let Some(value) = clean(Some(value)) else {
            continue;
        };
        target.insert(key, value);
    }
}

fn parse_browser_backend(value: String) -> Option<BrowserBackend> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(BrowserBackend::Auto),
        "simple" => Some(BrowserBackend::Simple),
        "agent_browser" | "agent-browser" => Some(BrowserBackend::AgentBrowser),
        "electron_devtools" | "electron-devtools" | "electron" | "devtools" => {
            Some(BrowserBackend::ElectronDevtools)
        }
        _ => None,
    }
}

fn env_value(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_flag(name: &str) -> Option<bool> {
    env::var(name).ok().and_then(|value| match value.trim() {
        "1" | "true" | "TRUE" | "yes" | "on" => Some(true),
        "0" | "false" | "FALSE" | "no" | "off" => Some(false),
        _ => None,
    })
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn canonicalize_lossy(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn normalized_path_parts(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{BrowserBackend, RuntimeBackend, RuntimeProfile};
    use std::fs;

    #[test]
    fn fallback_profile_is_local_and_office_ready() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profile = RuntimeProfile::fallback(tmp.path());
        assert_eq!(profile.backend, RuntimeBackend::Local);
        assert_eq!(profile.browser_backend, BrowserBackend::Auto);
        assert!(profile.office.enabled);
        assert!(profile.preinstalled_tools.iter().any(|item| item == "uv"));
        assert!(
            profile
                .preinstalled_tools
                .iter()
                .any(|item| item == "agent-browser")
        );
    }

    #[test]
    fn resolves_workspace_override_for_local_runtime() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path().join("repo-a");
        let data_dir = workspace.join(".hermes-agent-rs");
        fs::create_dir_all(&data_dir).expect("data dir");
        fs::write(
            data_dir.join("config.yaml"),
            format!(
                r#"
runtime:
  defaults:
    browser_backend: agent_browser
    uv_index_url: https://mirror.example/uv/simple
    bun_registry: https://mirror.example/npm/
    env:
      FOO: bar
    office:
      enabled: false
  workspaces:
    - path: "{}"
      name: Repo A
      browser_backend: simple
      env:
        BAR: baz
      office:
        enabled: true
"#,
                workspace.display()
            ),
        )
        .expect("config");

        fs::create_dir_all(&workspace).expect("workspace");
        let profile = RuntimeProfile::resolve(&data_dir, &workspace).expect("profile");
        assert_eq!(profile.backend, RuntimeBackend::Local);
        assert_eq!(profile.browser_backend, BrowserBackend::Simple);
        assert_eq!(
            profile.env.get("UV_INDEX_URL").map(String::as_str),
            Some("https://mirror.example/uv/simple")
        );
        assert_eq!(
            profile.env.get("NPM_CONFIG_REGISTRY").map(String::as_str),
            Some("https://mirror.example/npm/")
        );
        assert_eq!(profile.env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(profile.env.get("BAR").map(String::as_str), Some("baz"));
        assert!(profile.office.enabled);
    }

    #[test]
    fn inherits_runtime_config_from_ancestor_workspace() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo_root = tmp.path().join("repo-root");
        let workspace = repo_root.join("desktop-shell");
        let root_data_dir = repo_root.join(".hermes-agent-rs");
        let child_data_dir = workspace.join(".hermes-agent-rs");

        fs::create_dir_all(&root_data_dir).expect("root data dir");
        fs::create_dir_all(&child_data_dir).expect("child data dir");
        fs::create_dir_all(&workspace).expect("workspace");
        fs::write(
            root_data_dir.join("config.yaml"),
            r#"
runtime:
  defaults:
    browser_backend: agent_browser
"#,
        )
        .expect("config");

        let profile = RuntimeProfile::resolve(&child_data_dir, &workspace).expect("profile");
        assert_eq!(profile.backend, RuntimeBackend::Local);
        assert_eq!(profile.browser_backend, BrowserBackend::AgentBrowser);
    }
}
