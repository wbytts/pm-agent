pub const DEFAULT_THINKING_LEVEL: &str = "medium";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_thinking_level_matches_pi() {
        assert_eq!(DEFAULT_THINKING_LEVEL, "medium");
    }
}
