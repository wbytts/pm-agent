use serde_json::{json, Value};

use super::types::{Extension, ExtensionError, ExtensionEvent, ExtensionEventKind};

pub fn emit_before_provider_request(
    extensions: &[Extension],
    payload: Value,
    mut report_error: impl FnMut(ExtensionError),
) -> Value {
    let mut current_payload = payload;

    for extension in extensions {
        let Some(handlers) = extension.handlers.get("before_provider_request") else {
            continue;
        };
        for handler in handlers {
            let event = ExtensionEvent {
                kind: ExtensionEventKind::BeforeProviderRequest,
                payload: json!({
                    "type": "before_provider_request",
                    "payload": current_payload,
                }),
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(event)));
            match result {
                Ok(Some(value)) => current_payload = value,
                Ok(None) => {}
                Err(_) => report_error(ExtensionError {
                    extension_path: extension.path.clone(),
                    event: Some("before_provider_request".to_string()),
                    message: "Extension before_provider_request handler panicked".to_string(),
                }),
            }
        }
    }

    current_payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn chains_provider_payload_replacements() {
        let mut extension = Extension::new(
            "/extensions/provider-request.ts",
            create_synthetic_source_info(
                "/extensions/provider-request.ts",
                "local",
                None,
                None,
                None,
            ),
        );
        extension.handlers.insert(
            "before_provider_request".to_string(),
            vec![
                Arc::new(|event| {
                    assert_eq!(event.payload["type"], "before_provider_request");
                    Some(json!({ "messages": ["first"] }))
                }),
                Arc::new(|event| {
                    assert_eq!(event.payload["payload"]["messages"][0], "first");
                    Some(json!({ "messages": ["second"] }))
                }),
            ],
        );

        let payload =
            emit_before_provider_request(&[extension], json!({ "messages": ["original"] }), |_| {});

        assert_eq!(payload["messages"][0], "second");
    }

    #[test]
    fn reports_panicking_handler_and_keeps_payload() {
        let mut extension = Extension::new(
            "/extensions/panic-provider-request.ts",
            create_synthetic_source_info(
                "/extensions/panic-provider-request.ts",
                "local",
                None,
                None,
                None,
            ),
        );
        extension.handlers.insert(
            "before_provider_request".to_string(),
            vec![Arc::new(|_| panic!("bad provider request handler"))],
        );
        let mut errors = Vec::new();

        let payload = emit_before_provider_request(
            &[extension],
            json!({ "messages": ["original"] }),
            |error| errors.push((error.extension_path, error.event)),
        );

        assert_eq!(payload["messages"][0], "original");
        assert_eq!(
            errors,
            vec![(
                "/extensions/panic-provider-request.ts".to_string(),
                Some("before_provider_request".to_string())
            )]
        );
    }
}
