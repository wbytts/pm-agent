use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{Extension, ExtensionError, ExtensionEvent, ExtensionEventKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserBashEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub command: String,
    pub exclude_from_context: bool,
    pub cwd: String,
}

impl UserBashEvent {
    pub fn new(
        command: impl Into<String>,
        exclude_from_context: bool,
        cwd: impl Into<String>,
    ) -> Self {
        Self {
            event_type: "user_bash".to_string(),
            command: command.into(),
            exclude_from_context,
            cwd: cwd.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserBashResult {
    pub value: Value,
}

pub fn emit_user_bash(
    extensions: &[Extension],
    event: UserBashEvent,
    mut report_error: impl FnMut(ExtensionError),
) -> Option<UserBashResult> {
    for extension in extensions {
        let Some(handlers) = extension.handlers.get("user_bash") else {
            continue;
        };
        for handler in handlers {
            let extension_event = ExtensionEvent {
                kind: ExtensionEventKind::UserBash,
                payload: serde_json::to_value(&event).unwrap_or(Value::Null),
            };
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(extension_event)));
            match result {
                Ok(Some(value)) => return Some(UserBashResult { value }),
                Ok(None) => {}
                Err(_) => report_error(ExtensionError {
                    extension_path: extension.path.clone(),
                    event: Some("user_bash".to_string()),
                    message: "Extension user_bash handler panicked".to_string(),
                }),
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn returns_first_handler_result() {
        let mut extension = Extension::new(
            "/extensions/user-bash.ts",
            create_synthetic_source_info("/extensions/user-bash.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "user_bash".to_string(),
            vec![
                Arc::new(|event| {
                    assert_eq!(event.payload["type"], "user_bash");
                    assert_eq!(event.payload["excludeFromContext"], true);
                    Some(json!({ "result": { "stdout": "handled" } }))
                }),
                Arc::new(|_| Some(json!({ "result": { "stdout": "ignored" } }))),
            ],
        );

        let result = emit_user_bash(
            &[extension],
            UserBashEvent::new("pwd", true, "/tmp/project"),
            |_| {},
        )
        .expect("user bash should be handled");

        assert_eq!(result.value["result"]["stdout"], "handled");
    }

    #[test]
    fn reports_panicking_handler() {
        let mut extension = Extension::new(
            "/extensions/panic-user-bash.ts",
            create_synthetic_source_info(
                "/extensions/panic-user-bash.ts",
                "local",
                None,
                None,
                None,
            ),
        );
        extension.handlers.insert(
            "user_bash".to_string(),
            vec![Arc::new(|_| panic!("bad user bash handler"))],
        );
        let mut errors = Vec::new();

        let result = emit_user_bash(
            &[extension],
            UserBashEvent::new("pwd", false, "/tmp/project"),
            |error| errors.push((error.extension_path, error.event)),
        );

        assert!(result.is_none());
        assert_eq!(
            errors,
            vec![(
                "/extensions/panic-user-bash.ts".to_string(),
                Some("user_bash".to_string())
            )]
        );
    }
}
