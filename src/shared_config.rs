use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SharedAgentConfig {
    #[serde(default)]
    pub configured: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub aux_model: Option<String>,
}

pub fn load_shared_agent_config(data_dir: &Path) -> Result<SharedAgentConfig> {
    let Some(config_path) = existing_config_path(data_dir) else {
        return Ok(SharedAgentConfig::default());
    };

    let root = read_root_value(&config_path)?;
    let mut config = parse_shared_agent_config(&root);
    config.configured = has_any_shared_value(&config);
    Ok(config)
}

pub fn save_shared_agent_config(
    data_dir: &Path,
    config: &SharedAgentConfig,
) -> Result<SharedAgentConfig> {
    fs::create_dir_all(data_dir)
        .with_context(|| format!("failed to create config dir {}", data_dir.display()))?;

    let path = existing_config_path(data_dir).unwrap_or_else(|| data_dir.join("config.yaml"));
    let mut root = if path.is_file() {
        read_root_value(&path)?
    } else {
        Value::Mapping(Mapping::new())
    };
    let root_map = ensure_root_mapping(&mut root);

    let has_primary = [
        config.provider.as_deref(),
        config.model.as_deref(),
        config.base_url.as_deref(),
        config.api_key.as_deref(),
    ]
    .into_iter()
    .any(|value| value.is_some_and(|item| !item.trim().is_empty()));

    if has_primary {
        let mut model_map = existing_model_mapping(root_map).unwrap_or_default();
        set_optional_string(&mut model_map, "provider", config.provider.as_deref());
        set_optional_string(&mut model_map, "model", config.model.as_deref());
        set_optional_string(&mut model_map, "base_url", config.base_url.as_deref());
        set_optional_string(&mut model_map, "api_key", config.api_key.as_deref());
        root_map.insert(
            Value::String("model".to_string()),
            Value::Mapping(model_map),
        );
    } else {
        root_map.remove(Value::String("model".to_string()));
    }

    set_optional_string(root_map, "aux_model", config.aux_model.as_deref());

    let serialized = serde_yaml::to_string(&root).context("failed to serialize shared config")?;
    fs::write(&path, serialized).with_context(|| format!("failed to write {}", path.display()))?;

    let mut saved = config.clone();
    saved.configured = has_any_shared_value(&saved);
    Ok(saved)
}

fn parse_shared_agent_config(root: &Value) -> SharedAgentConfig {
    let Some(root_map) = root.as_mapping() else {
        return SharedAgentConfig::default();
    };
    let model_value = mapping_value(root_map, "model");
    let (provider, model, base_url, api_key) = match model_value {
        Some(Value::String(model)) => (None, clean(model), None, None),
        Some(Value::Mapping(model_map)) => (
            mapping_string(model_map, "provider"),
            mapping_string(model_map, "model").or_else(|| mapping_string(model_map, "default")),
            mapping_string(model_map, "base_url"),
            mapping_string(model_map, "api_key"),
        ),
        _ => (
            mapping_string(root_map, "provider"),
            mapping_string(root_map, "model"),
            mapping_string(root_map, "base_url"),
            mapping_string(root_map, "api_key"),
        ),
    };

    SharedAgentConfig {
        configured: false,
        provider,
        model,
        base_url,
        api_key,
        aux_model: mapping_string(root_map, "aux_model"),
    }
}

fn read_root_value(path: &Path) -> Result<Value> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn existing_config_path(data_dir: &Path) -> Option<PathBuf> {
    ["config.yaml", "config.yml"]
        .iter()
        .map(|name| data_dir.join(name))
        .find(|path| path.is_file())
}

fn ensure_root_mapping(root: &mut Value) -> &mut Mapping {
    if !matches!(root, Value::Mapping(_)) {
        *root = Value::Mapping(Mapping::new());
    }
    match root {
        Value::Mapping(map) => map,
        _ => unreachable!(),
    }
}

fn existing_model_mapping(root: &Mapping) -> Option<Mapping> {
    match mapping_value(root, "model") {
        Some(Value::Mapping(map)) => Some(map.clone()),
        Some(Value::String(model)) => {
            let mut map = Mapping::new();
            set_optional_string(&mut map, "model", Some(model));
            Some(map)
        }
        _ => None,
    }
}

fn mapping_value<'a>(map: &'a Mapping, key: &str) -> Option<&'a Value> {
    map.get(Value::String(key.to_string()))
}

fn mapping_string(map: &Mapping, key: &str) -> Option<String> {
    mapping_value(map, key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .and_then(clean)
}

fn set_optional_string(map: &mut Mapping, key: &str, value: Option<&str>) {
    let yaml_key = Value::String(key.to_string());
    match value.and_then(clean) {
        Some(value) => {
            map.insert(yaml_key, Value::String(value));
        }
        None => {
            map.remove(&yaml_key);
        }
    }
}

fn clean(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref().trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn has_any_shared_value(config: &SharedAgentConfig) -> bool {
    [
        config.provider.as_deref(),
        config.model.as_deref(),
        config.base_url.as_deref(),
        config.api_key.as_deref(),
        config.aux_model.as_deref(),
    ]
    .into_iter()
    .any(|value| value.is_some_and(|item| !item.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::{SharedAgentConfig, load_shared_agent_config, save_shared_agent_config};
    use std::fs;

    #[test]
    fn load_missing_config_returns_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = load_shared_agent_config(tmp.path()).expect("load config");

        assert!(!config.configured);
        assert_eq!(config.provider, None);
        assert_eq!(config.model, None);
        assert_eq!(config.base_url, None);
        assert_eq!(config.api_key, None);
        assert_eq!(config.aux_model, None);
    }

    #[test]
    fn save_shared_config_preserves_unrelated_sections() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("config.yaml");
        fs::write(
            &path,
            r#"providers:
  default: openai
custom_providers:
  local:
    base_url: http://127.0.0.1:1234/v1
model:
  provider: openai
  model: gpt-4.1-mini
  api_key: old-key
"#,
        )
        .expect("write config");

        let saved = save_shared_agent_config(
            tmp.path(),
            &SharedAgentConfig {
                configured: false,
                provider: Some("custom".to_string()),
                model: Some("gpt-5-mini".to_string()),
                base_url: Some("https://example.com/v1".to_string()),
                api_key: Some("new-key".to_string()),
                aux_model: Some("gpt-5-nano".to_string()),
            },
        )
        .expect("save config");

        assert!(saved.configured);

        let loaded = load_shared_agent_config(tmp.path()).expect("reload config");
        assert!(loaded.configured);
        assert_eq!(loaded.provider.as_deref(), Some("custom"));
        assert_eq!(loaded.model.as_deref(), Some("gpt-5-mini"));
        assert_eq!(loaded.base_url.as_deref(), Some("https://example.com/v1"));
        assert_eq!(loaded.api_key.as_deref(), Some("new-key"));
        assert_eq!(loaded.aux_model.as_deref(), Some("gpt-5-nano"));

        let raw = fs::read_to_string(&path).expect("read config");
        assert!(raw.contains("providers:"));
        assert!(raw.contains("custom_providers:"));
        assert!(raw.contains("aux_model: gpt-5-nano"));
    }
}
