use serde_json::{json, Value};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StringEnumOptions {
    pub description: Option<String>,
    pub default: Option<String>,
}

pub fn string_enum<T>(values: &[T], options: Option<StringEnumOptions>) -> Value
where
    T: AsRef<str>,
{
    let mut schema = json!({
        "type": "string",
        "enum": values.iter().map(|value| value.as_ref()).collect::<Vec<_>>(),
    });

    let Some(options) = options else {
        return schema;
    };
    if let Some(description) = options.description {
        schema["description"] = Value::String(description);
    }
    if let Some(default) = options.default {
        schema["default"] = Value::String(default);
    }
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_string_enum_schema() {
        let schema = string_enum(
            &["add", "subtract"],
            Some(StringEnumOptions {
                description: Some("operation".to_string()),
                default: Some("add".to_string()),
            }),
        );

        assert_eq!(
            schema,
            json!({
                "type": "string",
                "enum": ["add", "subtract"],
                "description": "operation",
                "default": "add"
            })
        );
    }
}
