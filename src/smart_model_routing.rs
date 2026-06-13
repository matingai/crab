use anyhow::Result;
use serde_yaml::{Mapping, Value};
use std::env;
use std::fs;
use std::path::Path;

use crate::providers::{
    ProviderResolutionRequest, ResolvedProviderConfig, resolve_runtime_provider,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartModelRoutingConfig {
    pub max_simple_chars: usize,
    pub max_simple_words: usize,
    pub cheap_model: SmartModelTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartModelTarget {
    pub provider: Option<String>,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub api_mode: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedTurnRoute {
    pub runtime: ResolvedProviderConfig,
    pub reason: String,
}

const DEFAULT_MAX_SIMPLE_CHARS: usize = 160;
const DEFAULT_MAX_SIMPLE_WORDS: usize = 28;
const COMPLEX_KEYWORDS: &[&str] = &[
    "debug",
    "debugging",
    "implement",
    "implementation",
    "refactor",
    "patch",
    "traceback",
    "stacktrace",
    "exception",
    "error",
    "analyze",
    "analysis",
    "investigate",
    "architecture",
    "design",
    "compare",
    "benchmark",
    "optimize",
    "optimise",
    "review",
    "terminal",
    "shell",
    "tool",
    "tools",
    "pytest",
    "test",
    "tests",
    "plan",
    "planning",
    "delegate",
    "subagent",
    "cron",
    "docker",
    "kubernetes",
];

pub fn load_smart_model_routing(data_dir: &Path) -> Result<Option<SmartModelRoutingConfig>> {
    let file_config = load_file_config(data_dir)?;
    Ok(apply_env_overrides(file_config))
}

pub fn resolve_turn_route(
    data_dir: &Path,
    user_message: &str,
    routing_config: Option<&SmartModelRoutingConfig>,
) -> Result<Option<ResolvedTurnRoute>> {
    let Some(config) = routing_config else {
        return Ok(None);
    };
    if !is_simple_turn(user_message, config) {
        return Ok(None);
    }

    let explicit_api_key = config.cheap_model.api_key.clone().or_else(|| {
        config
            .cheap_model
            .api_key_env
            .as_deref()
            .and_then(env::var_os)
            .and_then(|value| value.into_string().ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    });

    let runtime = resolve_runtime_provider(
        data_dir,
        ProviderResolutionRequest {
            provider: config.cheap_model.provider.clone(),
            model: Some(config.cheap_model.model.clone()),
            base_url: config.cheap_model.base_url.clone(),
            api_key: explicit_api_key,
            api_mode: config.cheap_model.api_mode.clone(),
        },
    )?;

    Ok(Some(ResolvedTurnRoute {
        reason: "simple_turn".to_string(),
        runtime,
    }))
}

fn load_file_config(data_dir: &Path) -> Result<Option<SmartModelRoutingConfig>> {
    let config_path = ["config.yaml", "config.yml"]
        .iter()
        .map(|name| data_dir.join(name))
        .find(|path| path.is_file());
    let Some(config_path) = config_path else {
        return Ok(None);
    };

    let raw = fs::read_to_string(&config_path)?;
    let root: Value = serde_yaml::from_str(&raw)?;
    let Some(root_map) = root.as_mapping() else {
        return Ok(None);
    };
    let Some(section) = mapping_value(root_map, "smart_model_routing") else {
        return Ok(None);
    };
    Ok(parse_config_value(section))
}

fn parse_config_value(value: &Value) -> Option<SmartModelRoutingConfig> {
    let map = value.as_mapping()?;
    if matches!(mapping_bool(map, "enabled"), Some(false)) {
        return None;
    }
    let cheap_model = parse_target(mapping_value(map, "cheap_model")?)?;
    Some(SmartModelRoutingConfig {
        max_simple_chars: mapping_usize(map, "max_simple_chars")
            .unwrap_or(DEFAULT_MAX_SIMPLE_CHARS),
        max_simple_words: mapping_usize(map, "max_simple_words")
            .unwrap_or(DEFAULT_MAX_SIMPLE_WORDS),
        cheap_model,
    })
}

fn parse_target(value: &Value) -> Option<SmartModelTarget> {
    let map = value.as_mapping()?;
    let provider = mapping_string(map, "provider");
    let base_url = mapping_string(map, "base_url");
    let model = mapping_string(map, "model")?;
    if provider.is_none() && base_url.is_none() {
        return None;
    }
    Some(SmartModelTarget {
        provider,
        model,
        base_url,
        api_key: mapping_string(map, "api_key"),
        api_key_env: mapping_string(map, "api_key_env"),
        api_mode: mapping_string(map, "api_mode"),
    })
}

fn apply_env_overrides(
    file_config: Option<SmartModelRoutingConfig>,
) -> Option<SmartModelRoutingConfig> {
    let env_enabled = env_bool("HERMES_RS_SMART_MODEL_ROUTING_ENABLED");
    if matches!(env_enabled, Some(false)) {
        return None;
    }
    let had_file_config = file_config.is_some();

    let mut config = file_config.unwrap_or(SmartModelRoutingConfig {
        max_simple_chars: DEFAULT_MAX_SIMPLE_CHARS,
        max_simple_words: DEFAULT_MAX_SIMPLE_WORDS,
        cheap_model: SmartModelTarget {
            provider: None,
            model: String::new(),
            base_url: None,
            api_key: None,
            api_key_env: None,
            api_mode: None,
        },
    });

    if let Some(value) = env_usize("HERMES_RS_SMART_MODEL_MAX_SIMPLE_CHARS") {
        config.max_simple_chars = value;
    }
    if let Some(value) = env_usize("HERMES_RS_SMART_MODEL_MAX_SIMPLE_WORDS") {
        config.max_simple_words = value;
    }
    if let Some(value) = env_string("HERMES_RS_SMART_MODEL_PROVIDER") {
        config.cheap_model.provider = Some(value);
    }
    if let Some(value) = env_string("HERMES_RS_SMART_MODEL") {
        config.cheap_model.model = value;
    }
    if let Some(value) = env_string("HERMES_RS_SMART_MODEL_BASE_URL") {
        config.cheap_model.base_url = Some(value);
    }
    if let Some(value) = env_string("HERMES_RS_SMART_MODEL_API_KEY") {
        config.cheap_model.api_key = Some(value);
    }
    if let Some(value) = env_string("HERMES_RS_SMART_MODEL_API_KEY_ENV") {
        config.cheap_model.api_key_env = Some(value);
    }
    if let Some(value) = env_string("HERMES_RS_SMART_MODEL_API_MODE") {
        config.cheap_model.api_mode = Some(value);
    }

    let target_valid = !config.cheap_model.model.trim().is_empty()
        && (config.cheap_model.provider.is_some() || config.cheap_model.base_url.is_some());
    if !target_valid {
        return None;
    }

    if matches!(env_enabled, Some(true)) || had_file_config {
        return Some(config);
    }
    None
}

fn is_simple_turn(user_message: &str, config: &SmartModelRoutingConfig) -> bool {
    let text = user_message.trim();
    if text.is_empty() {
        return false;
    }
    if text.len() > config.max_simple_chars {
        return false;
    }
    if text.split_whitespace().count() > config.max_simple_words {
        return false;
    }
    if text.lines().count() > 2 {
        return false;
    }
    if text.contains("```") || text.contains('`') {
        return false;
    }

    let lowered = text.to_lowercase();
    if lowered.contains("http://") || lowered.contains("https://") || lowered.contains("www.") {
        return false;
    }

    let words = lowered
        .split_whitespace()
        .map(|token| token.trim_matches(|ch: char| !ch.is_alphanumeric()))
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    !words
        .iter()
        .any(|token| COMPLEX_KEYWORDS.iter().any(|keyword| token == keyword))
}

fn mapping_value<'a>(map: &'a Mapping, key: &str) -> Option<&'a Value> {
    map.get(Value::String(key.to_string()))
}

fn mapping_string(map: &Mapping, key: &str) -> Option<String> {
    mapping_value(map, key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn mapping_bool(map: &Mapping, key: &str) -> Option<bool> {
    mapping_value(map, key).and_then(Value::as_bool)
}

fn mapping_usize(map: &Mapping, key: &str) -> Option<usize> {
    mapping_value(map, key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

fn env_string(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_bool(name: &str) -> Option<bool> {
    env::var(name)
        .ok()
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "on"))
}

fn env_usize(name: &str) -> Option<usize> {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::{
        SmartModelRoutingConfig, SmartModelTarget, is_simple_turn, load_smart_model_routing,
        resolve_turn_route,
    };

    #[test]
    fn rejects_complex_prompts_for_cheap_route() {
        let config = SmartModelRoutingConfig {
            max_simple_chars: 160,
            max_simple_words: 28,
            cheap_model: SmartModelTarget {
                provider: Some("openai".to_string()),
                model: "gpt-4.1-nano".to_string(),
                base_url: None,
                api_key: None,
                api_key_env: None,
                api_mode: None,
            },
        };

        assert!(is_simple_turn("summarize this", &config));
        assert!(!is_simple_turn(
            "debug this stacktrace and patch the error",
            &config
        ));
        assert!(!is_simple_turn(
            "check https://example.com and compare outputs",
            &config
        ));
    }

    #[test]
    fn loads_routing_config_from_yaml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            r#"smart_model_routing:
  enabled: true
  max_simple_chars: 96
  max_simple_words: 12
  cheap_model:
    provider: openai
    model: gpt-4.1-nano
    api_mode: responses
"#,
        )
        .expect("write config");

        let config = load_smart_model_routing(tmp.path())
            .expect("load")
            .expect("routing config");
        assert_eq!(config.max_simple_chars, 96);
        assert_eq!(config.max_simple_words, 12);
        assert_eq!(config.cheap_model.provider.as_deref(), Some("openai"));
        assert_eq!(config.cheap_model.model, "gpt-4.1-nano");
        assert_eq!(config.cheap_model.api_mode.as_deref(), Some("responses"));
    }

    #[test]
    fn resolves_simple_turn_route() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = SmartModelRoutingConfig {
            max_simple_chars: 160,
            max_simple_words: 28,
            cheap_model: SmartModelTarget {
                provider: None,
                model: "gpt-4.1-nano".to_string(),
                base_url: Some("mock://final-response".to_string()),
                api_key: None,
                api_key_env: None,
                api_mode: Some("responses".to_string()),
            },
        };

        let route = resolve_turn_route(tmp.path(), "summarize this", Some(&config))
            .expect("resolve")
            .expect("route");
        assert_eq!(route.reason, "simple_turn");
        assert_eq!(route.runtime.model, "gpt-4.1-nano");
        assert_eq!(route.runtime.base_url, "mock://final-response");
        assert_eq!(route.runtime.api_mode.as_str(), "responses");
    }
}
