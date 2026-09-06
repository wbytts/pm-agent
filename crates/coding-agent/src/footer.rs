use ai::{Model, ModelThinkingLevel, Usage};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tui::{truncate_to_width, visible_width};

#[derive(Debug, Clone, PartialEq)]
pub struct ContextUsage {
    pub tokens: Option<u64>,
    pub context_window: u64,
    pub percent: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct FooterRenderState {
    pub cwd: PathBuf,
    pub home: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub session_name: Option<String>,
    pub usages: Vec<Usage>,
    pub context_usage: Option<ContextUsage>,
    pub model: Option<Model>,
    pub thinking_level: Option<ModelThinkingLevel>,
    pub using_subscription: bool,
    pub available_provider_count: usize,
    pub auto_compact_enabled: bool,
    pub extension_statuses: BTreeMap<String, String>,
}

pub fn format_cwd_for_footer(cwd: impl AsRef<Path>, home: Option<impl AsRef<Path>>) -> String {
    let cwd = normalize_path(cwd.as_ref());
    let Some(home) = home else {
        return cwd.to_string_lossy().to_string();
    };
    let home = normalize_path(home.as_ref());

    if cwd == home {
        return "~".to_string();
    }

    if let Ok(relative) = cwd.strip_prefix(&home) {
        if !relative.as_os_str().is_empty() {
            return format!("~/{}", relative.to_string_lossy());
        }
    }

    cwd.to_string_lossy().to_string()
}

pub fn render_footer(state: &FooterRenderState, width: usize) -> Vec<String> {
    let mut pwd = format_cwd_for_footer(&state.cwd, state.home.as_ref());
    if let Some(branch) = &state.git_branch {
        if !branch.is_empty() {
            pwd = format!("{pwd} ({branch})");
        }
    }
    if let Some(session_name) = &state.session_name {
        if !session_name.is_empty() {
            pwd = format!("{pwd} • {session_name}");
        }
    }

    let mut stats_parts = Vec::new();
    let total = state
        .usages
        .iter()
        .fold(Usage::default(), |mut total, usage| {
            total.input += usage.input;
            total.output += usage.output;
            total.cache_read += usage.cache_read;
            total.cache_write += usage.cache_write;
            total.cost.total += usage.cost.total;
            total
        });

    if total.input > 0 {
        stats_parts.push(format!("↑{}", format_tokens(total.input)));
    }
    if total.output > 0 {
        stats_parts.push(format!("↓{}", format_tokens(total.output)));
    }
    if total.cache_read > 0 {
        stats_parts.push(format!("R{}", format_tokens(total.cache_read)));
    }
    if total.cache_write > 0 {
        stats_parts.push(format!("W{}", format_tokens(total.cache_write)));
    }
    if total.cost.total > 0.0 || state.using_subscription {
        stats_parts.push(format!(
            "${:.3}{}",
            total.cost.total,
            if state.using_subscription {
                " (sub)"
            } else {
                ""
            }
        ));
    }

    let context_window = state
        .context_usage
        .as_ref()
        .map(|usage| usage.context_window)
        .or_else(|| {
            state
                .model
                .as_ref()
                .map(|model| model.context_window as u64)
        })
        .unwrap_or(0);
    let auto_indicator = if state.auto_compact_enabled {
        " (auto)"
    } else {
        ""
    };
    let context_display = match state.context_usage.as_ref().and_then(|usage| usage.percent) {
        Some(percent) => format!(
            "{:.1}%/{}{}",
            percent,
            format_tokens(context_window),
            auto_indicator
        ),
        None => format!("?/{}{}", format_tokens(context_window), auto_indicator),
    };
    stats_parts.push(context_display);

    let mut stats_left = stats_parts.join(" ");
    if visible_width(&stats_left) > width {
        stats_left = truncate_to_width(&stats_left, width, "...", false);
    }

    let right_side = footer_right_side(state, width, visible_width(&stats_left));
    let stats_line = align_footer_line(&stats_left, &right_side, width);

    let mut lines = vec![truncate_to_width(&pwd, width, "...", false), stats_line];

    if !state.extension_statuses.is_empty() {
        let status_line = state
            .extension_statuses
            .values()
            .map(|text| sanitize_status_text(text))
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(truncate_to_width(&status_line, width, "...", false));
    }

    lines
}

pub fn format_tokens(count: u64) -> String {
    if count < 1_000 {
        count.to_string()
    } else if count < 10_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else if count < 1_000_000 {
        format!("{}k", (count as f64 / 1_000.0).round() as u64)
    } else if count < 10_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else {
        format!("{}M", (count as f64 / 1_000_000.0).round() as u64)
    }
}

fn footer_right_side(state: &FooterRenderState, width: usize, stats_left_width: usize) -> String {
    let model_name = state
        .model
        .as_ref()
        .map(|model| model.id.as_str())
        .unwrap_or("no-model");
    let mut right_without_provider = model_name.to_string();

    if state
        .model
        .as_ref()
        .and_then(|model| model.reasoning.as_ref())
        .is_some()
    {
        let thinking = state
            .thinking_level
            .map(thinking_level_key)
            .unwrap_or("off");
        right_without_provider = if thinking == "off" {
            format!("{model_name} • thinking off")
        } else {
            format!("{model_name} • {thinking}")
        };
    }

    if state.available_provider_count > 1 {
        if let Some(model) = &state.model {
            let with_provider = format!("({}) {right_without_provider}", model.provider);
            if stats_left_width + 2 + visible_width(&with_provider) <= width {
                return with_provider;
            }
        }
    }

    right_without_provider
}

fn align_footer_line(left: &str, right: &str, width: usize) -> String {
    let left_width = visible_width(left);
    let right_width = visible_width(right);
    let min_padding = 2;
    if left_width + min_padding + right_width <= width {
        return format!(
            "{left}{}{right}",
            " ".repeat(width.saturating_sub(left_width + right_width))
        );
    }

    let available_for_right = width.saturating_sub(left_width + min_padding);
    if available_for_right > 0 {
        let truncated_right = truncate_to_width(right, available_for_right, "", false);
        let padding = width.saturating_sub(left_width + visible_width(&truncated_right));
        format!("{left}{}{truncated_right}", " ".repeat(padding))
    } else {
        left.to_string()
    }
}

fn sanitize_status_text(text: &str) -> String {
    text.replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn thinking_level_key(level: ModelThinkingLevel) -> &'static str {
    match level {
        ModelThinkingLevel::Off => "off",
        ModelThinkingLevel::Minimal => "minimal",
        ModelThinkingLevel::Low => "low",
        ModelThinkingLevel::Medium => "medium",
        ModelThinkingLevel::High => "high",
        ModelThinkingLevel::XHigh => "xhigh",
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    path.components().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::{Model, ModelReasoning, ModelThinkingLevel, Usage, UsageCost};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn footer_formats_cwd_under_home_like_pi() {
        assert_eq!(
            format_cwd_for_footer(
                PathBuf::from("/Users/demo/project"),
                Some(PathBuf::from("/Users/demo"))
            ),
            "~/project"
        );
        assert_eq!(
            format_cwd_for_footer(
                PathBuf::from("/tmp/project"),
                Some(PathBuf::from("/Users/demo"))
            ),
            "/tmp/project"
        );
        assert_eq!(
            format_cwd_for_footer(
                PathBuf::from("/home/user2"),
                Some(PathBuf::from("/home/user"))
            ),
            "/home/user2"
        );
        assert_eq!(
            format_cwd_for_footer(
                PathBuf::from("/home/user"),
                Some(PathBuf::from("/home/user"))
            ),
            "~"
        );
    }

    #[test]
    fn footer_renders_usage_context_provider_model_and_thinking_like_pi() {
        let mut model = Model {
            id: "gpt-5".to_string(),
            provider: "openai".to_string(),
            context_window: 200_000,
            reasoning: Some(ModelReasoning { enabled: true }),
            ..Model::default()
        };
        model.display_name = "GPT-5".to_string();
        let state = FooterRenderState {
            cwd: PathBuf::from("/Users/demo/project"),
            home: Some(PathBuf::from("/Users/demo")),
            git_branch: Some("main".to_string()),
            session_name: Some("migration".to_string()),
            usages: vec![Usage {
                input: 1_250,
                output: 25_400,
                cache_read: 999,
                cache_write: 1_500_000,
                total_tokens: 0,
                cost: UsageCost {
                    total: 0.1234,
                    ..UsageCost::default()
                },
            }],
            context_usage: Some(ContextUsage {
                tokens: Some(140_000),
                context_window: 200_000,
                percent: Some(70.0),
            }),
            model: Some(model),
            thinking_level: Some(ModelThinkingLevel::High),
            using_subscription: true,
            available_provider_count: 2,
            auto_compact_enabled: true,
            extension_statuses: BTreeMap::new(),
        };

        let lines = render_footer(&state, 120);

        assert_eq!(lines[0], "~/project (main) • migration");
        assert!(lines[1].contains("↑1.2k ↓25k R999 W1.5M $0.123 (sub) 70.0%/200k (auto)"));
        assert!(lines[1].ends_with("(openai) gpt-5 • high"));
    }

    #[test]
    fn footer_sanitizes_sorts_and_truncates_extension_statuses() {
        let mut extension_statuses = BTreeMap::new();
        extension_statuses.insert("z".to_string(), "zeta\nready".to_string());
        extension_statuses.insert("a".to_string(), "alpha\tok".to_string());
        let state = FooterRenderState {
            cwd: PathBuf::from("/repo"),
            home: None,
            git_branch: None,
            session_name: None,
            usages: Vec::new(),
            context_usage: Some(ContextUsage {
                tokens: None,
                context_window: 4096,
                percent: None,
            }),
            model: None,
            thinking_level: None,
            using_subscription: false,
            available_provider_count: 0,
            auto_compact_enabled: false,
            extension_statuses,
        };

        let lines = render_footer(&state, 80);

        assert_eq!(lines[0], "/repo");
        assert!(lines[1].starts_with("?/4.1k"));
        assert!(lines[1].ends_with("no-model"));
        assert_eq!(lines[2], "alpha ok zeta ready");
    }

    #[test]
    fn footer_keeps_wide_session_name_lines_within_width_like_pi() {
        let state = FooterRenderState {
            cwd: PathBuf::from("/tmp/project"),
            home: None,
            git_branch: Some("main".to_string()),
            session_name: Some("한글".repeat(30)),
            usages: Vec::new(),
            context_usage: Some(ContextUsage {
                tokens: None,
                context_window: 200_000,
                percent: Some(12.3),
            }),
            model: Some(Model {
                id: "test-model".to_string(),
                provider: "test".to_string(),
                context_window: 200_000,
                ..Model::default()
            }),
            thinking_level: Some(ModelThinkingLevel::Off),
            using_subscription: false,
            available_provider_count: 1,
            auto_compact_enabled: false,
            extension_statuses: BTreeMap::new(),
        };

        let width = 93;
        let lines = render_footer(&state, width);

        assert!(lines.iter().all(|line| visible_width(line) <= width));
    }

    #[test]
    fn footer_keeps_stats_line_with_wide_model_and_provider_within_width_like_pi() {
        let state = FooterRenderState {
            cwd: PathBuf::from("/tmp/project"),
            home: None,
            git_branch: None,
            session_name: None,
            usages: vec![Usage {
                input: 12_345,
                output: 6_789,
                cache_read: 0,
                cache_write: 0,
                total_tokens: 0,
                cost: UsageCost {
                    total: 1.234,
                    ..UsageCost::default()
                },
            }],
            context_usage: Some(ContextUsage {
                tokens: None,
                context_window: 200_000,
                percent: Some(12.3),
            }),
            model: Some(Model {
                id: "模".repeat(30),
                provider: "공급자".to_string(),
                context_window: 200_000,
                reasoning: Some(ModelReasoning { enabled: true }),
                ..Model::default()
            }),
            thinking_level: Some(ModelThinkingLevel::High),
            using_subscription: false,
            available_provider_count: 2,
            auto_compact_enabled: false,
            extension_statuses: BTreeMap::new(),
        };

        let width = 60;
        let lines = render_footer(&state, width);

        assert!(lines.iter().all(|line| visible_width(line) <= width));
    }
}
