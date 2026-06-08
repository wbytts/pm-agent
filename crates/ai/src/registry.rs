use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::model_catalog::builtin_models;
use crate::providers::{
    AnthropicMessagesProvider, AzureOpenAiResponsesProvider, BedrockConverseProvider, EchoProvider,
    FauxProvider, GoogleGenerativeAiProvider, GoogleVertexProvider, MistralChatProvider,
    OpenAiCodexResponsesProvider, OpenAiCompletionsProvider, OpenAiResponsesProvider,
};
use crate::types::{
    validate_model, AiError, AiResult, LanguageModelProvider, Model, StreamEvent, StreamRequest,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProviderInfo {
    pub api: String,
    pub source_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RegisteredProvider {
    Echo(EchoProvider),
    Faux(FauxProvider),
    OpenAiCompletions(OpenAiCompletionsProvider),
    OpenAiChatCompat(OpenAiCompletionsProvider),
    OpenAiResponses(OpenAiResponsesProvider),
    AzureOpenAiResponses(AzureOpenAiResponsesProvider),
    OpenAiCodexResponses(OpenAiCodexResponsesProvider),
    AnthropicMessages(AnthropicMessagesProvider),
    MistralChat(MistralChatProvider),
    GoogleGenerativeAi(GoogleGenerativeAiProvider),
    GoogleVertex(GoogleVertexProvider),
    BedrockConverse(BedrockConverseProvider),
}

impl RegisteredProvider {
    pub fn api(&self) -> &'static str {
        match self {
            RegisteredProvider::Echo(_) => "local-echo",
            RegisteredProvider::Faux(_) => "faux",
            RegisteredProvider::OpenAiCompletions(_) => "openai-completions",
            RegisteredProvider::OpenAiChatCompat(_) => "openai-chat-completions",
            RegisteredProvider::OpenAiResponses(_) => "openai-responses",
            RegisteredProvider::AzureOpenAiResponses(_) => "azure-openai-responses",
            RegisteredProvider::OpenAiCodexResponses(_) => "openai-codex-responses",
            RegisteredProvider::AnthropicMessages(_) => "anthropic-messages",
            RegisteredProvider::MistralChat(_) => "mistral-chat-completions",
            RegisteredProvider::GoogleGenerativeAi(_) => "google-generative-ai",
            RegisteredProvider::GoogleVertex(_) => "google-vertex",
            RegisteredProvider::BedrockConverse(_) => "bedrock-converse-stream",
        }
    }
}

impl LanguageModelProvider for RegisteredProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        if request.model.api != self.api() {
            return Err(AiError::MismatchedApi {
                actual: request.model.api,
                expected: self.api().to_string(),
            });
        }

        match self {
            RegisteredProvider::Echo(provider) => provider.stream(request),
            RegisteredProvider::Faux(provider) => provider.stream(request),
            RegisteredProvider::OpenAiCompletions(provider) => provider.stream(request),
            RegisteredProvider::OpenAiChatCompat(provider) => provider.stream(request),
            RegisteredProvider::OpenAiResponses(provider) => provider.stream(request),
            RegisteredProvider::AzureOpenAiResponses(provider) => provider.stream(request),
            RegisteredProvider::OpenAiCodexResponses(provider) => provider.stream(request),
            RegisteredProvider::AnthropicMessages(provider) => provider.stream(request),
            RegisteredProvider::MistralChat(provider) => provider.stream(request),
            RegisteredProvider::GoogleGenerativeAi(provider) => provider.stream(request),
            RegisteredProvider::GoogleVertex(provider) => provider.stream(request),
            RegisteredProvider::BedrockConverse(provider) => provider.stream(request),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<String, (RegisteredProvider, Option<String>)>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builtins() -> Self {
        let mut registry = Self::new();
        registry.register(RegisteredProvider::Echo(EchoProvider), None);
        registry.register(RegisteredProvider::Faux(FauxProvider::default()), None);
        registry.register(
            RegisteredProvider::OpenAiCompletions(OpenAiCompletionsProvider::from_env()),
            None,
        );
        registry.register(
            RegisteredProvider::OpenAiChatCompat(OpenAiCompletionsProvider::from_env()),
            None,
        );
        registry.register(
            RegisteredProvider::OpenAiResponses(OpenAiResponsesProvider::from_env()),
            None,
        );
        registry.register(
            RegisteredProvider::AzureOpenAiResponses(AzureOpenAiResponsesProvider::from_env()),
            None,
        );
        registry.register(
            RegisteredProvider::OpenAiCodexResponses(OpenAiCodexResponsesProvider::from_env()),
            None,
        );
        registry.register(
            RegisteredProvider::AnthropicMessages(AnthropicMessagesProvider::from_env()),
            None,
        );
        registry.register(
            RegisteredProvider::MistralChat(MistralChatProvider::from_env()),
            None,
        );
        registry.register(
            RegisteredProvider::GoogleGenerativeAi(GoogleGenerativeAiProvider::from_env()),
            None,
        );
        registry.register(
            RegisteredProvider::GoogleVertex(GoogleVertexProvider::from_env()),
            None,
        );
        registry.register(
            RegisteredProvider::BedrockConverse(BedrockConverseProvider::from_env()),
            None,
        );
        registry
    }

    pub fn register(&mut self, provider: RegisteredProvider, source_id: Option<String>) {
        self.providers
            .insert(provider.api().to_string(), (provider, source_id));
    }

    pub fn get(&self, api: &str) -> Option<RegisteredProvider> {
        self.providers
            .get(api)
            .map(|(provider, _)| provider.clone())
    }

    pub fn provider_for(&self, model: &Model) -> AiResult<RegisteredProvider> {
        validate_model(model)?;
        self.get(&model.api)
            .ok_or_else(|| AiError::UnknownApi(model.api.clone()))
    }

    pub fn list(&self) -> Vec<ApiProviderInfo> {
        self.providers
            .iter()
            .map(|(api, (_, source_id))| ApiProviderInfo {
                api: api.clone(),
                source_id: source_id.clone(),
            })
            .collect()
    }

    pub fn unregister_source(&mut self, source_id: &str) {
        self.providers
            .retain(|_, (_, source)| source.as_deref() != Some(source_id));
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    models: BTreeMap<String, BTreeMap<String, Model>>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builtins() -> Self {
        let mut registry = Self::new();
        registry.register(Model {
            id: "echo".to_string(),
            provider: "local".to_string(),
            api: "local-echo".to_string(),
            display_name: "Local Echo".to_string(),
            context_window: 32_000,
            ..Model::default()
        });
        registry.register(Model {
            id: "faux".to_string(),
            provider: "local".to_string(),
            api: "faux".to_string(),
            display_name: "Faux Assistant".to_string(),
            context_window: 32_000,
            ..Model::default()
        });
        for model in builtin_models() {
            registry.register(model);
        }
        registry
    }

    pub fn register(&mut self, model: Model) {
        self.models
            .entry(model.provider.clone())
            .or_default()
            .insert(model.id.clone(), model);
    }

    pub fn get(&self, provider: &str, id: &str) -> Option<Model> {
        self.models
            .get(provider)
            .and_then(|models| models.get(id))
            .cloned()
    }

    pub fn providers(&self) -> Vec<String> {
        self.models.keys().cloned().collect()
    }

    pub fn models(&self, provider: &str) -> Vec<Model> {
        self.models
            .get(provider)
            .map(|models| models.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn all_models(&self) -> Vec<Model> {
        self.models
            .values()
            .flat_map(|models| models.values().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{
        AnthropicMessagesConfig, AnthropicMessagesProvider, BedrockConverseConfig,
        BedrockConverseProvider, GoogleGenerativeAiConfig, GoogleGenerativeAiProvider,
        GoogleVertexConfig, GoogleVertexProvider, MistralChatConfig, MistralChatProvider,
        OpenAiCompletionsConfig, OpenAiCompletionsProvider,
    };
    use crate::types::{Message, MessageRole};

    #[test]
    fn builtins_expose_models_and_providers() {
        let models = ModelRegistry::builtins();
        let providers = ProviderRegistry::builtins();
        let model = models
            .get("local", "echo")
            .expect("echo model should exist");
        let provider = providers
            .provider_for(&model)
            .expect("provider should exist");
        let events = provider
            .stream(StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: Default::default(),
            })
            .expect("stream should work");
        assert!(matches!(events.last(), Some(StreamEvent::Finished { .. })));
        let bedrock_model = models
            .get("amazon-bedrock", "anthropic.claude-sonnet-4-6")
            .expect("bedrock model should exist");
        assert!(providers.provider_for(&bedrock_model).is_ok());
        let vertex_model = models
            .get("google-vertex", "gemini-2.5-pro")
            .expect("vertex model should exist");
        assert!(providers.provider_for(&vertex_model).is_ok());
    }

    #[test]
    fn builtins_register_pi_builtin_api_providers() {
        let providers = ProviderRegistry::builtins()
            .list()
            .into_iter()
            .map(|provider| provider.api)
            .collect::<Vec<_>>();

        for expected in [
            "anthropic-messages",
            "openai-completions",
            "mistral-chat-completions",
            "openai-responses",
            "azure-openai-responses",
            "openai-codex-responses",
            "google-generative-ai",
            "google-vertex",
            "bedrock-converse-stream",
        ] {
            assert!(providers.iter().any(|api| api == expected));
        }
    }

    #[test]
    fn registry_rejects_unknown_api() {
        let providers = ProviderRegistry::builtins();
        let error = providers
            .provider_for(&Model {
                id: "x".to_string(),
                provider: "local".to_string(),
                api: "missing".to_string(),
                display_name: "Missing".to_string(),
                context_window: 1,
                ..Model::default()
            })
            .expect_err("unknown api should fail");
        assert!(matches!(error, AiError::UnknownApi(_)));
    }

    #[test]
    fn builtins_expose_github_copilot_models() {
        let models = ModelRegistry::builtins();
        let copilot_models = models.models("github-copilot");

        assert!(!copilot_models.is_empty());
        let model = models
            .get("github-copilot", "gpt-5.4")
            .expect("github copilot model should exist");
        assert_eq!(
            model.base_url.as_deref(),
            Some("https://api.individual.githubcopilot.com")
        );
        assert_eq!(
            model.headers.get("User-Agent").map(String::as_str),
            Some("GitHubCopilotChat/0.35.0")
        );
    }

    #[test]
    fn remote_providers_require_api_keys_before_network() {
        assert_missing_key(
            OpenAiCompletionsProvider::new(OpenAiCompletionsConfig {
                api_key: Some(String::new()),
                base_url: "https://api.openai.com/v1".to_string(),
            }),
            Model {
                id: "gpt-4o".to_string(),
                provider: "openai".to_string(),
                api: "openai-completions".to_string(),
                display_name: "GPT-4o".to_string(),
                context_window: 128_000,
                ..Model::default()
            },
        );
        assert_missing_key(
            AnthropicMessagesProvider::new(AnthropicMessagesConfig {
                api_key: Some(String::new()),
                base_url: "https://api.anthropic.com".to_string(),
                version: "2023-06-01".to_string(),
                max_tokens: 64,
            }),
            Model {
                id: "claude-sonnet-4-20250514".to_string(),
                provider: "anthropic".to_string(),
                api: "anthropic-messages".to_string(),
                display_name: "Claude Sonnet 4".to_string(),
                context_window: 200_000,
                ..Model::default()
            },
        );
        assert_missing_key(
            MistralChatProvider::new(MistralChatConfig {
                api_key: Some(String::new()),
                base_url: "https://api.mistral.ai/v1".to_string(),
            }),
            Model {
                id: "mistral-large-latest".to_string(),
                provider: "mistral".to_string(),
                api: "mistral-chat-completions".to_string(),
                display_name: "Mistral Large".to_string(),
                context_window: 128_000,
                ..Model::default()
            },
        );
        assert_missing_key(
            GoogleGenerativeAiProvider::new(GoogleGenerativeAiConfig {
                api_key: Some(String::new()),
                base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            }),
            Model {
                id: "gemini-2.0-flash".to_string(),
                provider: "google".to_string(),
                api: "google-generative-ai".to_string(),
                display_name: "Gemini 2.0 Flash".to_string(),
                context_window: 1_000_000,
                ..Model::default()
            },
        );
        assert_missing_key(
            BedrockConverseProvider::new(BedrockConverseConfig {
                region: None,
                profile: None,
                bearer_token: None,
                base_url: "https://bedrock-runtime.us-east-1.amazonaws.com".to_string(),
            }),
            Model {
                id: "anthropic.claude-sonnet-4-20250514-v1:0".to_string(),
                provider: "amazon-bedrock".to_string(),
                api: "bedrock-converse-stream".to_string(),
                display_name: "Claude Sonnet 4 Bedrock".to_string(),
                context_window: 200_000,
                ..Model::default()
            },
        );
        assert_missing_key(
            GoogleVertexProvider::new(GoogleVertexConfig {
                api_key: None,
                project: None,
                location: None,
                base_url: None,
            }),
            Model {
                id: "gemini-2.5-pro".to_string(),
                provider: "google-vertex".to_string(),
                api: "google-vertex".to_string(),
                display_name: "Gemini 2.5 Pro Vertex".to_string(),
                context_window: 1_000_000,
                ..Model::default()
            },
        );
    }

    fn assert_missing_key(provider: impl LanguageModelProvider, model: Model) {
        let error = provider
            .stream(StreamRequest {
                model,
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: Default::default(),
            })
            .expect_err("missing key should fail before network");
        assert!(matches!(error, AiError::MissingApiKey(_)));
    }
}
