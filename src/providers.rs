use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde_yaml::{Mapping, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::llm::ApiMode;

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const DEFAULT_NOUS_BASE_URL: &str = "https://inference-api.nousresearch.com/v1";
const DEFAULT_QWEN_BASE_URL: &str = "https://portal.qwen.ai/v1";
const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_GITHUB_MODELS_BASE_URL: &str = "https://models.inference.ai.azure.com";
const DEFAULT_COPILOT_ACP_BASE_URL: &str = "acp://copilot";

#[derive(Debug, Clone, Serialize)]
pub struct ProviderSummary {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub enabled: bool,
    pub is_default: bool,
    pub model: String,
    pub base_url: String,
    pub api_mode: String,
    pub auth_source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedProviderConfig {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub api_mode: ApiMode,
    pub auth_source: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderResolutionRequest {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub api_mode: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct RootConfig {
    providers_default: Option<String>,
    configured_provider: Option<String>,
    configured_model: Option<String>,
    configured_base_url: Option<String>,
    configured_api_key: Option<String>,
    configured_api_mode: Option<String>,
    profiles: Vec<ProviderProfile>,
}

#[derive(Debug, Clone)]
struct ProviderProfile {
    id: String,
    label: String,
    kind: String,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    api_key_envs: Vec<String>,
    enabled: bool,
    api_mode_hint: Option<String>,
}

impl RootConfig {
    fn active_provider_id(&self) -> Option<&str> {
        self.configured_provider
            .as_deref()
            .or(self.providers_default.as_deref())
    }

    fn has_runtime_defaults(&self) -> bool {
        self.active_provider_id().is_some()
            || self.configured_base_url.is_some()
            || self.configured_model.is_some()
            || self.configured_api_key.is_some()
    }
}

pub fn load_provider_summaries(data_dir: &Path) -> Result<Vec<ProviderSummary>> {
    let config = load_root_config(data_dir)?;

    if config.profiles.is_empty() {
        if config.has_runtime_defaults() {
            let resolved =
                resolve_runtime_provider(data_dir, ProviderResolutionRequest::default())?;
            return Ok(vec![summary_from_resolved(resolved, true)]);
        }
        return Ok(vec![build_legacy_summary()]);
    }

    let default_id = config.active_provider_id().map(ToString::to_string);
    let mut summaries = config
        .profiles
        .iter()
        .map(|profile| {
            let resolved = resolve_from_profile(
                profile,
                summary_request_for_profile(profile, &config),
                default_id.as_deref() == Some(profile.id.as_str()),
            )?;
            Ok(ProviderSummary {
                id: resolved.id,
                label: resolved.label,
                kind: resolved.kind,
                enabled: profile.enabled,
                is_default: default_id.as_deref() == Some(profile.id.as_str()),
                model: resolved.model,
                base_url: resolved.base_url,
                api_mode: resolved.api_mode.as_str().to_string(),
                auth_source: resolved.auth_source,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    if config.has_runtime_defaults() {
        let resolved = resolve_runtime_provider(data_dir, ProviderResolutionRequest::default())?;
        let already_listed = summaries
            .iter()
            .any(|item| item.id == resolved.id || item.base_url == resolved.base_url);
        if !already_listed {
            summaries.push(summary_from_resolved(
                resolved,
                default_id.as_deref().is_some(),
            ));
        }
    }

    summaries.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.label.cmp(&right.label))
    });
    Ok(summaries)
}

pub fn resolve_runtime_provider(
    data_dir: &Path,
    request: ProviderResolutionRequest,
) -> Result<ResolvedProviderConfig> {
    let config = load_root_config(data_dir)?;

    if let Some(base_url) = clean(request.base_url.clone()) {
        return resolve_direct_endpoint(
            "direct",
            "Direct Endpoint",
            request
                .model
                .clone()
                .or_else(|| config.configured_model.clone()),
            base_url,
            request
                .api_key
                .clone()
                .or_else(|| config.configured_api_key.clone()),
            clean(request.api_mode.clone())
                .as_deref()
                .or(config.configured_api_mode.as_deref()),
        );
    }

    let provider_id = clean(request.provider.clone())
        .or_else(|| config.active_provider_id().map(ToString::to_string));

    if let Some(provider_id) = provider_id {
        if let Some(profile) = config.profiles.iter().find(|item| item.id == provider_id) {
            return resolve_from_profile(
                profile,
                ProviderResolutionRequest {
                    provider: Some(provider_id.clone()),
                    model: clean(request.model.clone()).or_else(|| {
                        if config.active_provider_id() == Some(provider_id.as_str()) {
                            config.configured_model.clone()
                        } else {
                            None
                        }
                    }),
                    base_url: clean(request.base_url.clone()),
                    api_key: clean(request.api_key.clone()).or_else(|| {
                        if config.active_provider_id() == Some(provider_id.as_str()) {
                            config.configured_api_key.clone()
                        } else {
                            None
                        }
                    }),
                    api_mode: clean(request.api_mode.clone()).or_else(|| {
                        if config.active_provider_id() == Some(provider_id.as_str()) {
                            config.configured_api_mode.clone()
                        } else {
                            None
                        }
                    }),
                },
                true,
            );
        }

        if provider_id == "custom" {
            if let Some(base_url) = config.configured_base_url.clone() {
                return resolve_direct_endpoint(
                    "direct",
                    "Direct Endpoint",
                    clean(request.model.clone()).or_else(|| config.configured_model.clone()),
                    base_url,
                    clean(request.api_key.clone()).or_else(|| config.configured_api_key.clone()),
                    clean(request.api_mode.clone())
                        .as_deref()
                        .or(config.configured_api_mode.as_deref()),
                );
            }
        }

        return resolve_known_provider(&provider_id, &config, request);
    }

    if let Some(base_url) = config.configured_base_url.clone() {
        return resolve_direct_endpoint(
            "direct",
            "Direct Endpoint",
            clean(request.model.clone()).or_else(|| config.configured_model.clone()),
            base_url,
            clean(request.api_key.clone()).or_else(|| config.configured_api_key.clone()),
            clean(request.api_mode.clone())
                .as_deref()
                .or(config.configured_api_mode.as_deref()),
        );
    }

    Ok(resolve_legacy_openai(ProviderResolutionRequest {
        provider: None,
        model: clean(request.model).or_else(|| config.configured_model),
        base_url: None,
        api_key: clean(request.api_key).or_else(|| config.configured_api_key),
        api_mode: clean(request.api_mode).or(config.configured_api_mode),
    }))
}

fn summary_request_for_profile(
    profile: &ProviderProfile,
    config: &RootConfig,
) -> ProviderResolutionRequest {
    let is_active = config.active_provider_id() == Some(profile.id.as_str());
    ProviderResolutionRequest {
        provider: Some(profile.id.clone()),
        model: if is_active {
            config.configured_model.clone()
        } else {
            None
        },
        base_url: None,
        api_key: if is_active {
            config.configured_api_key.clone()
        } else {
            None
        },
        api_mode: if is_active {
            config.configured_api_mode.clone()
        } else {
            None
        },
    }
}

fn summary_from_resolved(resolved: ResolvedProviderConfig, is_default: bool) -> ProviderSummary {
    ProviderSummary {
        id: resolved.id,
        label: resolved.label,
        kind: resolved.kind,
        enabled: true,
        is_default,
        model: resolved.model,
        base_url: resolved.base_url,
        api_mode: resolved.api_mode.as_str().to_string(),
        auth_source: resolved.auth_source,
    }
}

fn resolve_known_provider(
    provider_id: &str,
    config: &RootConfig,
    request: ProviderResolutionRequest,
) -> Result<ResolvedProviderConfig> {
    let kind = provider_id.to_string();
    if !is_known_provider(&kind) && kind != "openai" {
        return Err(anyhow!("provider `{provider_id}` not found"));
    }

    let is_active = config.active_provider_id() == Some(provider_id);
    let base_url_override = if is_active {
        config.configured_base_url.clone()
    } else {
        None
    };
    let api_key_override = clean(request.api_key).or_else(|| {
        if is_active {
            config.configured_api_key.clone()
        } else {
            None
        }
    });
    let model_override = clean(request.model).or_else(|| {
        if is_active {
            config.configured_model.clone()
        } else {
            None
        }
    });
    let api_mode_override = clean(request.api_mode).or_else(|| {
        if is_active {
            config.configured_api_mode.clone()
        } else {
            None
        }
    });

    let profile = ProviderProfile {
        id: provider_id.to_string(),
        label: known_provider_label(provider_id).to_string(),
        kind: kind.clone(),
        model: model_override,
        base_url: base_url_override,
        api_key: None,
        api_key_envs: known_provider_api_key_envs(provider_id)
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        enabled: true,
        api_mode_hint: if is_active {
            config.configured_api_mode.clone()
        } else {
            None
        },
    };

    resolve_from_profile(
        &profile,
        ProviderResolutionRequest {
            provider: Some(provider_id.to_string()),
            model: None,
            base_url: None,
            api_key: api_key_override,
            api_mode: api_mode_override,
        },
        true,
    )
}

fn resolve_direct_endpoint(
    id: &str,
    label: &str,
    model: Option<String>,
    base_url: String,
    api_key: Option<String>,
    api_mode_hint: Option<&str>,
) -> Result<ResolvedProviderConfig> {
    let base_url = base_url.trim().to_string();
    let explicit_api_key = clean(api_key);
    let env_api_key = env::var("OPENAI_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let (api_key, auth_source) = match explicit_api_key {
        Some(api_key) => (Some(api_key), Some("request".to_string())),
        None => (
            env_api_key.clone(),
            env_api_key.map(|_| "OPENAI_API_KEY".to_string()),
        ),
    };

    Ok(ResolvedProviderConfig {
        id: id.to_string(),
        label: label.to_string(),
        kind: "custom".to_string(),
        model: clean(model).unwrap_or_else(|| "gpt-4.1-mini".to_string()),
        base_url: base_url.clone(),
        api_key,
        api_mode: infer_api_mode_for_endpoint("custom", &base_url, api_mode_hint),
        auth_source,
    })
}

fn resolve_from_profile(
    profile: &ProviderProfile,
    request: ProviderResolutionRequest,
    _allow_default: bool,
) -> Result<ResolvedProviderConfig> {
    if !profile.enabled {
        return Err(anyhow!("provider `{}` is disabled", profile.id));
    }

    let base_url = clean(request.base_url)
        .or_else(|| profile.base_url.clone())
        .unwrap_or_else(|| default_base_url(&profile.kind));
    let model = clean(request.model)
        .or_else(|| profile.model.clone())
        .unwrap_or_else(|| default_model(&profile.kind).to_string());

    let (api_key, auth_source) = resolve_profile_api_key(profile, request.api_key)?;
    let api_mode_hint = clean(request.api_mode).or_else(|| profile.api_mode_hint.clone());
    let api_mode = infer_api_mode_for_endpoint(&profile.kind, &base_url, api_mode_hint.as_deref());

    Ok(ResolvedProviderConfig {
        id: profile.id.clone(),
        label: profile.label.clone(),
        kind: profile.kind.clone(),
        model,
        base_url,
        api_key,
        api_mode,
        auth_source,
    })
}

fn resolve_profile_api_key(
    profile: &ProviderProfile,
    request_api_key: Option<String>,
) -> Result<(Option<String>, Option<String>)> {
    if let Some(api_key) = clean(request_api_key) {
        return Ok((Some(api_key), Some("request".to_string())));
    }
    if let Some(api_key) = clean(profile.api_key.clone()) {
        return Ok((Some(api_key), Some("config".to_string())));
    }
    for var_name in &profile.api_key_envs {
        let api_key = env::var(var_name)
            .ok()
            .filter(|value| !value.trim().is_empty());
        if api_key.is_some() {
            return Ok((api_key, Some(var_name.clone())));
        }
    }

    match profile.kind.as_str() {
        "openai-codex" => resolve_codex_runtime_token(),
        _ => {
            for var_name in known_provider_api_key_envs(&profile.kind) {
                let api_key = env::var(var_name)
                    .ok()
                    .filter(|value| !value.trim().is_empty());
                if api_key.is_some() {
                    return Ok((api_key, Some((*var_name).to_string())));
                }
            }
            Ok((
                env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty()),
                env::var("OPENAI_API_KEY")
                    .ok()
                    .map(|_| "OPENAI_API_KEY".to_string()),
            ))
        }
    }
}

fn resolve_codex_runtime_token() -> Result<(Option<String>, Option<String>)> {
    if let Some(token) = env::var("CODEX_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok((Some(token), Some("CODEX_API_KEY".to_string())));
    }
    if let Some(token) = env::var("OPENAI_CODEX_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok((Some(token), Some("OPENAI_CODEX_API_KEY".to_string())));
    }
    if let Some(token) = read_hermes_codex_token()? {
        return Ok((Some(token), Some("HERMES_HOME/auth.json".to_string())));
    }
    if let Some(token) = read_codex_cli_token()? {
        return Ok((Some(token), Some("CODEX_HOME/auth.json".to_string())));
    }
    Ok((None, None))
}

fn read_hermes_codex_token() -> Result<Option<String>> {
    let home = hermes_home();
    let path = home.join("auth.json");
    let Some(root) = read_json(&path)? else {
        return Ok(None);
    };
    Ok(root
        .get("providers")
        .and_then(|value| value.get("openai-codex"))
        .and_then(|value| value.get("tokens"))
        .and_then(|value| value.get("access_token"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string))
}

fn read_codex_cli_token() -> Result<Option<String>> {
    let home = codex_home();
    let path = home.join("auth.json");
    let Some(root) = read_json(&path)? else {
        return Ok(None);
    };
    Ok(root
        .get("tokens")
        .and_then(|value| value.get("access_token"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string))
}

fn read_json(path: &Path) -> Result<Option<serde_json::Value>> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(value))
}

fn resolve_legacy_openai(request: ProviderResolutionRequest) -> ResolvedProviderConfig {
    let request_api_key = clean(request.api_key.clone());
    let base_url = clean(request.base_url)
        .or_else(|| {
            env::var("OPENAI_BASE_URL")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string());
    let api_key = request_api_key.clone().or_else(|| {
        env::var("OPENAI_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
    });
    let auth_source = request_api_key.map(|_| "request".to_string()).or_else(|| {
        env::var("OPENAI_API_KEY")
            .ok()
            .map(|_| "OPENAI_API_KEY".to_string())
    });

    ResolvedProviderConfig {
        id: "openai".to_string(),
        label: "OpenAI Compatible".to_string(),
        kind: "openai".to_string(),
        model: clean(request.model).unwrap_or_else(|| "gpt-4.1-mini".to_string()),
        base_url: base_url.clone(),
        api_key,
        api_mode: infer_api_mode_for_endpoint("openai", &base_url, request.api_mode.as_deref()),
        auth_source,
    }
}

fn build_legacy_summary() -> ProviderSummary {
    let resolved = resolve_legacy_openai(ProviderResolutionRequest::default());
    ProviderSummary {
        id: resolved.id,
        label: resolved.label,
        kind: resolved.kind,
        enabled: true,
        is_default: true,
        model: resolved.model,
        base_url: resolved.base_url,
        api_mode: resolved.api_mode.as_str().to_string(),
        auth_source: resolved.auth_source,
    }
}

pub fn infer_api_mode_for_endpoint(
    kind: &str,
    base_url: &str,
    api_mode_hint: Option<&str>,
) -> ApiMode {
    if let Some(hint) = api_mode_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        match hint {
            "responses" | "codex_responses" => return ApiMode::Responses,
            "chat" | "chat_completions" | "chat-completions" => {
                return ApiMode::ChatCompletions;
            }
            _ => {}
        }
    }
    if kind == "openai-codex" || kind == "copilot-acp" || base_url.contains("/backend-api/codex") {
        return ApiMode::Responses;
    }
    if is_official_openai_base_url(base_url) {
        return ApiMode::Responses;
    }
    ApiMode::ChatCompletions
}

fn is_official_openai_base_url(base_url: &str) -> bool {
    let normalized = base_url.trim().trim_end_matches('/').to_ascii_lowercase();
    normalized == DEFAULT_OPENAI_BASE_URL
        || normalized.starts_with(&(DEFAULT_OPENAI_BASE_URL.to_string() + "/"))
}

fn default_base_url(kind: &str) -> String {
    if let Some(env_name) = known_provider_base_url_env(kind) {
        if let Some(value) = env::var(env_name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return value;
        }
    }

    match kind {
        "openai-codex" => DEFAULT_CODEX_BASE_URL.to_string(),
        "nous" => DEFAULT_NOUS_BASE_URL.to_string(),
        "qwen-oauth" => DEFAULT_QWEN_BASE_URL.to_string(),
        "openrouter" => DEFAULT_OPENROUTER_BASE_URL.to_string(),
        "copilot" => DEFAULT_GITHUB_MODELS_BASE_URL.to_string(),
        "copilot-acp" => DEFAULT_COPILOT_ACP_BASE_URL.to_string(),
        "gemini" => "https://generativelanguage.googleapis.com/v1beta/openai".to_string(),
        "zai" => "https://api.z.ai/api/paas/v4".to_string(),
        "kimi-coding" => "https://api.moonshot.ai/v1".to_string(),
        "minimax" => "https://api.minimax.io/anthropic".to_string(),
        "anthropic" => "https://api.anthropic.com".to_string(),
        "alibaba" => "https://dashscope-intl.aliyuncs.com/compatible-mode/v1".to_string(),
        "minimax-cn" => "https://api.minimaxi.com/anthropic".to_string(),
        "deepseek" => "https://api.deepseek.com/v1".to_string(),
        "ai-gateway" => "https://ai-gateway.vercel.sh/v1".to_string(),
        "opencode-zen" => "https://opencode.ai/zen/v1".to_string(),
        "opencode-go" => "https://opencode.ai/zen/go/v1".to_string(),
        "kilocode" => "https://api.kilo.ai/api/gateway".to_string(),
        "huggingface" => "https://router.huggingface.co/v1".to_string(),
        _ => DEFAULT_OPENAI_BASE_URL.to_string(),
    }
}

fn default_model(kind: &str) -> &'static str {
    match kind {
        "openai-codex" | "copilot-acp" => "gpt-5-codex",
        "anthropic" | "minimax" | "minimax-cn" => "claude-sonnet-4-20250514",
        "gemini" => "gemini-2.5-pro",
        _ => "gpt-4.1-mini",
    }
}

fn known_provider_label(kind: &str) -> &'static str {
    match kind {
        "openai" => "OpenAI Compatible",
        "openai-codex" => "OpenAI Codex",
        "nous" => "Nous Portal",
        "qwen-oauth" => "Qwen OAuth",
        "openrouter" => "OpenRouter",
        "copilot" => "GitHub Copilot",
        "copilot-acp" => "GitHub Copilot ACP",
        "gemini" => "Google AI Studio",
        "zai" => "Z.AI / GLM",
        "kimi-coding" => "Kimi / Moonshot",
        "minimax" => "MiniMax",
        "anthropic" => "Anthropic",
        "alibaba" => "Alibaba Cloud (DashScope)",
        "minimax-cn" => "MiniMax (China)",
        "deepseek" => "DeepSeek",
        "ai-gateway" => "AI Gateway",
        "opencode-zen" => "OpenCode Zen",
        "opencode-go" => "OpenCode Go",
        "kilocode" => "Kilo Code",
        "huggingface" => "Hugging Face",
        _ => "Custom Endpoint",
    }
}

fn known_provider_api_key_envs(kind: &str) -> &'static [&'static str] {
    match kind {
        "openrouter" => &["OPENROUTER_API_KEY", "OPENAI_API_KEY"],
        "copilot" => &["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"],
        "gemini" => &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
        "zai" => &["GLM_API_KEY", "ZAI_API_KEY", "Z_AI_API_KEY"],
        "kimi-coding" => &["KIMI_API_KEY"],
        "minimax" => &["MINIMAX_API_KEY"],
        "anthropic" => &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN",
        ],
        "alibaba" => &["DASHSCOPE_API_KEY"],
        "minimax-cn" => &["MINIMAX_CN_API_KEY"],
        "deepseek" => &["DEEPSEEK_API_KEY"],
        "ai-gateway" => &["AI_GATEWAY_API_KEY"],
        "opencode-zen" => &["OPENCODE_ZEN_API_KEY"],
        "opencode-go" => &["OPENCODE_GO_API_KEY"],
        "kilocode" => &["KILOCODE_API_KEY"],
        "huggingface" => &["HF_TOKEN"],
        "openai" => &["OPENAI_API_KEY"],
        _ => &[],
    }
}

fn known_provider_base_url_env(kind: &str) -> Option<&'static str> {
    match kind {
        "qwen-oauth" => Some("HERMES_QWEN_BASE_URL"),
        "openrouter" => Some("OPENROUTER_BASE_URL"),
        "copilot-acp" => Some("COPILOT_ACP_BASE_URL"),
        "gemini" => Some("GEMINI_BASE_URL"),
        "zai" => Some("GLM_BASE_URL"),
        "kimi-coding" => Some("KIMI_BASE_URL"),
        "minimax" => Some("MINIMAX_BASE_URL"),
        "alibaba" => Some("DASHSCOPE_BASE_URL"),
        "minimax-cn" => Some("MINIMAX_CN_BASE_URL"),
        "deepseek" => Some("DEEPSEEK_BASE_URL"),
        "ai-gateway" => Some("AI_GATEWAY_BASE_URL"),
        "opencode-zen" => Some("OPENCODE_ZEN_BASE_URL"),
        "opencode-go" => Some("OPENCODE_GO_BASE_URL"),
        "kilocode" => Some("KILOCODE_BASE_URL"),
        "huggingface" => Some("HF_BASE_URL"),
        _ => None,
    }
}

