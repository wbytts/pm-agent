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
        assert!(value.starts_with("pi/1.2.3 ("));
        assert!(value.contains("rust"));
    }
}
