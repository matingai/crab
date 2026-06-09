use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};

use crate::cli::GlobalOptions;
use crate::llm::ApiMode;
use crate::providers::{
    ProviderResolutionRequest, ResolvedProviderConfig, resolve_runtime_provider,
};
use crate::runtime_profile::RuntimeProfile;
use crate::shared_config::load_shared_agent_config;
use crate::smart_model_routing::{SmartModelRoutingConfig, load_smart_model_routing};

#[derive(Debug, Clone)]
pub struct AuxiliaryModelConfig {
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub api_mode: ApiMode,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub provider_id: String,
    pub provider_label: String,
    pub provider_kind: String,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub api_mode: ApiMode,
    pub skill_platform: String,
    pub workspace_root: PathBuf,
    pub data_dir: PathBuf,
    pub session_id: Option<String>,
    pub max_iterations: usize,
    pub system_prompt_override: Option<String>,
    pub tool_allowlist: Option<Vec<String>>,
    pub enable_shell_tool: bool,
    pub debug_context: bool,
    pub enable_solve_trace_context: bool,
    pub enable_meta_pattern_context: bool,
    pub enable_experience_context: bool,
    pub auxiliary_model: Option<AuxiliaryModelConfig>,
    pub smart_model_routing: Option<SmartModelRoutingConfig>,
    pub runtime_profile: RuntimeProfile,
}

impl AppConfig {
    pub fn load(cli: &GlobalOptions) -> Result<Self> {
        let workspace_root = match cli.workspace.clone() {
            Some(path) => path,
            None => env::current_dir().context("failed to read current directory")?,
        };
        let data_dir = cli
            .data_dir
            .clone()
            .or_else(|| env::var("HERMES_RS_DATA_DIR").ok().map(PathBuf::from))
            .unwrap_or_else(|| workspace_root.join(".hermes-agent-rs"));

        let provider = resolve_runtime_provider(
            &data_dir,
            ProviderResolutionRequest {
                provider: cli
                    .provider
                    .clone()
                    .or_else(|| env::var("HERMES_RS_PROVIDER").ok()),
                model: cli
                    .model
                    .clone()
                    .or_else(|| env::var("HERMES_RS_MODEL").ok()),
                base_url: cli
                    .base_url
                    .clone()
                    .or_else(|| env::var("OPENAI_BASE_URL").ok()),
                api_key: cli
                    .api_key
                    .clone()
                    .or_else(|| env::var("OPENAI_API_KEY").ok()),
            },
        )?;
        let auxiliary_model = load_auxiliary_model_from_env(&data_dir, &provider)?;
        let smart_model_routing = load_smart_model_routing(&data_dir)?;
        let runtime_profile = RuntimeProfile::resolve(&data_dir, &workspace_root)?;

        Ok(Self {
            provider_id: provider.id,
            provider_label: provider.label,
            provider_kind: provider.kind,
            model: provider.model,
            base_url: provider.base_url,
            api_key: provider.api_key,
            api_mode: provider.api_mode,
            skill_platform: env::var("HERMES_RS_PLATFORM")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "cli".to_string()),
            workspace_root,
            data_dir,
            session_id: cli
                .session
                .clone()
                .or_else(|| env::var("HERMES_RS_SESSION_ID").ok())
                .filter(|value| !value.trim().is_empty()),
            max_iterations: cli
                .max_iterations
                .or_else(|| {
                    env::var("HERMES_RS_MAX_ITERATIONS")
                        .ok()
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .unwrap_or(12),
            system_prompt_override: env::var("HERMES_RS_SYSTEM_PROMPT")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            tool_allowlist: None,
            enable_shell_tool: cli.enable_shell || env_flag("HERMES_RS_ENABLE_SHELL"),
            debug_context: env_flag("HERMES_RS_DEBUG_CONTEXT"),
            enable_solve_trace_context: env_flag("HERMES_RS_ENABLE_SOLVE_TRACE_CONTEXT"),
            enable_meta_pattern_context: env_flag("HERMES_RS_ENABLE_META_PATTERN_CONTEXT"),
            enable_experience_context: env_flag("HERMES_RS_ENABLE_EXPERIENCE_CONTEXT"),
            auxiliary_model,
            smart_model_routing,
            runtime_profile,
        })
    }
}

pub fn resolve_auxiliary_model(
    data_dir: &Path,
    primary: &ResolvedProviderConfig,
    provider: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<Option<AuxiliaryModelConfig>> {
    if provider.is_none() && model.is_none() && base_url.is_none() && api_key.is_none() {
        return Ok(None);
    }

    if provider.is_some() || base_url.is_some() {
        let resolved = resolve_runtime_provider(
            data_dir,
            ProviderResolutionRequest {
                provider,
                model,
                base_url,
                api_key,
            },
        )?;
        return Ok(Some(AuxiliaryModelConfig::from_resolved(resolved)));
    }

    Ok(Some(AuxiliaryModelConfig {
        model: model.unwrap_or_else(|| primary.model.clone()),
        base_url: primary.base_url.clone(),
        api_key: api_key.or_else(|| primary.api_key.clone()),
        api_mode: primary.api_mode,
    }))
}

fn load_auxiliary_model_from_env(
    data_dir: &Path,
    primary: &ResolvedProviderConfig,
) -> Result<Option<AuxiliaryModelConfig>> {
    let shared = load_shared_agent_config(data_dir).unwrap_or_default();
    resolve_auxiliary_model(
        data_dir,
        primary,
        env_value("HERMES_RS_AUX_PROVIDER"),
        env_value("HERMES_RS_AUX_MODEL").or(shared.aux_model),
        env_value("HERMES_RS_AUX_BASE_URL"),
        env_value("HERMES_RS_AUX_API_KEY"),
    )
}

impl AuxiliaryModelConfig {
    fn from_resolved(resolved: ResolvedProviderConfig) -> Self {
        Self {
            model: resolved.model,
            base_url: resolved.base_url,
            api_key: resolved.api_key,
            api_mode: resolved.api_mode,
        }
    }
}

fn env_value(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn env_flag(name: &str) -> bool {
    match env::var(name) {
        Ok(value) => matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "on"),
        Err(_) => false,
    }
}
