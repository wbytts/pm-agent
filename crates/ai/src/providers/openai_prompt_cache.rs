pub const OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH: usize = 64;

pub fn clamp_openai_prompt_cache_key(key: Option<&str>) -> Option<String> {
    let key = key?;
    let mut chars = key.chars();
    let clamped: String = chars
        .by_ref()
        .take(OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH)
        .collect();
    if chars.next().is_none() {
        Some(key.to_string())
    } else {
        Some(clamped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_short_keys_unchanged() {
        assert_eq!(
            clamp_openai_prompt_cache_key(Some("session-cache")).as_deref(),
            Some("session-cache")
        );
    }

    #[test]
    fn clamps_by_unicode_scalar_values() {
        let key = "你".repeat(70);
        let result = clamp_openai_prompt_cache_key(Some(&key)).expect("key");

        assert_eq!(result.chars().count(), OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH);
    }

    #[test]
    fn preserves_missing_key() {
        assert_eq!(clamp_openai_prompt_cache_key(None), None);
    }
}
