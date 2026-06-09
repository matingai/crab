use regex::Regex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryableErrorKind {
    RateLimit,
    Overloaded,
    Server,
    Timeout,
}

pub fn parse_context_limit_from_error(error_msg: &str) -> Option<usize> {
    let error_lower = error_msg.to_lowercase();
    let patterns = [
        r"(?:max(?:imum)?|limit)\s*(?:context\s*)?(?:length|size|window)?\s*(?:is|of|:)?\s*(\d{4,})",
        r"context\s*(?:length|size|window)\s*(?:is|of|:)?\s*(\d{4,})",
        r"(\d{4,})\s*(?:token)?\s*(?:context|limit)",
        r">\s*(\d{4,})\s*(?:max|limit|token)",
        r"(\d{4,})\s*(?:max(?:imum)?)\b",
    ];

    for pattern in patterns {
        let regex = Regex::new(pattern).expect("valid context limit regex");
        if let Some(captures) = regex.captures(&error_lower) {
            let limit = captures.get(1)?.as_str().parse::<usize>().ok()?;
            if (1024..=10_000_000).contains(&limit) {
                return Some(limit);
            }
        }
    }

    None
}

pub fn parse_available_output_tokens_from_error(error_msg: &str) -> Option<usize> {
    let error_lower = error_msg.to_lowercase();
    let is_output_cap_error = error_lower.contains("max_tokens")
        && (error_lower.contains("available_tokens") || error_lower.contains("available tokens"));
    if !is_output_cap_error {
        return None;
    }

    let patterns = [
        r"available_tokens[:\s]+(\d+)",
        r"available\s+tokens[:\s]+(\d+)",
        r"=\s*(\d+)\s*$",
    ];

    for pattern in patterns {
        let regex = Regex::new(pattern).expect("valid output budget regex");
        if let Some(captures) = regex.captures(&error_lower) {
            let tokens = captures.get(1)?.as_str().parse::<usize>().ok()?;
            if tokens >= 1 {
                return Some(tokens);
            }
        }
    }

    None
}

pub fn is_context_overflow_error(error_msg: &str) -> bool {
    if parse_available_output_tokens_from_error(error_msg).is_some() {
        return false;
    }

    let error_lower = error_msg.to_lowercase();
    [
        "prompt is too long",
        "context_length_exceeded",
        "context length exceeded",
        "maximum context length",
        "max context length",
        "context window exceeded",
        "context size exceeded",
        "too many tokens",
        "request too large for the model",
    ]
    .iter()
    .any(|pattern| error_lower.contains(pattern))
}

pub fn classify_retryable_error(error_msg: &str) -> Option<RetryableErrorKind> {
    if is_context_overflow_error(error_msg)
        || parse_available_output_tokens_from_error(error_msg).is_some()
    {
        return None;
    }

    let error_lower = error_msg.to_lowercase();
    if [
        "429",
        "rate limit",
        "too many requests",
        "resource_exhausted",
        "throttled",
        "try again later",
    ]
    .iter()
    .any(|pattern| error_lower.contains(pattern))
    {
        return Some(RetryableErrorKind::RateLimit);
    }
    if ["503", "529", "overloaded", "service unavailable"]
        .iter()
        .any(|pattern| error_lower.contains(pattern))
    {
        return Some(RetryableErrorKind::Overloaded);
    }
    if [
        "500",
        "502",
        "504",
        "internal server error",
        "bad gateway",
        "gateway timeout",
    ]
    .iter()
    .any(|pattern| error_lower.contains(pattern))
    {
        return Some(RetryableErrorKind::Server);
    }
    if [
        "timeout",
        "timed out",
        "connection reset",
        "connection aborted",
        "broken pipe",
        "unexpected eof",
        "remoteprotocolerror",
        "transport error",
    ]
    .iter()
    .any(|pattern| error_lower.contains(pattern))
    {
        return Some(RetryableErrorKind::Timeout);
    }
    None
}

