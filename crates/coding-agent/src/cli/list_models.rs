use crate::auth_guidance::{format_no_models_available_message, AuthGuidancePaths};
use crate::model_resolver::CodingModelRegistry;
use ai::{Model, ModelInputKind};
use tui::fuzzy_filter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListModelsOutput {
    pub stdout: String,
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelRow {
    provider: String,
    model: String,
    context: String,
    max_out: String,
    thinking: String,
    images: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ColumnWidths {
    provider: usize,
    model: usize,
    context: usize,
    max_out: usize,
    thinking: usize,
    images: usize,
}

const PROVIDER_HEADER: &str = "provider";
const MODEL_HEADER: &str = "model";
const CONTEXT_HEADER: &str = "context";
const MAX_OUT_HEADER: &str = "max-out";
const THINKING_HEADER: &str = "thinking";
const IMAGES_HEADER: &str = "images";

pub fn list_models_output(
    model_registry: &impl CodingModelRegistry,
    search_pattern: Option<&str>,
    load_error: Option<&str>,
    auth_paths: &AuthGuidancePaths,
) -> ListModelsOutput {
    let models = model_registry.available_models();
    let stderr = load_error
        .filter(|error| !error.trim().is_empty())
        .map(|error| format!("Warning: errors loading models.json:\n{error}"));

    if models.is_empty() {
        return ListModelsOutput {
            stdout: format_no_models_available_message(auth_paths),
            stderr,
        };
    }

    let rows = filtered_rows(models, search_pattern);
    if rows.is_empty() {
        return ListModelsOutput {
            stdout: format!(
                "No models matching \"{}\"",
                search_pattern.unwrap_or_default()
            ),
            stderr,
        };
    }

    ListModelsOutput {
        stdout: format_model_table(&rows),
        stderr,
    }
}

fn filtered_rows(mut models: Vec<Model>, search_pattern: Option<&str>) -> Vec<ModelRow> {
    if let Some(pattern) = search_pattern.filter(|pattern| !pattern.trim().is_empty()) {
        models = fuzzy_filter(&models, pattern, |model| {
            format!("{} {}", model.provider, model.id)
        });
    }

    models.sort_by(|a, b| a.provider.cmp(&b.provider).then_with(|| a.id.cmp(&b.id)));

    models.into_iter().map(model_row).collect()
}

fn model_row(model: Model) -> ModelRow {
    ModelRow {
        provider: model.provider,
        model: model.id,
        context: format_token_count(model.context_window),
        max_out: model
            .max_tokens
            .map(format_token_count)
            .unwrap_or_else(|| "-".to_string()),
        thinking: if model.reasoning.is_some() {
            "yes".to_string()
        } else {
            "no".to_string()
        },
        images: if model.input.contains(&ModelInputKind::Image) {
            "yes".to_string()
        } else {
            "no".to_string()
        },
    }
}

fn format_model_table(rows: &[ModelRow]) -> String {
    let widths = column_widths(rows);
    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(format_columns(
        &widths,
        PROVIDER_HEADER,
        MODEL_HEADER,
        CONTEXT_HEADER,
        MAX_OUT_HEADER,
        THINKING_HEADER,
        IMAGES_HEADER,
    ));
    for row in rows {
        lines.push(format_columns(
            &widths,
            &row.provider,
            &row.model,
            &row.context,
            &row.max_out,
            &row.thinking,
            &row.images,
        ));
    }
    lines.join("\n")
}

fn column_widths(rows: &[ModelRow]) -> ColumnWidths {
    rows.iter().fold(
        ColumnWidths {
            provider: PROVIDER_HEADER.len(),
            model: MODEL_HEADER.len(),
            context: CONTEXT_HEADER.len(),
            max_out: MAX_OUT_HEADER.len(),
            thinking: THINKING_HEADER.len(),
            images: IMAGES_HEADER.len(),
        },
        |mut widths, row| {
            widths.provider = widths.provider.max(row.provider.len());
            widths.model = widths.model.max(row.model.len());
            widths.context = widths.context.max(row.context.len());
            widths.max_out = widths.max_out.max(row.max_out.len());
            widths.thinking = widths.thinking.max(row.thinking.len());
            widths.images = widths.images.max(row.images.len());
            widths
        },
    )
}

fn format_columns(
    widths: &ColumnWidths,
    provider: &str,
    model: &str,
    context: &str,
    max_out: &str,
    thinking: &str,
    images: &str,
) -> String {
    format!(
        "{provider:<provider_width$}  {model:<model_width$}  {context:<context_width$}  {max_out:<max_out_width$}  {thinking:<thinking_width$}  {images:<images_width$}",
        provider_width = widths.provider,
        model_width = widths.model,
        context_width = widths.context,
        max_out_width = widths.max_out,
        thinking_width = widths.thinking,
        images_width = widths.images,
    )
}

fn format_token_count(count: usize) -> String {
    if count >= 1_000_000 {
        return format_compact_count(count, 1_000_000, "M");
    }
    if count >= 1_000 {
        return format_compact_count(count, 1_000, "K");
    }
    count.to_string()
}

fn format_compact_count(count: usize, unit: usize, suffix: &str) -> String {
    if count % unit == 0 {
        format!("{}{suffix}", count / unit)
    } else {
        format!("{:.1}{suffix}", count as f64 / unit as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::ModelReasoning;

    struct TestRegistry {
        available: Vec<Model>,
    }

    impl CodingModelRegistry for TestRegistry {
        fn all_models(&self) -> Vec<Model> {
            self.available.clone()
        }

        fn available_models(&self) -> Vec<Model> {
            self.available.clone()
        }
    }

    #[test]
    fn formats_token_count_like_pi_cli() {
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(1_000), "1K");
        assert_eq!(format_token_count(12_500), "12.5K");
        assert_eq!(format_token_count(1_000_000), "1M");
        assert_eq!(format_token_count(1_250_000), "1.2M");
    }

    #[test]
    fn formats_sorted_model_table() {
        let output = list_models_output(
            &TestRegistry {
                available: vec![
                    model("openai", "gpt-5", 400_000, Some(128_000), true, true),
                    model("anthropic", "claude", 1_000_000, None, false, false),
                ],
            },
            None,
            None,
            &AuthGuidancePaths::new("/docs"),
        );

        assert_eq!(
            output.stdout,
            [
                "provider   model   context  max-out  thinking  images",
                "anthropic  claude  1M       -        no        no    ",
                "openai     gpt-5   400K     128K     yes       yes   ",
            ]
            .join("\n")
        );
        assert!(output.stderr.is_none());
    }

    #[test]
    fn filters_models_with_fuzzy_search() {
        let output = list_models_output(
            &TestRegistry {
                available: vec![
                    model("openai", "gpt-5", 1, Some(1), false, false),
                    model("anthropic", "claude", 1, Some(1), false, false),
                ],
            },
            Some("cla"),
            None,
            &AuthGuidancePaths::new("/docs"),
        );

        assert!(output.stdout.contains("anthropic"));
        assert!(!output.stdout.contains("openai"));
    }

    #[test]
    fn reports_no_matching_models() {
        let output = list_models_output(
            &TestRegistry {
                available: vec![model("openai", "gpt-5", 1, Some(1), false, false)],
            },
            Some("missing"),
            None,
            &AuthGuidancePaths::new("/docs"),
        );

        assert_eq!(output.stdout, "No models matching \"missing\"");
    }

    #[test]
    fn reports_no_available_models_and_load_warning() {
        let output = list_models_output(
            &TestRegistry { available: vec![] },
            None,
            Some("bad json"),
            &AuthGuidancePaths::new("/docs"),
        );

        assert!(output.stdout.contains("No models available."));
        assert_eq!(
            output.stderr.as_deref(),
            Some("Warning: errors loading models.json:\nbad json")
        );
    }

    fn model(
        provider: &str,
        id: &str,
        context_window: usize,
        max_tokens: Option<usize>,
        reasoning: bool,
        images: bool,
    ) -> Model {
        Model {
            provider: provider.to_string(),
            id: id.to_string(),
            api: "test".to_string(),
            display_name: id.to_string(),
            context_window,
            max_tokens,
            reasoning: reasoning.then_some(ModelReasoning { enabled: true }),
            input: if images {
                vec![ModelInputKind::Text, ModelInputKind::Image]
            } else {
                vec![ModelInputKind::Text]
            },
            ..Model::default()
        }
    }
}
