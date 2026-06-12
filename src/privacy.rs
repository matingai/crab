use regex::{Captures, Regex};
use serde_json::Value;
use std::sync::OnceLock;

use crate::types::ChatMessage;

const REDACTED: &str = "[REDACTED]";

pub fn redact_secrets(text: impl AsRef<str>) -> String {
    let text = text.as_ref();
    if text.is_empty() {
        return String::new();
    }

    let mut redacted = assignment_regex()
        .replace_all(text, |captures: &Captures<'_>| {
            format!(
                "{}{}{}",
                captures
                    .get(1)
                    .map(|value| value.as_str())
                    .unwrap_or_default(),
                REDACTED,
                captures
                    .get(3)
                    .map(|value| value.as_str())
                    .unwrap_or_default()
            )
        })
        .to_string();
    redacted = auth_header_regex()
        .replace_all(&redacted, |captures: &Captures<'_>| {
            format!(
                "{}{}",
                captures
                    .get(1)
                    .map(|value| value.as_str())
                    .unwrap_or_default(),
                REDACTED
            )
        })
        .to_string();
    for regex in direct_secret_regexes() {
        redacted = regex.replace_all(&redacted, REDACTED).to_string();
    }
    redacted
}

pub fn redact_chat_message_secrets(mut message: ChatMessage) -> ChatMessage {
    if let Some(content) = message.content.take() {
        message.content = Some(redact_json_value(content));
    }
    if let Some(tool_calls) = message.tool_calls.as_mut() {
        for tool_call in tool_calls {
            tool_call.function.arguments = redact_secrets(&tool_call.function.arguments);
        }
    }
    message
}

fn redact_json_value(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_secrets(text)),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_json_value).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, redact_json_value(value)))
                .collect(),
        ),
        other => other,
    }
}

fn assignment_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?im)(["']?\b(?:[A-Z0-9]+[_-])*(?:API[_-]?KEY|ACCESS[_-]?TOKEN|REFRESH[_-]?TOKEN|AUTH[_-]?TOKEN|SESSION[_-]?TOKEN|GITHUB[_-]?TOKEN|CODEX[_-]?TOKEN|TOKEN|SECRET|PASSWORD|PRIVATE[_-]?KEY)(?:[_-][A-Z0-9]+)*["']?\s*[:=]\s*["']?)([^"',\s}]+)(["']?)"#,
        )
        .expect("assignment secret regex")
    })
}

fn auth_header_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"(?i)(\b(?:authorization|proxy-authorization)\s*[:=]\s*(?:bearer|basic)\s+)([A-Za-z0-9._~+/=-]{8,})"#)
            .expect("auth header secret regex")
    })
}

fn direct_secret_regexes() -> &'static [Regex] {
    static REGEXES: OnceLock<Vec<Regex>> = OnceLock::new();
    REGEXES.get_or_init(|| {
        [
            r#"\bsk-[A-Za-z0-9_-]{16,}\b"#,
            r#"\bgithub_pat_[A-Za-z0-9_]{20,}\b"#,
            r#"\bgh[pousr]_[A-Za-z0-9_]{20,}\b"#,
            r#"\bAKIA[0-9A-Z]{16}\b"#,
            r#"\bxox[baprs]-[A-Za-z0-9-]{20,}\b"#,
        ]
        .into_iter()
        .map(|pattern| Regex::new(pattern).expect("direct secret regex"))
        .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::{redact_chat_message_secrets, redact_secrets};
    use crate::types::{ChatMessage, ToolCall, ToolFunctionCall};
    use serde_json::json;

    #[test]
    fn redacts_common_secret_assignments_and_tokens() {
        let input = r#"
OPENAI_API_KEY=sk-test0123456789abcdef
"api_key": "sk-proj-abcdefghijklmnopqrstuvwxyz"
password: hunter2
Authorization: Bearer abcdefghijklmnopqrstuvwxyz
github_pat_1234567890abcdefghijklmnopqrstuvwxyz
AKIA1234567890ABCDEF
"#;

        let output = redact_secrets(input);

        assert!(output.contains("OPENAI_API_KEY=[REDACTED]"));
        assert!(output.contains(r#""api_key": "[REDACTED]""#));
        assert!(output.contains("password: [REDACTED]"));
        assert!(output.contains("Authorization: Bearer [REDACTED]"));
        assert!(!output.contains("sk-test0123456789abcdef"));
        assert!(!output.contains("github_pat_1234567890abcdefghijklmnopqrstuvwxyz"));
        assert!(!output.contains("AKIA1234567890ABCDEF"));
    }

    #[test]
    fn leaves_non_secret_text_alone() {
        let input = "status: completed\npath: README.md\nprojected_tokens: 1234\nmessage: no credentials here";

        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn redacts_chat_message_content_and_tool_arguments() {
        let message = ChatMessage {
            role: "assistant".to_string(),
            content: Some(json!("Use OPENAI_API_KEY=sk-test0123456789abcdef")),
            tool_calls: Some(vec![ToolCall {
                id: "call-1".to_string(),
                kind: "function".to_string(),
                function: ToolFunctionCall {
                    name: "write_file".to_string(),
                    arguments: r#"{"content":"TOKEN=abcdef1234567890abcdef"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
        };

        let redacted = redact_chat_message_secrets(message);

        assert!(!redacted.content_text().contains("sk-test"));
        assert!(
            !redacted.tool_calls.unwrap()[0]
                .function
                .arguments
                .contains("abcdef1234567890abcdef")
        );
    }
}
