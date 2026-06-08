use crate::settings_manager::{SettingsManager, SettingsStorage};

pub fn is_install_telemetry_enabled<S: SettingsStorage>(
    settings_manager: &SettingsManager<S>,
    telemetry_env: Option<&str>,
) -> bool {
    telemetry_env.map_or_else(
        || settings_manager.get_enable_install_telemetry(),
        is_truthy_env_flag,
    )
}

fn is_truthy_env_flag(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_manager::SettingsManager;
    use serde_json::json;

    #[test]
    fn env_overrides_settings() {
        let manager = SettingsManager::in_memory(json!({}));
        assert!(is_install_telemetry_enabled(&manager, Some("true")));
        assert!(!is_install_telemetry_enabled(&manager, Some("0")));
    }

    #[test]
    fn falls_back_to_settings() {
        let manager = SettingsManager::in_memory(json!({ "enableInstallTelemetry": true }));
        assert!(is_install_telemetry_enabled(&manager, None));
    }
}
