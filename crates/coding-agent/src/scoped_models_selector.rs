use crate::settings_manager::{SettingsManager, SettingsStorage};
use ai::Model;
use tui::fuzzy_filter;

pub type EnabledModelIds = Option<Vec<String>>;

#[derive(Debug, Clone)]
pub struct ScopedModelItem {
    pub full_id: String,
    pub model: Model,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ScopedModelsSelectorState {
    models_by_id: Vec<(String, Model)>,
    all_ids: Vec<String>,
    enabled_ids: EnabledModelIds,
    filtered_items: Vec<ScopedModelItem>,
    selected_index: usize,
    search_query: String,
    is_dirty: bool,
}

impl ScopedModelsSelectorState {
    pub fn new(all_models: Vec<Model>, enabled_ids: EnabledModelIds) -> Self {
        let models_by_id = all_models
            .into_iter()
            .map(|model| (model_full_id(&model), model))
            .collect::<Vec<_>>();
        let all_ids = models_by_id
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut state = Self {
            models_by_id,
            all_ids,
            enabled_ids,
            filtered_items: Vec::new(),
            selected_index: 0,
            search_query: String::new(),
            is_dirty: false,
        };
        state.refresh();
        state
    }

    pub fn enabled_ids(&self) -> EnabledModelIds {
        self.enabled_ids.clone()
    }

