use ai::{Model, ModelThinkingLevel};

#[derive(Debug, Clone)]
pub struct ScopedModel {
    pub model: Model,
    pub thinking_level: Option<ModelThinkingLevel>,
}

#[derive(Debug, Clone)]
pub struct ParsedModelResult {
    pub model: Option<Model>,
    pub thinking_level: Option<ModelThinkingLevel>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolveCliModelResult {
    pub model: Option<Model>,
    pub thinking_level: Option<ModelThinkingLevel>,
    pub warning: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InitialModelResult {
    pub model: Option<Model>,
    pub thinking_level: ModelThinkingLevel,
    pub fallback_message: Option<String>,
}

pub trait CodingModelRegistry {
    fn all_models(&self) -> Vec<Model>;
    fn available_models(&self) -> Vec<Model>;

    fn find(&self, provider: &str, model_id: &str) -> Option<Model> {
        self.all_models()
            .into_iter()
            .find(|model| model.provider == provider && model.id == model_id)
    }

    fn has_configured_auth(&self, model: &Model) -> bool {
        self.available_models()
            .iter()
            .any(|item| models_are_equal(item, model))
    }
}

pub fn models_are_equal(a: &Model, b: &Model) -> bool {
    a.id == b.id && a.provider == b.provider
}
