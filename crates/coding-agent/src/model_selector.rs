use crate::model_resolver::ScopedModel;
use crate::settings_manager::{SettingsManager, SettingsStorage};
use ai::Model;
use tui::fuzzy_filter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelScope {
    All,
    Scoped,
}

#[derive(Debug, Clone)]
pub struct ModelSelectorItem {
    pub provider: String,
    pub id: String,
    pub model: Model,
}

fn models_are_equal(left: &Model, right: &Model) -> bool {
    left.provider == right.provider && left.id == right.id
}

#[derive(Debug, Clone)]
pub struct ModelSelectorState {
    current_model: Option<Model>,
    all_models: Vec<ModelSelectorItem>,
    scoped_models: Vec<ModelSelectorItem>,
    active_models: Vec<ModelSelectorItem>,
    filtered_models: Vec<ModelSelectorItem>,
    selected_index: usize,
    scope: ModelScope,
    search_query: String,
    error_message: Option<String>,
}

impl ModelSelectorState {
    pub fn new(
        current_model: Option<Model>,
        available_models: Vec<Model>,
        scoped_models: Vec<ScopedModel>,
        initial_search_query: Option<&str>,
        error_message: Option<String>,
    ) -> Self {
        let mut state = Self {
            current_model,
            all_models: Vec::new(),
            scoped_models: scoped_models
                .into_iter()
                .map(|scoped| ModelSelectorItem::from(scoped.model))
                .collect(),
            active_models: Vec::new(),
            filtered_models: Vec::new(),
            selected_index: 0,
            scope: ModelScope::All,
            search_query: initial_search_query.unwrap_or_default().to_string(),
            error_message,
        };
        state.all_models = state.sort_models(
            available_models
                .into_iter()
                .map(ModelSelectorItem::from)
                .collect(),
        );
        state.scope = if state.scoped_models.is_empty() {
            ModelScope::All
        } else {
            ModelScope::Scoped
        };
        state.reset_active_models();
        let query = state.search_query.clone();
        state.filter_models(&query);
        state
    }