    pub fn filtered_items(&self) -> &[ScopedModelItem] {
        &self.filtered_items
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    pub fn enabled_count(&self) -> usize {
        self.enabled_ids
            .as_ref()
            .map(Vec::len)
            .unwrap_or(self.all_ids.len())
    }

    pub fn selected_item(&self) -> Option<&ScopedModelItem> {
        self.filtered_items.get(self.selected_index)
    }

    pub fn move_selection(&mut self, direction: isize) {
        if self.filtered_items.is_empty() || direction == 0 {
            return;
        }
        if direction < 0 {
            self.selected_index = if self.selected_index == 0 {
                self.filtered_items.len() - 1
            } else {
                self.selected_index - 1
            };
        } else {
            self.selected_index = if self.selected_index == self.filtered_items.len() - 1 {
                0
            } else {
                self.selected_index + 1
            };
        }
    }

    pub fn filter(&mut self, query: &str) {
        self.search_query = query.to_string();
        self.refresh();
    }

    pub fn toggle_selected(&mut self) {
        let Some(id) = self.selected_item().map(|item| item.full_id.clone()) else {
            return;
        };
        self.enabled_ids = toggle_enabled(self.enabled_ids.as_deref(), &id);
        self.mark_changed();
    }

    pub fn enable_all(&mut self) {
        let target_ids = self.target_ids_for_filter();
        self.enabled_ids = enable_all(
            self.enabled_ids.as_deref(),
            &self.all_ids,
            target_ids.as_deref(),
        );
        self.mark_changed();
    }

    pub fn clear_all(&mut self) {
        let target_ids = self.target_ids_for_filter();
        self.enabled_ids = clear_all(
            self.enabled_ids.as_deref(),
            &self.all_ids,
            target_ids.as_deref(),
        );
        self.mark_changed();
    }

    pub fn toggle_selected_provider(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        let provider = item.model.provider.clone();
        let provider_ids = self
            .models_by_id
            .iter()
            .filter(|(_, model)| model.provider == provider)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let provider_all_enabled = provider_ids
            .iter()
            .all(|id| is_enabled(self.enabled_ids.as_deref(), id));
        self.enabled_ids = if provider_all_enabled {
            clear_all(
                self.enabled_ids.as_deref(),
                &self.all_ids,
                Some(&provider_ids),
            )
        } else {
            enable_all(
                self.enabled_ids.as_deref(),
                &self.all_ids,
                Some(&provider_ids),
            )
        };
        self.mark_changed();
    }

    pub fn reorder_selected(&mut self, delta: isize) {
        if delta == 0 {
            return;
        }
        let Some(item) = self.selected_item() else {
            return;
        };
        if !is_enabled(self.enabled_ids.as_deref(), &item.full_id) {
            return;
        }
        let updated = move_enabled(self.enabled_ids.as_deref(), &item.full_id, delta);
        if updated == self.enabled_ids {
            return;
        }
        self.enabled_ids = updated;
        if delta < 0 {
            self.selected_index = self.selected_index.saturating_sub(1);
        } else {
            self.selected_index =
                (self.selected_index + 1).min(self.filtered_items.len().saturating_sub(1));
        }
        self.mark_changed();
    }

    pub fn persist<S: SettingsStorage>(&mut self, settings: &mut SettingsManager<S>) {
        let patterns = match self.enabled_ids.as_ref() {
            None => None,
            Some(ids) if ids.len() == self.all_ids.len() => None,
            Some(ids) => Some(ids.clone()),
        };
        settings.set_enabled_models(patterns);
        self.is_dirty = false;
    }

    fn mark_changed(&mut self) {
        self.is_dirty = true;
        self.refresh();
    }

    fn refresh(&mut self) {
        let items = build_items(
            &self.models_by_id,
            &self.all_ids,
            self.enabled_ids.as_deref(),
        );
        self.filtered_items = if self.search_query.trim().is_empty() {
            items
        } else {
            fuzzy_filter(&items, &self.search_query, |item| {
                format!("{} {}", item.model.id, item.model.provider)
            })
        };
        self.selected_index = self
            .selected_index
            .min(self.filtered_items.len().saturating_sub(1));
    }

    fn target_ids_for_filter(&self) -> Option<Vec<String>> {
        (!self.search_query.trim().is_empty()).then(|| {
            self.filtered_items
                .iter()
                .map(|item| item.full_id.clone())
                .collect()
        })
    }
}

pub fn model_full_id(model: &Model) -> String {
    format!("{}/{}", model.provider, model.id)
}

pub fn is_enabled(enabled_ids: Option<&[String]>, id: &str) -> bool {
    enabled_ids.is_none_or(|ids| ids.iter().any(|enabled_id| enabled_id == id))
}

pub fn toggle_enabled(enabled_ids: Option<&[String]>, id: &str) -> EnabledModelIds {
    let Some(enabled_ids) = enabled_ids else {
        return Some(vec![id.to_string()]);
    };
    let mut updated = enabled_ids.to_vec();
    if let Some(index) = updated.iter().position(|enabled_id| enabled_id == id) {
        updated.remove(index);
    } else {
        updated.push(id.to_string());
    }
    Some(updated)
}

pub fn enable_all(
    enabled_ids: Option<&[String]>,
    all_ids: &[String],
    target_ids: Option<&[String]>,
) -> EnabledModelIds {
    let Some(enabled_ids) = enabled_ids else {
        return None;
    };
    let targets = target_ids.unwrap_or(all_ids);
    let mut updated = enabled_ids.to_vec();
    for id in targets {
        if !updated.contains(id) {
            updated.push(id.clone());
        }
    }
    if updated.len() == all_ids.len() {
        None
    } else {
        Some(updated)
    }
}

pub fn clear_all(
    enabled_ids: Option<&[String]>,
    all_ids: &[String],
    target_ids: Option<&[String]>,
) -> EnabledModelIds {
    if enabled_ids.is_none() {
        return Some(match target_ids {
            Some(targets) => all_ids
                .iter()
                .filter(|id| !targets.contains(id))
                .cloned()
                .collect(),
            None => Vec::new(),
        });
    }
    let enabled_ids = enabled_ids.unwrap_or_default();
    let targets = target_ids.unwrap_or(enabled_ids);
    Some(
        enabled_ids
            .iter()
            .filter(|id| !targets.contains(id))
            .cloned()
            .collect(),
    )
}

pub fn move_enabled(enabled_ids: Option<&[String]>, id: &str, delta: isize) -> EnabledModelIds {
    let Some(enabled_ids) = enabled_ids else {
        return None;
    };
    let mut updated = enabled_ids.to_vec();
    let Some(index) = updated.iter().position(|enabled_id| enabled_id == id) else {
        return Some(updated);
    };
    let target_index = index as isize + delta;
    if target_index < 0 || target_index >= updated.len() as isize {
        return Some(updated);
    }
    updated.swap(index, target_index as usize);
    Some(updated)
}

fn build_items(
    models_by_id: &[(String, Model)],
    all_ids: &[String],
    enabled_ids: Option<&[String]>,
) -> Vec<ScopedModelItem> {
    sorted_ids(enabled_ids, all_ids)
        .into_iter()
        .filter_map(|id| {
            let model = models_by_id
                .iter()
                .find(|(model_id, _)| model_id == &id)
                .map(|(_, model)| model.clone())?;
            let enabled = is_enabled(enabled_ids, &id);
            Some(ScopedModelItem {
                full_id: id,
                model,
                enabled,
            })
        })
        .collect()
}

fn sorted_ids(enabled_ids: Option<&[String]>, all_ids: &[String]) -> Vec<String> {
    let Some(enabled_ids) = enabled_ids else {
        return all_ids.to_vec();
    };
    let mut ids = enabled_ids.to_vec();
    for id in all_ids {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::{EnabledModelIds, ScopedModelsSelectorState};
    use crate::settings_manager::SettingsManager;
    use ai::Model;

    #[test]
    fn scoped_models_selector_uses_null_as_all_enabled_and_first_toggle_keeps_only_current() {
        let mut state = state(None);

        assert_eq!(state.enabled_count(), 3);
        assert!(state.filtered_items().iter().all(|item| item.enabled));

        state.move_selection(1);
        state.toggle_selected();

        assert_eq!(
            state.enabled_ids(),
            Some(vec!["anthropic/claude".to_string()])
        );
        assert!(state.is_dirty());
    }

    #[test]
    fn scoped_models_selector_orders_enabled_ids_before_disabled_ids() {
        let state = state(Some(vec![
            "google/gemini".to_string(),
            "openai/gpt-4o".to_string(),
        ]));

        assert_eq!(
            state
                .filtered_items()
                .iter()
                .map(|item| item.full_id.as_str())
                .collect::<Vec<_>>(),
            vec!["google/gemini", "openai/gpt-4o", "anthropic/claude"]
        );
        assert_eq!(state.enabled_count(), 2);
    }

    #[test]
    fn scoped_models_selector_enable_clear_filter_and_provider_behave_like_pi() {
        let mut state = state(Some(vec!["openai/gpt-4o".to_string()]));

        state.filter("claude");
        state.enable_all();
        assert_eq!(
            state.enabled_ids(),
            Some(vec![
                "openai/gpt-4o".to_string(),
                "anthropic/claude".to_string()
            ])
        );

        state.clear_all();
        assert_eq!(state.enabled_ids(), Some(vec!["openai/gpt-4o".to_string()]));

        state.filter("");
        state.toggle_selected_provider();
        assert_eq!(state.enabled_ids(), Some(Vec::new()));
    }

    #[test]
    fn scoped_models_selector_reorders_only_enabled_models() {
        let mut state = state(Some(vec![
            "openai/gpt-4o".to_string(),
            "anthropic/claude".to_string(),
        ]));

        state.move_selection(1);
        state.reorder_selected(-1);

        assert_eq!(
            state.enabled_ids(),
            Some(vec![
                "anthropic/claude".to_string(),
                "openai/gpt-4o".to_string()
            ])
        );
    }

    #[test]
    fn scoped_models_selector_persist_clears_full_selection_or_writes_partial_selection() {
        let mut selector = state(Some(vec![
            "openai/gpt-4o".to_string(),
            "anthropic/claude".to_string(),
            "google/gemini".to_string(),
        ]));
        let mut settings = SettingsManager::in_memory(serde_json::json!({
            "enabledModels": ["old"]
        }));

        selector.persist(&mut settings);
        assert_eq!(settings.get_enabled_models(), None);

        let mut selector = state(Some(vec!["anthropic/claude".to_string()]));
        selector.persist(&mut settings);

        assert_eq!(
            settings.get_enabled_models(),
            Some(vec!["anthropic/claude".to_string()])
        );
    }

    fn state(enabled_ids: EnabledModelIds) -> ScopedModelsSelectorState {
        ScopedModelsSelectorState::new(
            vec![
                model("openai", "gpt-4o"),
                model("anthropic", "claude"),
                model("google", "gemini"),
            ],
            enabled_ids,
        )
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
