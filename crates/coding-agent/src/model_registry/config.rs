use ai::{Model, ModelCost, ModelInputKind, StreamEvent, StreamRequest};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub(super) struct ProviderRequestConfig {
    pub api_key: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub auth_header: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct CustomModelsResult {
    pub models: Vec<Model>,
    pub provider_overrides: BTreeMap<String, ProviderOverride>,
    pub model_overrides: BTreeMap<String, BTreeMap<String, ModelOverride>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ProviderOverride {
    pub display_name: Option<String>,
    pub base_url: Option<String>,
    pub compat: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ModelsConfig {
    pub providers: BTreeMap<String, ProviderConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub api: Option<String>,
    pub base_url: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub compat: Option<BTreeMap<String, serde_json::Value>>,
    pub auth_header: Option<bool>,
    pub models: Option<Vec<ModelDefinition>>,
    pub model_overrides: Option<BTreeMap<String, ModelOverride>>,
    pub oauth: Option<OAuthProviderConfig>,
    pub stream_simple: Option<StreamSimpleConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthProviderConfig {
    pub name: String,
}

pub type ProviderStreamHandler =
    Arc<dyn Fn(StreamRequest) -> ai::AiResult<Vec<StreamEvent>> + Send + Sync>;

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSimpleConfig {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(skip)]
    pub handler: Option<ProviderStreamHandler>,
}

impl std::fmt::Debug for StreamSimpleConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamSimpleConfig")
            .field("text", &self.text)
            .field("handler", &self.handler.as_ref().map(|_| "<handler>"))
            .finish()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDefinition {
    pub id: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub api: Option<String>,
    pub base_url: Option<String>,
    pub reasoning: Option<bool>,
    pub thinking_level_map: Option<BTreeMap<String, Option<String>>>,
    pub input: Option<Vec<ModelInputKind>>,
    pub cost: Option<ModelCost>,
    pub context_window: Option<usize>,
    pub max_tokens: Option<usize>,
    pub headers: Option<BTreeMap<String, String>>,
    pub compat: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOverride {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub reasoning: Option<bool>,
    pub thinking_level_map: Option<BTreeMap<String, Option<String>>>,
    pub input: Option<Vec<ModelInputKind>>,
    pub cost: Option<PartialModelCost>,
    pub context_window: Option<usize>,
    pub max_tokens: Option<usize>,
    pub headers: Option<BTreeMap<String, String>>,
    pub compat: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialModelCost {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}
