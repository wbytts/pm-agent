use super::paths::{basename_without_extension, display_path};
use crate::diagnostics::{
    ResourceCollision, ResourceDiagnostic, ResourceDiagnosticKind, ResourceType,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub path: String,
    pub content: Value,
    pub source_info: Option<Value>,
}

pub fn load_themes(
    paths: impl IntoIterator<Item = impl AsRef<Path>>,
) -> (Vec<Theme>, Vec<ResourceDiagnostic>) {
    let mut themes = Vec::new();
    let mut diagnostics = Vec::new();
    for path in paths {
        load_theme_path(path.as_ref(), &mut themes, &mut diagnostics);
    }
    let (themes, mut collision_diagnostics) = dedupe_themes(themes);
    diagnostics.append(&mut collision_diagnostics);
    (themes, diagnostics)
}

fn load_theme_path(
    path: &Path,
    themes: &mut Vec<Theme>,
    diagnostics: &mut Vec<ResourceDiagnostic>,
) {
    if !path.exists() {
        diagnostics.push(error_diagnostic("Theme path does not exist", path));
        return;
    }
    if path.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            diagnostics.push(error_diagnostic("Could not list theme directory", path));
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                load_theme_path(&path, themes, diagnostics);
            }
        }
        return;
    }
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            diagnostics.push(error_diagnostic(
                format!("Could not read theme: {error}"),
                path,
            ));
            return;
        }
    };
    let json = match serde_json::from_str::<Value>(&content) {
        Ok(json) => json,
        Err(error) => {
            diagnostics.push(error_diagnostic(
                format!("Could not parse theme: {error}"),
                path,
            ));
            return;
        }
    };
    let name = json
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| basename_without_extension(path));
    themes.push(Theme {
        name,
        path: display_path(path),
        content: json,
        source_info: None,
    });
}

fn dedupe_themes(themes: Vec<Theme>) -> (Vec<Theme>, Vec<ResourceDiagnostic>) {
    let mut seen = BTreeMap::<String, Theme>::new();
    let mut diagnostics = Vec::new();
    for theme in themes {
        if let Some(existing) = seen.get(&theme.name) {
            diagnostics.push(ResourceDiagnostic {
                kind: ResourceDiagnosticKind::Collision,
                message: format!("name \"{}\" collision", theme.name),
                path: Some(theme.path.clone()),
                collision: Some(ResourceCollision {
                    resource_type: ResourceType::Theme,
                    name: theme.name.clone(),
                    winner_path: existing.path.clone(),
                    loser_path: theme.path,
                    winner_source: None,
                    loser_source: None,
                }),
            });
        } else {
            seen.insert(theme.name.clone(), theme);
        }
    }
    (seen.into_values().collect(), diagnostics)
}

fn error_diagnostic(message: impl Into<String>, path: &Path) -> ResourceDiagnostic {
    ResourceDiagnostic {
        kind: ResourceDiagnosticKind::Error,
        message: message.into(),
        path: Some(display_path(path)),
        collision: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::ResourceType;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn duplicate_theme_names_report_collision_like_pi() {
        let dir = temp_dir();
        let first = dir.join("first.json");
        let second = dir.join("second.json");
        fs::write(&first, r#"{"name":"work","color":"first"}"#).expect("first theme");
        fs::write(&second, r#"{"name":"work","color":"second"}"#).expect("second theme");

        let (themes, diagnostics) = load_themes([&first, &second]);

        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].content["color"], "first");
        let collision = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind == ResourceDiagnosticKind::Collision)
            .and_then(|diagnostic| diagnostic.collision.as_ref())
            .expect("duplicate theme should report collision");
        assert_eq!(collision.resource_type, ResourceType::Theme);
        assert_eq!(collision.name, "work");
        assert_eq!(collision.winner_path, display_path(&first));
        assert_eq!(collision.loser_path, display_path(&second));
    }

    #[test]
    fn custom_theme_uses_content_name_instead_of_file_name_like_pi_picker() {
        let dir = temp_dir();
        let path = dir.join("foo.json");
        fs::write(
            &path,
            r##"{
                "name": "bar",
                "colors": {
                    "accent": "#ffffff"
                }
            }"##,
        )
        .expect("theme should be written");

        let (themes, diagnostics) = load_themes([&path]);

        assert!(diagnostics.is_empty());
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].name, "bar");
        assert_eq!(themes[0].path, display_path(&path));
        assert_ne!(themes[0].name, "foo");
    }

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pm-agent-themes-test-{nanos}-{count}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}
