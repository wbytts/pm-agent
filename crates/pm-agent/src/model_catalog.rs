use ai::{ApiProviderInfo, Model, ModelRegistry, ProviderRegistry};

pub fn available_models() -> Vec<Model> {
    let registry = ModelRegistry::builtins();
    registry
        .providers()
        .into_iter()
        .flat_map(|provider| registry.models(&provider))
        .collect()
}

pub fn available_providers() -> Vec<ApiProviderInfo> {
    ProviderRegistry::builtins().list()
}

pub(crate) fn default_model() -> Model {
    ModelRegistry::builtins()
        .get("local", "echo")
        .expect("内置 echo 模型必须注册")
}