fn is_known_provider(kind: &str) -> bool {
    matches!(
        kind,
        "openai"
            | "openai-codex"
            | "nous"
            | "qwen-oauth"
            | "openrouter"
            | "copilot"
            | "copilot-acp"
            | "gemini"
            | "zai"
            | "kimi-coding"
            | "minimax"
            | "anthropic"
            | "alibaba"
            | "minimax-cn"
            | "deepseek"
            | "ai-gateway"
            | "opencode-zen"
            | "opencode-go"
            | "kilocode"
            | "huggingface"
    )
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
    let root: Value = serde_yaml::from_str(&raw)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    let mut config = RootConfig::default();
    let Some(root_map) = root.as_mapping() else {
        return Ok(config);
    };

    if let Some(model) = mapping_value(root_map, "model") {
        parse_model_section(model, &mut config);
    }
    if config.configured_provider.is_none() {
        config.configured_provider = mapping_string(root_map, "provider");
    }
    if config.configured_base_url.is_none() {
        config.configured_base_url = mapping_string(root_map, "base_url");
    }

    if let Some(providers) = mapping_value(root_map, "providers") {
        parse_providers_section(providers, &mut config);
    }
    if let Some(custom_providers) = mapping_value(root_map, "custom_providers") {
        parse_custom_providers_section(custom_providers, &mut config);
    }

    Ok(config)
}

