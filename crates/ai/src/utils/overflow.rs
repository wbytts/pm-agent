use crate::Usage;

#[derive(Debug, Clone)]
pub struct AssistantMessageLike {
    pub stop_reason: String,
    pub error_message: Option<String>,
    pub usage: Usage,
}

const OVERFLOW_PATTERNS: &[&str] = &[
    "prompt is too long",
    "request_too_large",
    "input is too long for requested model",
    "exceeds the context window",
    "maximum context length",
    "input token count",
    "maximum prompt length is",
    "reduce the length of the messages",
    "exceeds the available context size",
    "greater than the context length",
    "context window exceeds limit",
    "exceeded model token limit",
    "model_context_window_exceeded",
    "context_length_exceeded",
    "context length exceeded",
    "too many tokens",
    "token limit exceeded",
];

const NON_OVERFLOW_PATTERNS: &[&str] = &[
    "throttling error:",
    "service unavailable:",
    "rate limit",
    "too many requests",
];

pub fn is_context_overflow(message: &AssistantMessageLike, context_window: Option<u64>) -> bool {
    if message.stop_reason == "error" {
        if let Some(error_message) = &message.error_message {
            let error = error_message.to_lowercase();
            let non_overflow = NON_OVERFLOW_PATTERNS
                .iter()
                .any(|pattern| error.contains(pattern));
            if !non_overflow && matches_overflow_error(&error) {
                return true;
            }
        }
    }

    if let Some(context_window) = context_window {
        let input_tokens = message.usage.input + message.usage.cache_read;
        if message.stop_reason == "stop" && input_tokens > context_window {
            return true;
        }
        if message.stop_reason == "length" && message.usage.output == 0 {
            let threshold = (context_window as f64 * 0.99).floor() as u64;
            if input_tokens >= threshold {
                return true;
            }
        }
    }

    false
}

pub fn get_overflow_patterns() -> Vec<&'static str> {
    OVERFLOW_PATTERNS.to_vec()
}

fn matches_overflow_error(error: &str) -> bool {
    if OVERFLOW_PATTERNS
        .iter()
        .any(|pattern| error.contains(pattern))
    {
        return true;
    }
    let compact = error.replace(',', "");
    (compact.contains("exceeds the model") && compact.contains("maximum context length"))
        || (compact.contains("input (")
            && compact.contains("tokens)")
            && compact.contains("context length"))
        || (compact.starts_with("400") || compact.starts_with("413"))
            && compact.contains("(no body)")
        || compact.contains("too large for model with")
        || compact.contains("prompt too long; exceeded")
        || compact.contains("prompt token count") && compact.contains("exceeds the limit of")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(
        stop_reason: &str,
        error: Option<&str>,
        input: u64,
        cache_read: u64,
        output: u64,
    ) -> AssistantMessageLike {
        AssistantMessageLike {
            stop_reason: stop_reason.to_string(),
            error_message: error.map(str::to_string),
            usage: Usage {
                input,
                cache_read,
                output,
                ..Usage::default()
            },
        }
    }

    #[test]
    fn detects_error_based_overflow_and_excludes_rate_limits() {
        let overflow_errors = [
            "Your input exceeds the context window of this model",
            "This endpoint's maximum context length is 128000 tokens. However, you requested about 193928 tokens",
            "The input (193928 tokens) is longer than the model's context length (128000 tokens).",
            "prompt token count of 193928 exceeds the limit of 128000",
            "Prompt contains 193928 tokens and is too large for model with 128000 maximum context length",
            "prompt too long; exceeded max context length by 65928 tokens",
            "400 status code (no body)",
            "413 (no body)",
        ];

        for error in overflow_errors {
            assert!(
                is_context_overflow(&message("error", Some(error), 0, 0, 0), None),
                "expected overflow detection for {error:?}"
            );
        }

        assert!(!is_context_overflow(
            &message(
                "error",
                Some("Throttling error: Too many tokens, wait"),
                0,
                0,
                0
            ),
            None
        ));
    }

    #[test]
    fn detects_silent_and_length_stop_overflow() {
        assert!(is_context_overflow(
            &message("stop", None, 101, 0, 1),
            Some(100)
        ));
        assert!(is_context_overflow(
            &message("length", None, 99, 0, 0),
            Some(100)
        ));
        assert!(!is_context_overflow(
            &message("length", None, 50, 0, 1),
            Some(100)
        ));
    }
}
