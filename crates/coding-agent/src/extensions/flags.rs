use std::collections::{BTreeMap, BTreeSet};

use super::types::{Extension, ExtensionFlag};

pub fn resolve_flags(extensions: &[Extension]) -> BTreeMap<String, ExtensionFlag> {
    let mut seen = BTreeSet::<String>::new();
    let mut flags = BTreeMap::<String, ExtensionFlag>::new();

    for extension in extensions {
        for (name, flag) in &extension.flags {
            if seen.insert(name.clone()) {
                flags.insert(name.clone(), flag.clone());
            }
        }
    }

    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;

    #[test]
    fn resolves_flags_with_first_registration_wins_like_pi() {
        let mut first = Extension::new(
            "/extensions/one.ts",
            create_synthetic_source_info("/extensions/one.ts", "local", None, None, None),
        );
        let mut second = Extension::new(
            "/extensions/two.ts",
            create_synthetic_source_info("/extensions/two.ts", "local", None, None, None),
        );
        first.flags.insert(
            "demo".to_string(),
            ExtensionFlag {
                name: "demo".to_string(),
                description: Some("first".to_string()),
            },
        );
        second.flags.insert(
            "demo".to_string(),
            ExtensionFlag {
                name: "demo".to_string(),
                description: Some("second".to_string()),
            },
        );

        let flags = resolve_flags(&[first, second]);

        assert_eq!(flags.len(), 1);
        assert_eq!(
            flags
                .get("demo")
                .and_then(|flag| flag.description.as_deref()),
            Some("first")
        );
    }
}
