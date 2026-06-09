use reqwest::Method;
use reqwest::Url;
use serde_json::Value;
use std::time::Duration;

const CONTEXT_KEYS: &[&str] = &[
    "max_model_len",
    "max_context_length",
    "context_length",
    "max_tokens",
];

pub fn probe_context_length(model: &str, base_url: &str) -> Option<usize> {
    if !is_local_endpoint(base_url) {
        return None;
    }

    let model = strip_provider_prefix(model);
    let server_url = server_root(base_url)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .ok()?;

    query_ollama_show(&client, &server_url, &model)
        .or_else(|| query_lm_studio_models(&client, &server_url, &model))
        .or_else(|| query_llamacpp_props(&client, &server_url))
        .or_else(|| query_openai_models(&client, &server_url, &model))
}

fn is_local_endpoint(base_url: &str) -> bool {
    let Ok(url) = Url::parse(base_url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    matches!(host, "localhost" | "0.0.0.0" | "127.0.0.1" | "::1")
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || is_private_172(host)
}

fn is_private_172(host: &str) -> bool {
    let Some(rest) = host.strip_prefix("172.") else {
        return false;
    };
    let Some((octet, _)) = rest.split_once('.') else {
        return false;
    };
    octet
        .parse::<u8>()
        .ok()
        .is_some_and(|value| (16..=31).contains(&value))
}

fn strip_provider_prefix(model: &str) -> String {
    let trimmed = model.trim();
    let Some((prefix, bare)) = trimmed.split_once(':') else {
        return trimmed.to_string();
    };
    if bare.is_empty() || prefix.contains('/') {
        return trimmed.to_string();
    }
    match prefix.to_lowercase().as_str() {
        "local" | "openai" | "anthropic" | "openrouter" | "custom" | "ollama" | "lmstudio"
        | "vllm" | "llamacpp" => bare.to_string(),
        _ => trimmed.to_string(),
    }
}

fn server_root(base_url: &str) -> Option<String> {
    let mut url = Url::parse(base_url).ok()?;
    if url.path().ends_with("/v1") {
        let trimmed = url.path().trim_end_matches("/v1").to_string();
        url.set_path(if trimmed.is_empty() { "/" } else { &trimmed });
    }
    Some(url.to_string().trim_end_matches('/').to_string())
}

fn query_ollama_show(
    client: &reqwest::blocking::Client,
    server_url: &str,
    model: &str,
) -> Option<usize> {
    let payload = serde_json::json!({ "name": model });
    let body = request_json(
        client,
        Method::POST,
        &format!("{server_url}/api/show"),
        Some(payload),
    )?;
    extract_ollama_context_length(&body)
}

fn query_lm_studio_models(
    client: &reqwest::blocking::Client,
    server_url: &str,
    model: &str,
) -> Option<usize> {
    let body = request_json(
        client,
        Method::GET,
        &format!("{server_url}/api/v1/models"),
        None,
    )?;
    select_model_context_from_list(&body, model).or_else(|| extract_context_length(&body))
}

fn query_llamacpp_props(client: &reqwest::blocking::Client, server_url: &str) -> Option<usize> {
    for path in ["/v1/props", "/props"] {
        let Some(body) = request_json(client, Method::GET, &format!("{server_url}{path}"), None)
        else {
            continue;
        };
        if let Some(value) = body
            .get("default_generation_settings")
            .and_then(|settings| settings.get("n_ctx"))
            .and_then(as_positive_usize)
            .or_else(|| body.get("n_ctx").and_then(as_positive_usize))
        {
            return Some(value);
        }
    }
    None
}

fn query_openai_models(
    client: &reqwest::blocking::Client,
    server_url: &str,
    model: &str,
) -> Option<usize> {
    let body = request_json(
        client,
        Method::GET,
        &format!("{server_url}/v1/models"),
        None,
    )?;
    select_model_context_from_list(&body, model).or_else(|| extract_context_length(&body))
}

fn request_json(
    client: &reqwest::blocking::Client,
    method: Method,
    url: &str,
    body: Option<Value>,
) -> Option<Value> {
    let mut request = client.request(method, url);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<Value>().ok()
}

fn extract_ollama_context_length(body: &Value) -> Option<usize> {
    if let Some(model_info) = body.get("model_info").and_then(Value::as_object) {
        for (key, value) in model_info {
            if key.contains("context_length")
                && let Some(value) = as_positive_usize(value)
            {
                return Some(value);
            }
        }
    }

    let parameters = body.get("parameters").and_then(Value::as_str)?;
    for line in parameters.lines() {
        if !line.contains("num_ctx") {
            continue;
        }
        let value = line
            .split_whitespace()
            .rev()
            .find_map(|part| part.parse::<usize>().ok());
        if value.is_some() {
            return value;
        }
    }
    None
}

fn select_model_context_from_list(body: &Value, model: &str) -> Option<usize> {
    let list = body
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| body.get("models").and_then(Value::as_array))?;

    let selected = list
        .iter()
        .find(|entry| {
            entry
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| model_id_matches(id, model))
                || entry
                    .get("key")
                    .and_then(Value::as_str)
                    .is_some_and(|key| model_id_matches(key, model))
        })
        .or_else(|| (list.len() == 1).then(|| &list[0]))?;

    if let Some(instances) = selected.get("loaded_instances").and_then(Value::as_array) {
        for instance in instances {
            if let Some(value) = instance
                .get("config")
                .and_then(|config| config.get("context_length"))
                .and_then(as_positive_usize)
            {
                return Some(value);
            }
        }
    }

    extract_context_length(selected)
}

