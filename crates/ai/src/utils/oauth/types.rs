use std::collections::BTreeMap;

use crate::{AiResult, Model};

pub type OAuthProviderId = String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthCredentials {
    pub refresh: String,
    pub access: String,
    pub expires: u128,
    pub extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthPrompt {
    pub message: String,
    pub placeholder: Option<String>,
    pub allow_empty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthAuthInfo {
    pub url: String,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthDeviceCodeInfo {
    pub user_code: String,
    pub verification_uri: String,
    pub interval_seconds: Option<u64>,
    pub expires_in_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthSelectOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthSelectPrompt {
    pub message: String,
    pub options: Vec<OAuthSelectOption>,
}

pub trait OAuthLoginCallbacks {
    fn on_auth(&mut self, info: OAuthAuthInfo);
    fn on_device_code(&mut self, info: OAuthDeviceCodeInfo);
    fn on_prompt(&mut self, prompt: OAuthPrompt) -> AiResult<String>;
    fn on_progress(&mut self, _message: &str) {}
    fn on_manual_code_input(&mut self) -> AiResult<Option<String>> {
        Ok(None)
    }
    fn on_select(&mut self, prompt: OAuthSelectPrompt) -> AiResult<Option<String>>;
    fn is_cancelled(&self) -> bool {
        false
    }
}

pub trait OAuthProviderInterface: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn uses_callback_server(&self) -> bool {
        false
    }
    fn login(&self, callbacks: &mut dyn OAuthLoginCallbacks) -> AiResult<OAuthCredentials>;
    fn refresh_token(&self, credentials: &OAuthCredentials) -> AiResult<OAuthCredentials>;
    fn get_api_key(&self, credentials: &OAuthCredentials) -> String;
    fn modify_models(&self, models: Vec<Model>, _credentials: &OAuthCredentials) -> Vec<Model> {
        models
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthProviderInfo {
    pub id: OAuthProviderId,
    pub name: String,
    pub available: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCallbacks;

    impl OAuthLoginCallbacks for TestCallbacks {
        fn on_auth(&mut self, _info: OAuthAuthInfo) {}

        fn on_device_code(&mut self, _info: OAuthDeviceCodeInfo) {}

        fn on_prompt(&mut self, prompt: OAuthPrompt) -> AiResult<String> {
            Ok(prompt.placeholder.unwrap_or_default())
        }

        fn on_select(&mut self, prompt: OAuthSelectPrompt) -> AiResult<Option<String>> {
            Ok(prompt.options.first().map(|option| option.id.clone()))
        }
    }

    #[test]
    fn prompt_callback_can_return_placeholder() {
        let mut callbacks = TestCallbacks;

        let value = callbacks
            .on_prompt(OAuthPrompt {
                message: "Input code".to_string(),
                placeholder: Some("code".to_string()),
                allow_empty: false,
            })
            .expect("prompt");

        assert_eq!(value, "code");
    }

    #[test]
    fn select_callback_can_return_first_option() {
        let mut callbacks = TestCallbacks;

        let selected = callbacks
            .on_select(OAuthSelectPrompt {
                message: "Choose account".to_string(),
                options: vec![OAuthSelectOption {
                    id: "default".to_string(),
                    label: "Default".to_string(),
                }],
            })
            .expect("select");

        assert_eq!(selected.as_deref(), Some("default"));
    }
}