fn parse_model_section(value: &Value, config: &mut RootConfig) {
    if let Some(model) = value.as_str() {
        config.configured_model = clean(Some(model.to_string()));
        return;
    }
    let Some(map) = value.as_mapping() else {
        return;
    };

    config.configured_model =
        mapping_string(map, "default").or_else(|| mapping_string(map, "model"));
    config.configured_provider = mapping_string(map, "provider");
    config.configured_base_url = mapping_string(map, "base_url");
    config.configured_api_key = mapping_string(map, "api_key");
    config.configured_api_mode = mapping_string(map, "api_mode");
}

fn parse_providers_section(value: &Value, config: &mut RootConfig) {
    let Some(map) = value.as_mapping() else {
        return;
    };

    if mapping_value(map, "profiles").is_some() || mapping_value(map, "default").is_some() {
        config.providers_default = mapping_string(map, "default");
        let Some(profiles) = mapping_value(map, "profiles").and_then(Value::as_sequence) else {
            return;
        };
        for entry in profiles {
            let Some(entry_map) = entry.as_mapping() else {
                continue;
            };
            let Some(id) = mapping_string(entry_map, "id") else {
                continue;
            };
            config.profiles.push(ProviderProfile {
                label: mapping_string(entry_map, "label").unwrap_or_else(|| id.clone()),
                kind: mapping_string(entry_map, "kind").unwrap_or_else(|| "openai".to_string()),
                model: mapping_string(entry_map, "model"),
                base_url: mapping_string(entry_map, "base_url"),
                api_key: mapping_string(entry_map, "api_key"),
                api_key_envs: mapping_string(entry_map, "api_key_env")
                    .into_iter()
                    .collect(),
                enabled: mapping_bool(entry_map, "enabled").unwrap_or(true),
                id,
                api_mode_hint: None,
            });
        }
        return;
    }

    for (key, entry) in map {
        let Some(id) = key
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(entry_map) = entry.as_mapping() else {
            continue;
        };
        let transport = mapping_string(entry_map, "transport")
            .or_else(|| mapping_string(entry_map, "api_mode"));
        config.profiles.push(ProviderProfile {
            id: id.to_string(),
            label: mapping_string(entry_map, "name")
                .or_else(|| mapping_string(entry_map, "label"))
                .unwrap_or_else(|| id.to_string()),
            kind: mapping_string(entry_map, "kind")
                .unwrap_or_else(|| provider_kind_from_config(id, transport.as_deref())),
            model: mapping_string(entry_map, "default_model")
                .or_else(|| mapping_string(entry_map, "model")),
            base_url: mapping_string(entry_map, "api")
                .or_else(|| mapping_string(entry_map, "url"))
                .or_else(|| mapping_string(entry_map, "base_url")),
            api_key: mapping_string(entry_map, "api_key"),
            api_key_envs: mapping_string(entry_map, "api_key_env")
                .or_else(|| mapping_string(entry_map, "key_env"))
                .into_iter()
                .collect(),
            enabled: mapping_bool(entry_map, "enabled").unwrap_or(true),
            api_mode_hint: transport,
        });
    }
}

