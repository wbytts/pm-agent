use serde_json::{json, Value};

use crate::package_manager::PathMetadata;

use super::types::{Extension, ExtensionError, ExtensionEvent, ExtensionEventKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourcesDiscoverReason {
    Startup,
    Reload,
}

impl ResourcesDiscoverReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Reload => "reload",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionResourcePath {
    pub path: String,
    pub extension_path: String,
    pub metadata: Option<PathMetadata>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveredExtensionResources {
    pub skill_paths: Vec<ExtensionResourcePath>,
    pub prompt_paths: Vec<ExtensionResourcePath>,
    pub theme_paths: Vec<ExtensionResourcePath>,
}

pub fn emit_resources_discover(
    extensions: &[Extension],
    cwd: &str,
    reason: ResourcesDiscoverReason,
    mut report_error: impl FnMut(ExtensionError),
) -> DiscoveredExtensionResources {
    let mut resources = DiscoveredExtensionResources::default();
    let event = ExtensionEvent {
        kind: ExtensionEventKind::ResourcesDiscover,
        payload: json!({
            "type": "resources_discover",
            "cwd": cwd,
            "reason": reason.as_str(),
        }),
    };

    for extension in extensions {
        let Some(handlers) = extension.handlers.get("resources_discover") else {
            continue;
        };
        for handler in handlers {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(event.clone())));
            match result {
                Ok(Some(value)) => collect_result(&mut resources, extension, &value),
                Ok(None) => {}
                Err(_) => report_error(ExtensionError {
                    extension_path: extension.path.clone(),
                    event: Some("resources_discover".to_string()),
                    message: "Extension resources_discover handler panicked".to_string(),
                }),
            }
        }
    }

    resources
}

fn collect_result(
    resources: &mut DiscoveredExtensionResources,
    extension: &Extension,
    value: &Value,
) {
    collect_paths(
        &mut resources.skill_paths,
        extension,
        value.get("skillPaths"),
    );
    collect_paths(
        &mut resources.prompt_paths,
        extension,
        value.get("promptPaths"),
    );
    collect_paths(
        &mut resources.theme_paths,
        extension,
        value.get("themePaths"),
    );
}

fn collect_paths(
    target: &mut Vec<ExtensionResourcePath>,
    extension: &Extension,
    value: Option<&Value>,
) {
    let Some(paths) = value.and_then(Value::as_array) else {
        return;
    };
    target.extend(
        paths
            .iter()
            .filter_map(Value::as_str)
            .map(|path| ExtensionResourcePath {
                path: path.to_string(),
                extension_path: extension.path.clone(),
                metadata: Some(PathMetadata {
                    source: extension.source_info.source.clone(),
                    scope: extension.source_info.scope,
                    origin: extension.source_info.origin,
                    base_dir: extension.source_info.base_dir.clone(),
                }),
            }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package_manager::{SourceOrigin, SourceScope};
    use crate::source_info::create_synthetic_source_info;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[test]
    fn collects_resources_discover_paths_with_extension_path() {
        let mut extension = Extension::new(
            "/extensions/resources.ts",
            create_synthetic_source_info("/extensions/resources.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "resources_discover".to_string(),
            vec![Arc::new(|event| {
                assert_eq!(event.payload["cwd"], "/tmp/project");
                assert_eq!(event.payload["reason"], "startup");
                Some(json!({
                    "skillPaths": ["/skills/demo"],
                    "promptPaths": ["/prompts/review.md"],
                    "themePaths": ["/themes/work.json"],
                }))
            })],
        );

        let resources = emit_resources_discover(
            &[extension],
            "/tmp/project",
            ResourcesDiscoverReason::Startup,
            |_| {},
        );

        assert_eq!(resources.skill_paths[0].path, "/skills/demo");
        assert_eq!(
            resources.skill_paths[0].extension_path,
            "/extensions/resources.ts"
        );
        assert_eq!(resources.prompt_paths[0].path, "/prompts/review.md");
        assert_eq!(resources.theme_paths[0].path, "/themes/work.json");
    }

    #[test]
    fn resources_discover_paths_inherit_extension_source_info_like_pi() {
        let mut extension = Extension::new(
            "/packages/pkg/extensions/resources.ts",
            create_synthetic_source_info(
                "/packages/pkg/extensions/resources.ts",
                "npm:pkg",
                Some(SourceScope::User),
                Some(SourceOrigin::Package),
                Some("/packages/pkg".to_string()),
            ),
        );
        extension.handlers.insert(
            "resources_discover".to_string(),
            vec![Arc::new(|_| {
                Some(json!({
                    "promptPaths": ["/packages/pkg/prompts/review.md"],
                }))
            })],
        );

        let resources = emit_resources_discover(
            &[extension],
            "/tmp/project",
            ResourcesDiscoverReason::Reload,
            |_| {},
        );

        let metadata = resources.prompt_paths[0]
            .metadata
            .as_ref()
            .expect("metadata should inherit extension source info");
        assert_eq!(metadata.source, "npm:pkg");
        assert_eq!(metadata.scope, SourceScope::User);
        assert_eq!(metadata.origin, SourceOrigin::Package);
        assert_eq!(metadata.base_dir.as_deref(), Some("/packages/pkg"));
    }

    #[test]
    fn reports_panicking_resources_discover_handler() {
        let mut extension = Extension::new(
            "/extensions/bad.ts",
            create_synthetic_source_info("/extensions/bad.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "resources_discover".to_string(),
            vec![Arc::new(|_| panic!("bad handler"))],
        );
        let mut errors = BTreeMap::<String, String>::new();

        let resources = emit_resources_discover(
            &[extension],
            "/tmp/project",
            ResourcesDiscoverReason::Reload,
            |error| {
                errors.insert(error.extension_path, error.event.unwrap_or_default());
            },
        );

        assert!(resources.skill_paths.is_empty());
        assert_eq!(
            errors.get("/extensions/bad.ts").map(String::as_str),
            Some("resources_discover")
        );
    }
}
