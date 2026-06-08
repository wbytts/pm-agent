use std::path::PathBuf;

pub const DEFAULT_PACKAGE_NAME: &str = "@earendil-works/pi-coding-agent";
pub const DEFAULT_APP_NAME: &str = "pi";
pub const DEFAULT_APP_TITLE: &str = "π";
pub const DEFAULT_CONFIG_DIR_NAME: &str = ".pi";
pub const DEFAULT_SHARE_VIEWER_URL: &str = "https://pi.dev/session/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfigPaths {
    pub home_dir: PathBuf,
    pub package_dir: PathBuf,
    pub app_name: String,
    pub config_dir_name: String,
    pub is_bun_binary: bool,
    pub has_src_dir: bool,
    pub env_agent_dir_value: Option<String>,
    pub env_session_dir_value: Option<String>,
    pub share_viewer_url_value: Option<String>,
}

impl AppConfigPaths {
    pub fn new(home_dir: impl Into<PathBuf>) -> Self {
        Self {
            home_dir: home_dir.into(),
            package_dir: PathBuf::new(),
            app_name: DEFAULT_APP_NAME.to_string(),
            config_dir_name: DEFAULT_CONFIG_DIR_NAME.to_string(),
            is_bun_binary: false,
            has_src_dir: true,
            env_agent_dir_value: None,
            env_session_dir_value: None,
            share_viewer_url_value: None,
        }
    }
}

pub fn env_agent_dir_name(app_name: &str) -> String {
    format!("{}_CODING_AGENT_DIR", app_name.to_uppercase())
}

pub fn env_session_dir_name(app_name: &str) -> String {
    format!("{}_CODING_AGENT_SESSION_DIR", app_name.to_uppercase())
}

pub fn agent_dir(config: &AppConfigPaths) -> PathBuf {
    if let Some(env_dir) = &config.env_agent_dir_value {
        return expand_tilde_path(env_dir, &config.home_dir);
    }
    config.home_dir.join(&config.config_dir_name).join("agent")
}

pub fn sessions_dir(config: &AppConfigPaths) -> PathBuf {
    if let Some(env_dir) = &config.env_session_dir_value {
        return expand_tilde_path(env_dir, &config.home_dir);
    }
    agent_dir(config).join("sessions")
}

pub fn settings_path(config: &AppConfigPaths) -> PathBuf {
    agent_dir(config).join("settings.json")
}

pub fn auth_path(config: &AppConfigPaths) -> PathBuf {
    agent_dir(config).join("auth.json")
}

pub fn models_path(config: &AppConfigPaths) -> PathBuf {
    agent_dir(config).join("models.json")
}

pub fn keybindings_path(config: &AppConfigPaths) -> PathBuf {
    agent_dir(config).join("keybindings.json")
}

pub fn bin_dir(config: &AppConfigPaths) -> PathBuf {
    agent_dir(config).join("bin")
}

pub fn prompts_dir(config: &AppConfigPaths) -> PathBuf {
    agent_dir(config).join("prompts")
}

pub fn tools_dir(config: &AppConfigPaths) -> PathBuf {
    agent_dir(config).join("tools")
}

pub fn custom_themes_dir(config: &AppConfigPaths) -> PathBuf {
    agent_dir(config).join("themes")
}

pub fn debug_log_path(config: &AppConfigPaths) -> PathBuf {
    agent_dir(config).join(format!("{}-debug.log", config.app_name))
}

pub fn share_viewer_url(config: &AppConfigPaths, gist_id: &str) -> String {
    let base_url = config
        .share_viewer_url_value
        .as_deref()
        .unwrap_or(DEFAULT_SHARE_VIEWER_URL);
    format!("{base_url}#{gist_id}")
}

pub fn themes_dir(config: &AppConfigPaths) -> PathBuf {
    if config.is_bun_binary {
        return config.package_dir.join("theme");
    }
    config
        .package_dir
        .join(src_or_dist(config))
        .join("modes")
        .join("interactive")
        .join("theme")
}

pub fn export_template_dir(config: &AppConfigPaths) -> PathBuf {
    if config.is_bun_binary {
        return config.package_dir.join("export-html");
    }
    config
        .package_dir
        .join(src_or_dist(config))
        .join("core")
        .join("export-html")
}

pub fn interactive_assets_dir(config: &AppConfigPaths) -> PathBuf {
    if config.is_bun_binary {
        return config.package_dir.join("assets");
    }
    config
        .package_dir
        .join(src_or_dist(config))
        .join("modes")
        .join("interactive")
        .join("assets")
}

pub fn bundled_interactive_asset_path(config: &AppConfigPaths, name: &str) -> PathBuf {
    interactive_assets_dir(config).join(name)
}

pub fn package_json_path(config: &AppConfigPaths) -> PathBuf {
    config.package_dir.join("package.json")
}

pub fn readme_path(config: &AppConfigPaths) -> PathBuf {
    config.package_dir.join("README.md")
}

pub fn docs_path(config: &AppConfigPaths) -> PathBuf {
    config.package_dir.join("docs")
}

pub fn examples_path(config: &AppConfigPaths) -> PathBuf {
    config.package_dir.join("examples")
}

pub fn changelog_path(config: &AppConfigPaths) -> PathBuf {
    config.package_dir.join("CHANGELOG.md")
}

