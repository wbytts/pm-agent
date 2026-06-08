use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticErrorInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessageDiagnostic {
    pub r#type: String,
    pub timestamp_millis: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DiagnosticErrorInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<BTreeMap<String, Value>>,
}

pub trait DiagnosticTarget {
    fn diagnostics_mut(&mut self) -> &mut Vec<AssistantMessageDiagnostic>;
}

pub fn format_thrown_value(value: impl ToString) -> String {
    value.to_string()
}

pub fn extract_diagnostic_error(error: &(dyn Error + 'static)) -> DiagnosticErrorInfo {
    DiagnosticErrorInfo {
        name: Some(error_type_name(error).to_string()),
        message: if error.to_string().is_empty() {
            error_type_name(error).to_string()
        } else {
            error.to_string()
        },
        stack: None,
        code: None,
    }
}

pub fn diagnostic_error_from_message(message: impl Into<String>) -> DiagnosticErrorInfo {
    DiagnosticErrorInfo {
        name: Some("ThrownValue".to_string()),
        message: message.into(),
        stack: None,
        code: None,
    }
}

pub fn create_assistant_message_diagnostic(
    diagnostic_type: impl Into<String>,
    error: Option<DiagnosticErrorInfo>,
    details: Option<BTreeMap<String, Value>>,
) -> AssistantMessageDiagnostic {
    AssistantMessageDiagnostic {
        r#type: diagnostic_type.into(),
        timestamp_millis: now_millis(),
        error,
        details,
    }
}

pub fn create_error_diagnostic(
    diagnostic_type: impl Into<String>,
    error: &(dyn Error + 'static),
    details: Option<BTreeMap<String, Value>>,
) -> AssistantMessageDiagnostic {
    create_assistant_message_diagnostic(
        diagnostic_type,
        Some(extract_diagnostic_error(error)),
        details,
    )
}

pub fn create_message_diagnostic(
    diagnostic_type: impl Into<String>,
    message: impl Into<String>,
    details: Option<BTreeMap<String, Value>>,
) -> AssistantMessageDiagnostic {
    create_assistant_message_diagnostic(
        diagnostic_type,
        Some(diagnostic_error_from_message(message)),
        details,
    )
}

pub fn append_assistant_message_diagnostic<T>(
    message: &mut T,
    diagnostic: AssistantMessageDiagnostic,
) where
    T: DiagnosticTarget,
{
    message.diagnostics_mut().push(diagnostic);
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn error_type_name(_error: &(dyn Error + 'static)) -> &'static str {
    std::any::type_name::<dyn Error>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("provider failed")]
    struct ProviderError;

    #[derive(Default)]
    struct TestMessage {
        diagnostics: Vec<AssistantMessageDiagnostic>,
    }

    impl DiagnosticTarget for TestMessage {
        fn diagnostics_mut(&mut self) -> &mut Vec<AssistantMessageDiagnostic> {
            &mut self.diagnostics
        }
    }

    #[test]
    fn creates_diagnostic_from_error() {
        let diagnostic = create_error_diagnostic("provider_error", &ProviderError, None);

        assert_eq!(diagnostic.r#type, "provider_error");
        assert_eq!(diagnostic.error.expect("error").message, "provider failed");
    }

    #[test]
    fn appends_diagnostics_to_target() {
        let mut message = TestMessage::default();
        append_assistant_message_diagnostic(
            &mut message,
            create_message_diagnostic("runtime", "failed", None),
        );

        assert_eq!(message.diagnostics.len(), 1);
        assert_eq!(
            message.diagnostics[0]
                .error
                .as_ref()
                .map(|error| error.message.as_str()),
            Some("failed")
        );
    }
}