pub fn parse_retry_after_ms(error_msg: &str) -> Option<u64> {
    let error_lower = error_msg.to_lowercase();
    for pattern in [r"retry_after_ms[:\s]+(\d+)", r"retry after ms[:\s]+(\d+)"] {
        let regex = Regex::new(pattern).expect("valid retry_after_ms regex");
        if let Some(captures) = regex.captures(&error_lower) {
            let value = captures.get(1)?.as_str().parse::<u64>().ok()?;
            if value > 0 {
                return Some(value);
            }
        }
    }

    for pattern in [r"retry_after[:\s]+(\d+)", r"try again in (\d+) seconds?"] {
        let regex = Regex::new(pattern).expect("valid retry_after regex");
        if let Some(captures) = regex.captures(&error_lower) {
            let value = captures.get(1)?.as_str().parse::<u64>().ok()?;
            if value > 0 {
                return Some(value.saturating_mul(1_000));
            }
        }
    }

    None
}

pub fn jittered_backoff_ms(attempt: usize, base_ms: u64, max_ms: u64) -> u64 {
    let exponent = attempt.saturating_sub(1).min(10);
    let delay = base_ms.saturating_mul(1u64 << exponent).min(max_ms);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let jitter = if delay == 0 {
        0
    } else {
        nanos % (delay / 2 + 1)
    };
    delay.saturating_add(jitter).min(max_ms)
}

#[cfg(test)]
mod tests {
    use super::{
        RetryableErrorKind, classify_retryable_error, is_context_overflow_error,
        jittered_backoff_ms, parse_available_output_tokens_from_error,
        parse_context_limit_from_error, parse_retry_after_ms,
    };

    #[test]
    fn parses_available_output_tokens_for_output_cap_errors() {
        assert_eq!(
            parse_available_output_tokens_from_error(
                "max_tokens: 32768 > context_window: 200000 - input_tokens: 190000 = available_tokens: 10000",
            ),
            Some(10_000)
        );
        assert_eq!(
            parse_available_output_tokens_from_error(
                "max_tokens must be at most 10000 given your prompt (available tokens: 10000)",
            ),
            Some(10_000)
        );
        assert_eq!(
            parse_available_output_tokens_from_error(
                "max_tokens: 32768 > 200000 available_tokens : 5000",
            ),
            Some(5_000)
        );
    }

    #[test]
    fn ignores_non_output_cap_errors() {
        assert_eq!(
            parse_available_output_tokens_from_error(
                "prompt is too long: 205000 tokens > 200000 maximum",
            ),
            None
        );
        assert_eq!(
            parse_available_output_tokens_from_error(
                "context_length_exceeded: prompt has 131073 tokens, limit is 131072",
            ),
            None
        );
    }

    #[test]
    fn parses_context_limit_from_error_messages() {
        assert_eq!(
            parse_context_limit_from_error(
                "context_length_exceeded: prompt has 131073 tokens, limit is 131072",
            ),
            Some(131_072)
        );
        assert_eq!(
            parse_context_limit_from_error("prompt is too long: 205000 tokens > 200000 maximum"),
            Some(200_000)
        );
    }

    #[test]
    fn classifies_context_overflow_without_matching_output_cap_errors() {
        assert!(is_context_overflow_error(
            "prompt is too long: 205000 tokens > 200000 maximum"
        ));
        assert!(is_context_overflow_error(
            "context_length_exceeded: prompt has 131073 tokens, limit is 131072"
        ));
        assert!(!is_context_overflow_error(
            "max_tokens: 32768 > context_window: 200000 - input_tokens: 190000 = available_tokens: 10000"
        ));
    }

    #[test]
    fn classifies_retryable_non_context_errors() {
        assert_eq!(
            classify_retryable_error(
                "model API returned 429 Too Many Requests: rate limit exceeded"
            ),
            Some(RetryableErrorKind::RateLimit)
        );
        assert_eq!(
            classify_retryable_error("model API returned 503 Service Unavailable"),
            Some(RetryableErrorKind::Overloaded)
        );
        assert_eq!(
            classify_retryable_error("streaming request failed: operation timed out"),
            Some(RetryableErrorKind::Timeout)
        );
    }

    #[test]
    fn parses_retry_after_delays() {
        assert_eq!(
            parse_retry_after_ms("rate limit exceeded retry_after_ms: 25"),
            Some(25)
        );
        assert_eq!(parse_retry_after_ms("try again in 3 seconds"), Some(3_000));
    }

    #[test]
    fn computes_bounded_jittered_backoff() {
        let delay = jittered_backoff_ms(2, 100, 500);
        assert!((100..=500).contains(&delay));
    }
}