fn src_or_dist(config: &AppConfigPaths) -> &'static str {
    if config.has_src_dir {
        "src"
    } else {
        "dist"
    }
}

fn expand_tilde_path(path: &str, home_dir: &std::path::Path) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir.join(rest);
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_env_names_like_pi_config() {
        assert_eq!(env_agent_dir_name("pi"), "PI_CODING_AGENT_DIR");
        assert_eq!(env_session_dir_name("tau"), "TAU_CODING_AGENT_SESSION_DIR");
    }

    #[test]
    fn resolves_agent_and_session_dirs_like_pi_config() {
        let config = AppConfigPaths::new("/home/alice");

        assert_eq!(agent_dir(&config), PathBuf::from("/home/alice/.pi/agent"));
        assert_eq!(
            sessions_dir(&config),
            PathBuf::from("/home/alice/.pi/agent/sessions")
        );
    }

    #[test]
    fn env_overrides_agent_and_session_dirs_like_pi_config() {
        let mut config = AppConfigPaths::new("/home/alice");
        config.env_agent_dir_value = Some("~/custom-agent".to_string());
        config.env_session_dir_value = Some("/tmp/sessions".to_string());

        assert_eq!(
            agent_dir(&config),
            PathBuf::from("/home/alice/custom-agent")
        );
        assert_eq!(sessions_dir(&config), PathBuf::from("/tmp/sessions"));
    }

    #[test]
    fn builds_agent_child_paths_like_pi_config() {
        let config = AppConfigPaths::new("/home/alice");

        assert_eq!(
            settings_path(&config),
            PathBuf::from("/home/alice/.pi/agent/settings.json")
        );
        assert_eq!(
            auth_path(&config),
            PathBuf::from("/home/alice/.pi/agent/auth.json")
        );
        assert_eq!(
            models_path(&config),
            PathBuf::from("/home/alice/.pi/agent/models.json")
        );
        assert_eq!(
            keybindings_path(&config),
            PathBuf::from("/home/alice/.pi/agent/keybindings.json")
        );
        assert_eq!(bin_dir(&config), PathBuf::from("/home/alice/.pi/agent/bin"));
        assert_eq!(
            prompts_dir(&config),
            PathBuf::from("/home/alice/.pi/agent/prompts")
        );
        assert_eq!(
            tools_dir(&config),
            PathBuf::from("/home/alice/.pi/agent/tools")
        );
        assert_eq!(
            custom_themes_dir(&config),
            PathBuf::from("/home/alice/.pi/agent/themes")
        );
        assert_eq!(
            debug_log_path(&config),
            PathBuf::from("/home/alice/.pi/agent/pi-debug.log")
        );
    }

    #[test]
    fn builds_share_viewer_url_like_pi_config() {
        let mut config = AppConfigPaths::new("/home/alice");
        assert_eq!(
            share_viewer_url(&config, "abc"),
            "https://pi.dev/session/#abc"
        );

        config.share_viewer_url_value = Some("https://example.test/view/".to_string());
        assert_eq!(
            share_viewer_url(&config, "def"),
            "https://example.test/view/#def"
        );
    }

    #[test]
    fn builds_package_asset_paths_like_pi_config() {
        let mut config = AppConfigPaths::new("/home/alice");
        config.package_dir = PathBuf::from("/opt/pi");
        config.has_src_dir = true;

        assert_eq!(
            themes_dir(&config),
            PathBuf::from("/opt/pi/src/modes/interactive/theme")
        );
        assert_eq!(
            export_template_dir(&config),
            PathBuf::from("/opt/pi/src/core/export-html")
        );
        assert_eq!(
            interactive_assets_dir(&config),
            PathBuf::from("/opt/pi/src/modes/interactive/assets")
        );
        assert_eq!(
            bundled_interactive_asset_path(&config, "logo.png"),
            PathBuf::from("/opt/pi/src/modes/interactive/assets/logo.png")
        );
        assert_eq!(
            package_json_path(&config),
            PathBuf::from("/opt/pi/package.json")
        );
        assert_eq!(readme_path(&config), PathBuf::from("/opt/pi/README.md"));
        assert_eq!(docs_path(&config), PathBuf::from("/opt/pi/docs"));
        assert_eq!(examples_path(&config), PathBuf::from("/opt/pi/examples"));
        assert_eq!(
            changelog_path(&config),
            PathBuf::from("/opt/pi/CHANGELOG.md")
        );
    }

    #[test]
    fn builds_dist_and_bun_binary_asset_paths_like_pi_config() {
        let mut config = AppConfigPaths::new("/home/alice");
        config.package_dir = PathBuf::from("/opt/pi");
        config.has_src_dir = false;

        assert_eq!(
            themes_dir(&config),
            PathBuf::from("/opt/pi/dist/modes/interactive/theme")
        );

        config.is_bun_binary = true;
        assert_eq!(themes_dir(&config), PathBuf::from("/opt/pi/theme"));
        assert_eq!(
            export_template_dir(&config),
            PathBuf::from("/opt/pi/export-html")
        );
        assert_eq!(
            interactive_assets_dir(&config),
            PathBuf::from("/opt/pi/assets")
        );
    }
}
