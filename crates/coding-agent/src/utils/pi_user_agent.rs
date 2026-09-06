pub fn get_pi_user_agent(version: &str) -> String {
    let platform = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!("pi/{version} ({platform}; rust; {arch})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_user_agent() {
        let value = get_pi_user_agent("1.2.3");
        assert_eq!(
            value,
            format!(
                "pi/1.2.3 ({}; rust; {})",
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        );
        assert_pi_dev_user_agent_shape(&value);
    }

    fn assert_pi_dev_user_agent_shape(value: &str) {
        let Some(rest) = value.strip_prefix("pi/") else {
            panic!("user agent should start with pi/");
        };
        let Some((version, details)) = rest.split_once(" (") else {
            panic!("user agent should include details");
        };
        assert!(!version.is_empty());
        assert!(!version
            .chars()
            .any(|ch| ch.is_whitespace() || ch == '(' || ch == ')'));
        assert!(details.ends_with(')'));

        let details = details.trim_end_matches(')');
        let parts = details.split(';').map(str::trim).collect::<Vec<_>>();
        assert_eq!(parts.len(), 3);
        for part in parts {
            assert!(!part.is_empty());
            assert!(!part.contains(['(', ')']));
        }
    }
}