fn parse_custom_providers_section(value: &Value, config: &mut RootConfig) {
    let Some(entries) = value.as_sequence() else {
        return;
    };

    for entry in entries {
        let Some(entry_map) = entry.as_mapping() else {
            continue;
        };
        let Some(name) = mapping_string(entry_map, "name") else {
            continue;
        };
        let Some(base_url) = mapping_string(entry_map, "base_url")
            .or_else(|| mapping_string(entry_map, "url"))
            .or_else(|| mapping_string(entry_map, "api"))
        else {
            continue;
        };
        config.profiles.push(ProviderProfile {
            id: custom_provider_slug(&name),
            label: name,
            kind: "custom".to_string(),
            model: mapping_string(entry_map, "model")
                .or_else(|| mapping_string(entry_map, "default_model")),
            base_url: Some(base_url),
            api_key: mapping_string(entry_map, "api_key"),
            api_key_envs: Vec::new(),
            enabled: mapping_bool(entry_map, "enabled").unwrap_or(true),
            api_mode_hint: mapping_string(entry_map, "api_mode"),
        });
    }
}

fn provider_kind_from_config(id: &str, transport: Option<&str>) -> String {
    if is_known_provider(id) || id == "openai" {
        return id.to_string();
    }
    if matches!(transport, Some("responses" | "codex_responses")) {
        return "openai-codex".to_string();
    }
    "custom".to_string()
}

