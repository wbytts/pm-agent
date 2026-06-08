use std::path::Path;

use crate::extensions::{DiscoveredExtensionResources, ExtensionResourcePath};
use crate::package_manager::{PathMetadata, SourceOrigin, SourceScope};

use super::{ResourceExtensionPaths, ResourcePath};

pub fn discovered_resources_to_paths(
    resources: DiscoveredExtensionResources,
) -> ResourceExtensionPaths {
    ResourceExtensionPaths {
        skill_paths: extension_resource_paths(resources.skill_paths),
        prompt_paths: extension_resource_paths(resources.prompt_paths),
        theme_paths: extension_resource_paths(resources.theme_paths),
    }
}

fn extension_resource_paths(paths: Vec<ExtensionResourcePath>) -> Vec<ResourcePath> {
    paths
        .into_iter()
        .map(|entry| {
            let base_dir = Path::new(&entry.extension_path)
                .parent()
                .map(|path| path.to_string_lossy().to_string());
            let metadata = entry.metadata.unwrap_or_else(|| PathMetadata {
                source: entry.extension_path.clone(),
                scope: SourceScope::Temporary,
                origin: SourceOrigin::TopLevel,
                base_dir,
            });
            ResourcePath {
                path: entry.path,
                metadata,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_discovered_extension_paths_to_resource_paths() {
        let paths = discovered_resources_to_paths(DiscoveredExtensionResources {
            skill_paths: vec![ExtensionResourcePath {
                path: "/skills/demo".to_string(),
                extension_path: "/extensions/demo/index.ts".to_string(),
                metadata: None,
            }],
            prompt_paths: Vec::new(),
            theme_paths: Vec::new(),
        });

        assert_eq!(paths.skill_paths[0].path, "/skills/demo");
        assert_eq!(
            paths.skill_paths[0].metadata.source,
            "/extensions/demo/index.ts"
        );
        assert_eq!(paths.skill_paths[0].metadata.scope, SourceScope::Temporary);
        assert_eq!(paths.skill_paths[0].metadata.origin, SourceOrigin::TopLevel);
        assert_eq!(
            paths.skill_paths[0].metadata.base_dir.as_deref(),
            Some("/extensions/demo")
        );
    }
}
