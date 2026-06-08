use std::path::{Path, PathBuf};

const UNKNOWN_PROVIDER: &str = "unknown";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthGuidancePaths {
    pub docs_path: PathBuf,
}

impl AuthGuidancePaths {
    pub fn new(docs_path: impl Into<PathBuf>) -> Self {
        Self {
            docs_path: docs_path.into(),
        }
    }
}

pub fn get_provider_login_help(paths: &AuthGuidancePaths) -> String {
    [
        "Use /login to log into a provider via OAuth or API key. See:".to_string(),
        format!("  {}", slash_path(paths.docs_path.join("providers.md"))),
        format!("  {}", slash_path(paths.docs_path.join("models.md"))),
    ]
    .join("\n")
}

pub fn format_no_models_available_message(paths: &AuthGuidancePaths) -> String {
    format!("No models available. {}", get_provider_login_help(paths))
}

pub fn format_no_model_selected_message(paths: &AuthGuidancePaths) -> String {
    format!(
        "No model selected.\n\n{}\n\nThen use /model to select a model.",
        get_provider_login_help(paths)
    )
}

pub fn format_no_api_key_found_message(provider: &str, paths: &AuthGuidancePaths) -> String {
    let provider_display = if provider == UNKNOWN_PROVIDER {
        "the selected model"
    } else {
        provider
    };
    format!(
        "No API key found for {provider_display}.\n\n{}",
        get_provider_login_help(paths)
    )
}

fn slash_path(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_login_help_with_docs_paths() {
        let paths = AuthGuidancePaths::new("/opt/pi/docs");
        let help = get_provider_login_help(&paths);
        assert!(help.contains("/opt/pi/docs/providers.md"));
        assert!(help.contains("/opt/pi/docs/models.md"));
    }

    #[test]
    fn formats_unknown_provider_message() {
        let paths = AuthGuidancePaths::new("/docs");
        let message = format_no_api_key_found_message("unknown", &paths);
        assert!(message.contains("the selected model"));
    }
}
