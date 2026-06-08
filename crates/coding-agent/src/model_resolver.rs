mod cli;
mod defaults;
mod pattern;
mod selection;
mod types;

pub use cli::resolve_cli_model;
pub use defaults::default_model_for_provider;
pub use pattern::{find_exact_model_reference_match, parse_model_pattern, resolve_model_scope};
pub use selection::{find_initial_model, restore_model_from_session};
pub use types::{
    CodingModelRegistry, InitialModelResult, ParsedModelResult, ResolveCliModelResult, ScopedModel,
};

#[cfg(test)]
mod tests {
    use super::*;
    use ai::{Model, ModelThinkingLevel};

    #[test]
    fn parses_model_pattern_with_thinking_suffix() {
        let models = models();
        let parsed = parse_model_pattern("claude-sonnet:high", &models, true);
        assert_eq!(
            parsed.model.expect("model should match").id,
            "claude-sonnet"
        );
        assert_eq!(parsed.thinking_level, Some(ModelThinkingLevel::High));
    }

    #[test]
    fn resolves_glob_scope_without_duplicates() {
        let scoped = resolve_model_scope(&["anthropic/*:low".to_string()], &models());
        assert_eq!(scoped.len(), 2);
        assert_eq!(scoped[0].thinking_level, Some(ModelThinkingLevel::Low));
    }

    #[test]
    fn resolves_cli_provider_model_and_custom_fallback() {
        let registry = TestRegistry {
            models: models(),
            available: models(),
        };
        let resolved = resolve_cli_model(Some("openai"), Some("custom-model"), &registry);
        assert_eq!(
            resolved.model.expect("fallback model should exist").id,
            "custom-model"
        );
        assert!(resolved
            .warning
            .expect("warning should exist")
            .contains("custom model id"));
    }

    #[test]
    fn restores_available_model_or_current_fallback() {
        let registry = TestRegistry {
            models: models(),
            available: vec![model("openai", "gpt-4o-20241001", "GPT-4o")],
        };
        let restored = restore_model_from_session(
            "anthropic",
            "claude-sonnet",
            Some(model("openai", "gpt-4o-20241001", "GPT-4o")),
            &registry,
        );
        assert!(restored.fallback_message.is_some());
        assert_eq!(
            restored.model.expect("fallback should exist").provider,
            "openai"
        );
    }

    struct TestRegistry {
        models: Vec<Model>,
        available: Vec<Model>,
    }

    impl CodingModelRegistry for TestRegistry {
        fn all_models(&self) -> Vec<Model> {
            self.models.clone()
        }

        fn available_models(&self) -> Vec<Model> {
            self.available.clone()
        }
    }

    fn models() -> Vec<Model> {
        vec![
            model("anthropic", "claude-sonnet-20241022", "Claude Sonnet"),
            model("anthropic", "claude-sonnet", "Claude Sonnet"),
            model("openai", "gpt-4o-20241001", "GPT-4o"),
        ]
    }

    fn model(provider: &str, id: &str, display_name: &str) -> Model {
        Model {
            provider: provider.to_string(),
            id: id.to_string(),
            api: "test".to_string(),
            display_name: display_name.to_string(),
            context_window: 1,
            ..Model::default()
        }
    }
}