fn custom_provider_slug(display_name: &str) -> String {
    format!(
        "custom:{}",
        display_name.trim().to_lowercase().replace(' ', "-")
    )
}

fn mapping_value<'a>(map: &'a Mapping, key: &str) -> Option<&'a Value> {
    map.get(&Value::String(key.to_string()))
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

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn hermes_home() -> PathBuf {
    env::var("HERMES_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".hermes"))
}

fn codex_home() -> PathBuf {
    env::var("CODEX_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".codex"))
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
    use super::{ProviderResolutionRequest, load_provider_summaries, resolve_runtime_provider};
    use std::fs;

    #[test]
    fn resolves_default_profile_from_legacy_profiles_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"providers:
  default: codex
  profiles:
    - id: codex
      kind: openai-codex
      model: gpt-5-codex
"#,
        )
        .expect("write config");

        let resolved = resolve_runtime_provider(tmp.path(), ProviderResolutionRequest::default())
            .expect("resolve provider");
        assert_eq!(resolved.id, "codex");
        assert_eq!(resolved.kind, "openai-codex");
        assert_eq!(resolved.base_url, "https://chatgpt.com/backend-api/codex");
        assert_eq!(resolved.api_mode.as_str(), "responses");
    }

    #[test]
    fn resolves_provider_from_current_providers_dict_format() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"model:
  provider: local
  default: qwen-coder
providers:
  local:
    name: Local LM Studio
    api: http://localhost:1234/v1
"#,
        )
        .expect("write config");

        let resolved = resolve_runtime_provider(tmp.path(), ProviderResolutionRequest::default())
            .expect("resolve provider");
        assert_eq!(resolved.id, "local");
        assert_eq!(resolved.label, "Local LM Studio");
        assert_eq!(resolved.kind, "custom");
        assert_eq!(resolved.model, "qwen-coder");
        assert_eq!(resolved.base_url, "http://localhost:1234/v1");
    }

    #[test]
    fn lists_current_named_custom_providers() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"model:
  provider: custom:local-dev
  default: rotator-openrouter-coding