fn extract_context_length(body: &Value) -> Option<usize> {
    CONTEXT_KEYS
        .iter()
        .find_map(|key| body.get(*key).and_then(as_positive_usize))
}

fn as_positive_usize(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
        .filter(|number| *number > 0)
        .or_else(|| {
            value
                .as_i64()
                .and_then(|number| usize::try_from(number).ok())
                .filter(|number| *number > 0)
        })
}

fn model_id_matches(candidate_id: &str, lookup_model: &str) -> bool {
    if candidate_id == lookup_model {
        return true;
    }
    if let Some((_, slug)) = candidate_id.rsplit_once('/')
        && slug == lookup_model
    {
        return true;
    }
    candidate_id.contains(lookup_model) || lookup_model.contains(candidate_id)
}

#[cfg(test)]
mod tests {
    use super::{
        extract_ollama_context_length, is_local_endpoint, select_model_context_from_list,
        strip_provider_prefix,
    };
    use serde_json::json;

    #[test]
    fn strips_known_provider_prefixes_only() {
        assert_eq!(
            strip_provider_prefix("local:qwen2.5-coder"),
            "qwen2.5-coder"
        );
        assert_eq!(strip_provider_prefix("qwen3.5:32b"), "qwen3.5:32b");
    }

    #[test]
    fn recognizes_local_and_private_network_endpoints() {
        assert!(is_local_endpoint("http://127.0.0.1:11434/v1"));
        assert!(is_local_endpoint("http://192.168.1.50:8000/v1"));
        assert!(!is_local_endpoint("https://api.openai.com/v1"));
    }

    #[test]
    fn extracts_ollama_context_length_from_payload() {
        let payload = json!({
            "model_info": {
                "llama.context_length": 32768
            }
        });
        assert_eq!(extract_ollama_context_length(&payload), Some(32_768));
    }

    #[test]
    fn selects_loaded_instance_context_from_lm_studio_payload() {
        let payload = json!({
            "data": [
                {
                    "id": "publisher/demo-model",
                    "max_context_length": 32768,
                    "loaded_instances": [
                        { "config": { "context_length": 24576 } }
                    ]
                }
            ]
        });

        assert_eq!(
            select_model_context_from_list(&payload, "demo-model"),
            Some(24_576)
        );
    }

    #[test]
    fn falls_back_to_single_model_list_context_length() {
        let payload = json!({
            "data": [
                {
                    "id": "only-model",
                    "max_model_len": 65536
                }
            ]
        });

        assert_eq!(
            select_model_context_from_list(&payload, "different-name"),
            Some(65_536)
        );
    }
}
