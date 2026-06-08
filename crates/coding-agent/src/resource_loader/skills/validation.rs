const MAX_NAME_LENGTH: usize = 64;
const MAX_DESCRIPTION_LENGTH: usize = 1024;

pub fn validate_name(name: &str) -> Vec<String> {
    let mut errors = Vec::new();
    if name.len() > MAX_NAME_LENGTH {
        errors.push(format!(
            "name exceeds {MAX_NAME_LENGTH} characters ({})",
            name.len()
        ));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        errors.push(
            "name contains invalid characters (must be lowercase a-z, 0-9, hyphens only)"
                .to_string(),
        );
    }
    if name.starts_with('-') || name.ends_with('-') {
        errors.push("name must not start or end with a hyphen".to_string());
    }
    if name.contains("--") {
        errors.push("name must not contain consecutive hyphens".to_string());
    }
    errors
}

pub fn validate_description(description: &str) -> Vec<String> {
    if description.trim().is_empty() {
        return vec!["description is required".to_string()];
    }
    if description.len() > MAX_DESCRIPTION_LENGTH {
        return vec![format!(
            "description exceeds {MAX_DESCRIPTION_LENGTH} characters ({})",
            description.len()
        )];
    }
    Vec::new()
}