custom_providers:
  - name: local dev
    base_url: http://127.0.0.1:4141/v1
    model: rotator-openrouter-coding
"#,
        )
        .expect("write config");

        let providers = load_provider_summaries(tmp.path()).expect("providers");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "custom:local-dev");
        assert_eq!(providers[0].label, "local dev");
        assert!(providers[0].is_default);
        assert_eq!(providers[0].base_url, "http://127.0.0.1:4141/v1");
    }

    #[test]
    fn lists_current_builtin_provider_from_model_section() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"model:
  provider: openai-codex
  default: gpt-5.4
"#,
        )
        .expect("write config");

        let providers = load_provider_summaries(tmp.path()).expect("providers");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "openai-codex");
        assert!(providers[0].is_default);
        assert_eq!(providers[0].model, "gpt-5.4");
        assert_eq!(providers[0].api_mode, "responses");
    }

    #[test]
    fn defaults_official_openai_runtime_to_responses() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let resolved = resolve_runtime_provider(tmp.path(), ProviderResolutionRequest::default())
            .expect("resolve provider");
        assert_eq!(resolved.base_url, "https://api.openai.com/v1");
        assert_eq!(resolved.api_mode.as_str(), "responses");
    }

    #[test]
    fn defaults_direct_official_openai_endpoint_to_responses() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let resolved = resolve_runtime_provider(
            tmp.path(),
            ProviderResolutionRequest {
                base_url: Some("https://api.openai.com/v1".to_string()),
                ..ProviderResolutionRequest::default()
            },
        )
        .expect("resolve provider");
        assert_eq!(resolved.kind, "custom");
        assert_eq!(resolved.api_mode.as_str(), "responses");
    }

    #[test]
    fn keeps_local_openai_compatible_endpoint_on_chat_completions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let resolved = resolve_runtime_provider(
            tmp.path(),
            ProviderResolutionRequest {
                base_url: Some("http://localhost:1234/v1".to_string()),
                ..ProviderResolutionRequest::default()
            },
        )
        .expect("resolve provider");
        assert_eq!(resolved.kind, "custom");
        assert_eq!(resolved.api_mode.as_str(), "chat_completions");
    }

    #[test]
    fn direct_endpoint_api_mode_can_force_responses() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let resolved = resolve_runtime_provider(
            tmp.path(),
            ProviderResolutionRequest {
                base_url: Some("http://localhost:50930/v1".to_string()),
                api_mode: Some("responses".to_string()),
                ..ProviderResolutionRequest::default()
            },
        )
        .expect("resolve provider");

        assert_eq!(resolved.kind, "custom");
        assert_eq!(resolved.api_mode.as_str(), "responses");
    }

    #[test]
    fn official_openai_api_mode_can_force_chat_completions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let resolved = resolve_runtime_provider(
            tmp.path(),
            ProviderResolutionRequest {
                base_url: Some("https://api.openai.com/v1".to_string()),
                api_mode: Some("chat_completions".to_string()),
                ..ProviderResolutionRequest::default()
            },
        )
        .expect("resolve provider");

        assert_eq!(resolved.api_mode.as_str(), "chat_completions");
    }
}
