use serde::Deserialize;
use std::collections::BTreeMap;

use crate::types::{ImagesModel, ModelCost, ModelInputKind};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogImagesModel {
    id: String,
    name: String,
    api: String,
    provider: String,
    base_url: String,
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    output: Vec<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    cost: ModelCost,
}

pub fn builtin_image_models() -> Vec<ImagesModel> {
    let catalog = serde_json::from_str::<BTreeMap<String, BTreeMap<String, CatalogImagesModel>>>(
        include_str!("catalog/image_models.json"),
    )
    .expect("内置图片模型 catalog 必须是合法 JSON");

    catalog
        .into_values()
        .flat_map(|models| models.into_values().map(ImagesModel::from))
        .collect()
}

impl From<CatalogImagesModel> for ImagesModel {
    fn from(model: CatalogImagesModel) -> Self {
        Self {
            id: model.id,
            provider: model.provider,
            api: model.api,
            display_name: model.name,
            base_url: model.base_url,
            input: model.input.into_iter().map(input_kind).collect(),
            output: model.output.into_iter().map(input_kind).collect(),
            headers: model.headers,
            cost: model.cost,
        }
    }
}

fn input_kind(value: String) -> ModelInputKind {
    match value.as_str() {
        "image" => ModelInputKind::Image,
        _ => ModelInputKind::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_generated_image_model_catalog() {
        let models = builtin_image_models();

        assert!(models.len() >= 29);
        assert!(models
            .iter()
            .any(|model| model.id == "google/gemini-3-pro-image-preview"));
        assert!(models.iter().any(|model| model.id == "openrouter/auto"));
    }
}
