use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
struct ContextLengthCacheFile {
    #[serde(default)]
    context_lengths: BTreeMap<String, usize>,
}

pub fn load_context_length(data_dir: &Path, model: &str, base_url: &str) -> Result<Option<usize>> {
    let path = cache_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed: ContextLengthCacheFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed
        .context_lengths
        .get(&cache_key(model, base_url))
        .copied()
        .filter(|value| *value > 0))
}

pub fn save_context_length(
    data_dir: &Path,
    model: &str,
    base_url: &str,
    context_length: usize,
) -> Result<()> {
    if context_length == 0 {
        return Ok(());
    }

    let path = cache_path(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut parsed = if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str::<ContextLengthCacheFile>(&raw).unwrap_or_default()
    } else {
        ContextLengthCacheFile::default()
    };

    parsed
        .context_lengths
        .insert(cache_key(model, base_url), context_length);
    let raw = serde_json::to_string_pretty(&parsed).context("failed to serialize context cache")?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("context-lengths.json")
}

fn cache_key(model: &str, base_url: &str) -> String {
    format!(
        "{}|{}",
        normalize_model(model),
        normalize(base_url.trim_end_matches('/'))
    )
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

fn normalize_model(model: &str) -> String {
    let trimmed = normalize(model);
    let Some((prefix, bare)) = trimmed.split_once(':') else {
        return trimmed;
    };
    if bare.is_empty() || prefix.contains('/') {
        return trimmed;
    }
    match prefix {
        "local" | "openai" | "anthropic" | "openrouter" | "custom" | "ollama" | "lmstudio"
        | "vllm" | "llamacpp" => bare.to_string(),
        _ => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::{load_context_length, save_context_length};

    #[test]
    fn saves_and_loads_cached_context_length() {
        let tmp = tempfile::tempdir().expect("tempdir");
        save_context_length(tmp.path(), "gpt-4.1", "http://127.0.0.1:1234/v1", 32_768)
            .expect("save");

        let loaded =
            load_context_length(tmp.path(), "gpt-4.1", "http://127.0.0.1:1234/v1").expect("load");
        assert_eq!(loaded, Some(32_768));
    }

    #[test]
    fn cache_keys_are_case_and_slash_insensitive() {
        let tmp = tempfile::tempdir().expect("tempdir");
        save_context_length(tmp.path(), "GPT-4.1", "http://127.0.0.1:1234/v1/", 65_536)
            .expect("save");

        let loaded =
            load_context_length(tmp.path(), "gpt-4.1", "http://127.0.0.1:1234/v1").expect("load");
        assert_eq!(loaded, Some(65_536));
    }

    #[test]
    fn cache_keys_ignore_known_model_prefixes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        save_context_length(
            tmp.path(),
            "local:qwen2.5-coder",
            "http://127.0.0.1:1234/v1",
            32_768,
        )
        .expect("save");

        let loaded = load_context_length(tmp.path(), "qwen2.5-coder", "http://127.0.0.1:1234/v1")
            .expect("load");
        assert_eq!(loaded, Some(32_768));
    }
}
