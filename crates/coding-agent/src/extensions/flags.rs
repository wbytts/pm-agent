use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::types::{Extension, ExtensionFlag, ExtensionFlagType};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedExtensionFlagValues {
    pub flag_values: BTreeMap<String, Value>,
    pub diagnostics: Vec<ExtensionFlagDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionFlagDiagnostic {
    pub message: String,
}

pub fn apply_extension_flag_values(
    registered_flags: &BTreeMap<String, ExtensionFlag>,
    requested_values: &BTreeMap<String, Value>,
) -> AppliedExtensionFlagValues {
    let mut flag_values = BTreeMap::new();
    let mut diagnostics = Vec::new();
    let mut unknown_flags = Vec::new();

    for (name, value) in requested_values {
        let Some(flag) = registered_flags.get(name) else {
            unknown_flags.push(name.clone());
            continue;
        };
        match flag.flag_type {
            ExtensionFlagType::Boolean => {
                flag_values.insert(name.clone(), Value::Bool(true));
            }
            ExtensionFlagType::String => {
                if let Some(value) = value.as_str() {
                    flag_values.insert(name.clone(), Value::String(value.to_string()));
                } else {
                    diagnostics.push(ExtensionFlagDiagnostic {
                        message: format!("Extension flag \"--{name}\" requires a value"),
                    });
                }
            }
        }
    }

    if !unknown_flags.is_empty() {
        let options = unknown_flags
            .iter()
            .map(|name| format!("--{name}"))
            .collect::<Vec<_>>()
            .join(", ");
        diagnostics.push(ExtensionFlagDiagnostic {
            message: format!(
                "Unknown option{}: {options}",
                if unknown_flags.len() == 1 { "" } else { "s" }
            ),
        });
    }

    AppliedExtensionFlagValues {
        flag_values,
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;
    use serde_json::json;

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
                flag_type: Default::default(),
                description: Some("first".to_string()),
            },
        );
        second.flags.insert(
            "demo".to_string(),
            ExtensionFlag {
                name: "demo".to_string(),
                flag_type: Default::default(),
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

    #[test]
    fn applies_extension_flag_values_like_pi_session_services() {
        let mut registered = BTreeMap::new();
        registered.insert(
            "verbose".to_string(),
            ExtensionFlag {
                name: "verbose".to_string(),
                flag_type: ExtensionFlagType::Boolean,
                description: None,
            },
        );
        registered.insert(
            "profile".to_string(),
            ExtensionFlag {
                name: "profile".to_string(),
                flag_type: ExtensionFlagType::String,
                description: None,
            },
        );
        registered.insert(
            "missing-value".to_string(),
            ExtensionFlag {
                name: "missing-value".to_string(),
                flag_type: ExtensionFlagType::String,
                description: None,
            },
        );

        let values = BTreeMap::from([
            ("verbose".to_string(), json!(false)),
            ("profile".to_string(), json!("prod")),
            ("missing-value".to_string(), json!(true)),
            ("unknown".to_string(), json!("x")),
        ]);

        let applied = apply_extension_flag_values(&registered, &values);

        assert_eq!(applied.flag_values.get("verbose"), Some(&json!(true)));
        assert_eq!(applied.flag_values.get("profile"), Some(&json!("prod")));
        assert_eq!(applied.diagnostics.len(), 2);
        assert_eq!(
            applied.diagnostics[0].message,
            "Extension flag \"--missing-value\" requires a value"
        );
        assert_eq!(applied.diagnostics[1].message, "Unknown option: --unknown");
    }
}
