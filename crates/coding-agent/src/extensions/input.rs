use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{Extension, ExtensionError, ExtensionEvent, ExtensionEventKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputSource {
    Interactive,
    Rpc,
    Extension,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InputEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Value>,
    pub source: InputSource,
}

impl InputEvent {
    pub fn new(text: impl Into<String>, images: Option<Value>, source: InputSource) -> Self {
        Self {
            event_type: "input".to_string(),
            text: text.into(),
            images,
            source,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum InputEventResult {
    #[serde(rename = "continue")]
    Continue,
    #[serde(rename = "transform")]
    Transform {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        images: Option<Value>,
    },
    #[serde(rename = "handled")]
    Handled,
}

pub fn emit_input(
    extensions: &[Extension],
    text: impl Into<String>,
    images: Option<Value>,
    source: InputSource,
    mut report_error: impl FnMut(ExtensionError),
) -> InputEventResult {
    let original_text = text.into();
    let original_images = images;
    let mut current_text = original_text.clone();
    let mut current_images = original_images.clone();

    for extension in extensions {
        let Some(handlers) = extension.handlers.get("input") else {
            continue;
        };
        for handler in handlers {
            let event =
                InputEvent::new(current_text.clone(), current_images.clone(), source.clone());
            let extension_event = ExtensionEvent {
                kind: ExtensionEventKind::Input,
                payload: serde_json::to_value(&event).unwrap_or(Value::Null),
            };
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(extension_event)));
            match result {
                Ok(Some(value)) => {
                    let Some(input_result) = parse_input_result(&value) else {
                        continue;
                    };
                    match input_result {
                        InputEventResult::Continue => {}
                        InputEventResult::Handled => return InputEventResult::Handled,
                        InputEventResult::Transform { text, images } => {
                            current_text = text;
                            if images.is_some() {
                                current_images = images;
                            }
                        }
                    }
                }
                Ok(None) => {}
                Err(_) => report_error(ExtensionError {
                    extension_path: extension.path.clone(),
                    event: Some("input".to_string()),
                    message: "Extension input handler panicked".to_string(),
                }),
            }
        }
    }

    if current_text != original_text || current_images != original_images {
        InputEventResult::Transform {
            text: current_text,
            images: current_images,
        }
    } else {
        InputEventResult::Continue
    }
}

fn parse_input_result(value: &Value) -> Option<InputEventResult> {
    serde_json::from_value(value.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_info::create_synthetic_source_info;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn chains_transform_results() {
        let mut extension = Extension::new(
            "/extensions/input.ts",
            create_synthetic_source_info("/extensions/input.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "input".to_string(),
            vec![
                Arc::new(|event| {
                    assert_eq!(event.payload["type"], "input");
                    Some(json!({ "action": "transform", "text": "first" }))
                }),
                Arc::new(|event| {
                    assert_eq!(event.payload["text"], "first");
                    Some(json!({ "action": "transform", "text": "second", "images": [{ "type": "image" }] }))
                }),
            ],
        );

        let result = emit_input(&[extension], "original", None, InputSource::Rpc, |_| {});

        assert_eq!(
            result,
            InputEventResult::Transform {
                text: "second".to_string(),
                images: Some(json!([{ "type": "image" }]))
            }
        );
    }

    #[test]
    fn handled_stops_processing() {
        let mut extension = Extension::new(
            "/extensions/input-handled.ts",
            create_synthetic_source_info("/extensions/input-handled.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "input".to_string(),
            vec![
                Arc::new(|_| Some(json!({ "action": "handled" }))),
                Arc::new(|_| Some(json!({ "action": "transform", "text": "ignored" }))),
            ],
        );

        let result = emit_input(
            &[extension],
            "original",
            None,
            InputSource::Interactive,
            |_| {},
        );

        assert_eq!(result, InputEventResult::Handled);
    }

    #[test]
    fn reports_panicking_handler() {
        let mut extension = Extension::new(
            "/extensions/panic-input.ts",
            create_synthetic_source_info("/extensions/panic-input.ts", "local", None, None, None),
        );
        extension.handlers.insert(
            "input".to_string(),
            vec![Arc::new(|_| panic!("bad input handler"))],
        );
        let mut errors = Vec::new();

        let result = emit_input(
            &[extension],
            "original",
            None,
            InputSource::Extension,
            |error| errors.push((error.extension_path, error.event)),
        );

        assert_eq!(result, InputEventResult::Continue);
        assert_eq!(
            errors,
            vec![(
                "/extensions/panic-input.ts".to_string(),
                Some("input".to_string())
            )]
        );
    }
}
