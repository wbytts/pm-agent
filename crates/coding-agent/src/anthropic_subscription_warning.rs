use crate::settings_manager::WarningSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnthropicCredentialKind {
    ApiKey,
    OAuth,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnthropicSubscriptionWarningInput<'a> {
    pub warnings: &'a WarningSettings,
    pub warning_already_shown: bool,
    pub model_provider: Option<&'a str>,
    pub stored_credential_kind: Option<AnthropicCredentialKind>,
    pub api_key: Option<&'a str>,
}

pub fn should_warn_about_anthropic_subscription_auth(
    input: AnthropicSubscriptionWarningInput<'_>,
) -> bool {
    if input.warnings.anthropic_extra_usage == Some(false) {
        return false;
    }
    if input.warning_already_shown {
        return false;
    }
    if input.model_provider != Some("anthropic") {
        return false;
    }
    if input.stored_credential_kind == Some(AnthropicCredentialKind::OAuth) {
        return true;
    }

    input
        .api_key
        .is_some_and(|api_key| api_key.starts_with("sk-ant-oat"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(warnings: &WarningSettings) -> AnthropicSubscriptionWarningInput<'_> {
        AnthropicSubscriptionWarningInput {
            warnings,
            warning_already_shown: false,
            model_provider: Some("anthropic"),
            stored_credential_kind: None,
            api_key: None,
        }
    }

    #[test]
    fn warns_for_anthropic_oauth_without_api_key_lookup_like_pi() {
        let warnings = WarningSettings::default();
        let mut input = input(&warnings);
        input.stored_credential_kind = Some(AnthropicCredentialKind::OAuth);

        assert!(should_warn_about_anthropic_subscription_auth(input));
    }

    #[test]
    fn warns_for_anthropic_subscription_api_key_like_pi() {
        let warnings = WarningSettings::default();
        let mut input = input(&warnings);
        input.stored_credential_kind = Some(AnthropicCredentialKind::ApiKey);
        input.api_key = Some("sk-ant-oat01-test");

        assert!(should_warn_about_anthropic_subscription_auth(input));
    }

    #[test]
    fn skips_warning_when_disabled_already_shown_or_not_anthropic_like_pi() {
        let warnings = WarningSettings::default();
        let disabled = WarningSettings {
            anthropic_extra_usage: Some(false),
        };

        assert!(!should_warn_about_anthropic_subscription_auth(
            AnthropicSubscriptionWarningInput {
                warnings: &disabled,
                ..input(&warnings)
            }
        ));
        assert!(!should_warn_about_anthropic_subscription_auth(
            AnthropicSubscriptionWarningInput {
                warning_already_shown: true,
                ..input(&warnings)
            }
        ));
        assert!(!should_warn_about_anthropic_subscription_auth(
            AnthropicSubscriptionWarningInput {
                model_provider: Some("openai"),
                ..input(&warnings)
            }
        ));
    }

    #[test]
    fn skips_warning_for_regular_anthropic_api_key_like_pi() {
        let warnings = WarningSettings::default();
        let mut input = input(&warnings);
        input.stored_credential_kind = Some(AnthropicCredentialKind::ApiKey);
        input.api_key = Some("sk-ant-api03-test");

        assert!(!should_warn_about_anthropic_subscription_auth(input));
    }
}
