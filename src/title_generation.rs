use anyhow::Result;
use tokio::time::{Duration, timeout};

use crate::llm::OpenAiCompatClient;
use crate::types::ChatMessage;

const TITLE_PROMPT: &str = "Generate a short, descriptive conversation title in 3 to 7 words. Return only the title text. No quotes, no prefix, no trailing punctuation.";

pub async fn generate_title(
    client: &OpenAiCompatClient,
    model: &str,
    user_message: &str,
    assistant_response: &str,
) -> Result<Option<String>> {
    let user_snippet = truncate(user_message, 320);
    let assistant_snippet = truncate(assistant_response, 320);
    if user_snippet.is_empty() || assistant_snippet.is_empty() {
        return Ok(None);
    }

    let messages = vec![
        ChatMessage::system(TITLE_PROMPT),
        ChatMessage::user(format!(
            "User: {user_snippet}\n\nAssistant: {assistant_snippet}"
        )),
    ];
    let response = timeout(
        Duration::from_secs(3),
        client.respond(model, &messages, &[]),
    )
    .await;
    let Ok(response) = response else {
        return Ok(None);
    };
    let title = response?.message.content_text();
    Ok(clean_title(&title))
}

pub fn should_generate_title(user_turns: usize, current_title: Option<&str>) -> bool {
    if current_title.is_some_and(|value| !value.trim().is_empty()) {
        return false;
    }
    user_turns <= 2
}

fn clean_title(value: &str) -> Option<String> {
    let mut title = value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string();
    if title.to_lowercase().starts_with("title:") {
        title = title[6..].trim().to_string();
    }
    title = title
        .trim_end_matches(['.', '!', '?', '。', '！', '？'])
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }
    if title.chars().count() > 80 {
        title = title
            .chars()
            .take(80)
            .collect::<String>()
            .trim()
            .to_string();
    }
    if title.is_empty() { None } else { Some(title) }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    trimmed.chars().take(max_chars).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::{clean_title, should_generate_title};

    #[test]
    fn cleans_generated_title() {
        assert_eq!(
            clean_title("Title: Fix session loading."),
            Some("Fix session loading".to_string())
        );
        assert_eq!(
            clean_title("\"Implement Codex auth\""),
            Some("Implement Codex auth".to_string())
        );
    }

    #[test]
    fn only_generates_title_for_early_turns_without_existing_title() {
        assert!(should_generate_title(1, None));
        assert!(should_generate_title(2, None));
        assert!(!should_generate_title(3, None));
        assert!(!should_generate_title(1, Some("Existing")));
    }
}