    pub fn scope(&self) -> ModelScope {
        self.scope
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn filtered_models(&self) -> &[ModelSelectorItem] {
        &self.filtered_models
    }

    pub fn selected_model(&self) -> Option<&Model> {
        self.filtered_models
            .get(self.selected_index)
            .map(|item| &item.model)
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    pub fn toggle_scope(&mut self) {
        if self.scoped_models.is_empty() {
            return;
        }
        let next_scope = match self.scope {
            ModelScope::All => ModelScope::Scoped,
            ModelScope::Scoped => ModelScope::All,
        };
        self.set_scope(next_scope);
    }

    pub fn set_scope(&mut self, scope: ModelScope) {
        if self.scope == scope || (scope == ModelScope::Scoped && self.scoped_models.is_empty()) {
            return;
        }
        self.scope = scope;
        self.reset_active_models();
        self.selected_index = self
            .current_index_in(&self.active_models)
            .unwrap_or_default();
        let query = self.search_query.clone();
        self.filter_models(&query);
    }

    pub fn filter_models(&mut self, query: &str) {
        self.search_query = query.to_string();
        self.filtered_models = if query.trim().is_empty() {
            self.active_models.clone()
        } else {
            fuzzy_filter(&self.active_models, query, |item| {
                format!(
                    "{} {} {}/{} {} {}",
                    item.id, item.provider, item.provider, item.id, item.provider, item.id
                )
            })
        };
        self.clamp_selection();
    }

    pub fn move_selection(&mut self, direction: isize) {
        if self.filtered_models.is_empty() || direction == 0 {
            return;
        }
        if direction < 0 {
            self.selected_index = if self.selected_index == 0 {
                self.filtered_models.len() - 1
            } else {
                self.selected_index - 1
            };
        } else {
            self.selected_index = if self.selected_index == self.filtered_models.len() - 1 {
                0
            } else {
                self.selected_index + 1
            };
        }
    }

    pub fn apply_selection<S: SettingsStorage>(
        &self,
        settings: &mut SettingsManager<S>,
    ) -> Option<Model> {
        let model = self.selected_model()?.clone();
        settings.set_default_model_and_provider(model.provider.clone(), model.id.clone());
        Some(model)
    }

    fn reset_active_models(&mut self) {
        self.active_models = match self.scope {
            ModelScope::All => self.all_models.clone(),
            ModelScope::Scoped => self.scoped_models.clone(),
        };
    }

    fn sort_models(&self, mut models: Vec<ModelSelectorItem>) -> Vec<ModelSelectorItem> {
        models.sort_by(|left, right| {
            let left_current = self
                .current_model
                .as_ref()
                .is_some_and(|current| models_are_equal(current, &left.model));
            let right_current = self
                .current_model
                .as_ref()
                .is_some_and(|current| models_are_equal(current, &right.model));
            left_current
                .cmp(&right_current)
                .reverse()
                .then(left.provider.cmp(&right.provider))
        });
        models
    }

    fn current_index_in(&self, models: &[ModelSelectorItem]) -> Option<usize> {
        let current = self.current_model.as_ref()?;
        models
            .iter()
            .position(|item| models_are_equal(current, &item.model))
    }

    fn clamp_selection(&mut self) {
        if self.filtered_models.is_empty() {
            self.selected_index = 0;
            return;
        }
        self.selected_index = self.selected_index.min(self.filtered_models.len() - 1);
    }
}

impl From<Model> for ModelSelectorItem {
    fn from(model: Model) -> Self {
        Self {
            provider: model.provider.clone(),
            id: model.id.clone(),
            model,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelScope, ModelSelectorState};
    use crate::model_resolver::ScopedModel;
    use crate::settings_manager::SettingsManager;
    use ai::Model;

    #[test]
    fn model_selector_sorts_current_model_first_then_provider() {
        let current = model("openai", "gpt-4o");
        let state = ModelSelectorState::new(
            Some(current.clone()),
            vec![
                model("z-provider", "zed"),
                model("anthropic", "claude"),
                current,
            ],
            Vec::new(),
            None,
            None,
        );

        assert_eq!(state.scope(), ModelScope::All);
        assert_eq!(
            state
                .filtered_models()
                .iter()
                .map(|item| format!("{}/{}", item.provider, item.id))
                .collect::<Vec<_>>(),
            vec!["openai/gpt-4o", "anthropic/claude", "z-provider/zed"]
        );
        assert_eq!(state.selected_index(), 0);
    }

    #[test]
    fn model_selector_defaults_to_scoped_scope_and_toggles_to_all() {
        let current = model("openai", "gpt-4o");
        let mut state = ModelSelectorState::new(
            Some(current.clone()),
            vec![current.clone(), model("anthropic", "claude")],
            vec![ScopedModel {
                model: current,
                thinking_level: None,
            }],
            None,
            None,
        );

        assert_eq!(state.scope(), ModelScope::Scoped);
        assert_eq!(state.filtered_models().len(), 1);

        state.toggle_scope();

        assert_eq!(state.scope(), ModelScope::All);
        assert_eq!(state.filtered_models().len(), 2);
    }

    #[test]
    fn model_selector_filters_and_wraps_selection_like_pi_component() {
        let mut state = ModelSelectorState::new(
            None,
            vec![
                model("openai", "gpt-4o"),
                model("anthropic", "claude-sonnet"),
                model("google", "gemini-pro"),
            ],
            Vec::new(),
            None,
            None,
        );

        state.filter_models("claude");

        assert_eq!(state.filtered_models().len(), 1);
        assert_eq!(
            state.selected_model().map(|model| model.id.as_str()),
            Some("claude-sonnet")
        );
        state.move_selection(1);
        assert_eq!(state.selected_index(), 0);
        state.filter_models("");
        state.move_selection(-1);
        assert_eq!(state.selected_index(), 2);
    }

    #[test]
    fn model_selector_selection_writes_default_model_and_provider() {
        let mut state = ModelSelectorState::new(
            None,
            vec![model("anthropic", "claude"), model("openai", "gpt-4o")],
            Vec::new(),
            None,
            None,
        );
        let mut settings = SettingsManager::in_memory(serde_json::json!({}));

        state.filter_models("gpt");
        let selected = state
            .apply_selection(&mut settings)
            .expect("selected model");

        assert_eq!(selected.provider, "openai");
        assert_eq!(selected.id, "gpt-4o");
        assert_eq!(settings.get_default_provider().as_deref(), Some("openai"));
        assert_eq!(settings.get_default_model().as_deref(), Some("gpt-4o"));
    }

    fn model(provider: &str, id: &str) -> Model {
        Model {
            provider: provider.to_string(),
            id: id.to_string(),
            display_name: format!("{provider}/{id}"),
            ..Model::default()
        }
    }
}
