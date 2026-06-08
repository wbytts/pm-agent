use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::conversation::ToolCall;
use crate::types::ToolDefinition;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct JsonSchemaObject {
    #[serde(rename = "type")]
    pub schema_type: Option<Value>,
    #[serde(default)]
    pub properties: Option<Map<String, Value>>,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub items: Option<Value>,
    #[serde(default)]
    pub additional_properties: Option<Value>,
    #[serde(rename = "allOf", default)]
    pub all_of: Vec<Value>,
    #[serde(rename = "anyOf", default)]
    pub any_of: Vec<Value>,
    #[serde(rename = "oneOf", default)]
    pub one_of: Vec<Value>,
    #[serde(default)]
    pub r#enum: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

pub fn coerce_with_json_schema(value: Value, schema: &Value) -> Value {
    let Some(schema) = schema.as_object() else {
        return value;
    };
    let schema = parse_schema_object(schema);
    coerce_with_schema_object(value, &schema)
}

pub fn validate_tool_call(tools: &[ToolDefinition], tool_call: &ToolCall) -> Result<Value, String> {
    let Some(tool) = tools.iter().find(|tool| tool.name == tool_call.name) else {
        return Err(format!("Tool \"{}\" not found", tool_call.name));
    };
    validate_tool_arguments(tool, tool_call)
}

pub fn validate_tool_arguments(
    tool: &ToolDefinition,
    tool_call: &ToolCall,
) -> Result<Value, String> {
    let args = Value::Object(
        tool_call
            .arguments
            .clone()
            .into_iter()
            .collect::<Map<String, Value>>(),
    );
    let coerced = coerce_with_json_schema(args, &tool.parameters);
    let errors = validate_json_schema_value(&coerced, &tool.parameters);
    if errors.is_empty() {
        return Ok(coerced);
    }

    let errors = errors
        .iter()
        .map(|error| format!("  - {}: {}", error.path, error.message))
        .collect::<Vec<_>>()
        .join("\n");
    let received =
        serde_json::to_string_pretty(&tool_call.arguments).unwrap_or_else(|_| "{}".to_string());
    Err(format!(
        "Validation failed for tool \"{}\":\n{}\n\nReceived arguments:\n{}",
        tool_call.name, errors, received
    ))
}

pub fn validate_json_schema_value(value: &Value, schema: &Value) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    validate_json_schema_value_at(value, schema, "root", &mut errors);
    errors
}

fn coerce_with_schema_object(value: Value, schema: &JsonSchemaObject) -> Value {
    let mut next_value = value;

    for nested in &schema.all_of {
        next_value = coerce_with_json_schema(next_value, nested);
    }
    if !schema.any_of.is_empty() {
        next_value = coerce_with_union_schema(next_value, &schema.any_of);
    }
    if !schema.one_of.is_empty() {
        next_value = coerce_with_union_schema(next_value, &schema.one_of);
    }

    let types = schema_types(schema);
    let matches_union_member = types.len() > 1
        && types
            .iter()
            .any(|schema_type| matches_json_type(&next_value, schema_type));
    if !types.is_empty() && !matches_union_member {
        for schema_type in &types {
            let candidate = coerce_primitive_by_type(next_value.clone(), schema_type);
            if candidate != next_value {
                next_value = candidate;
                break;
            }
        }
    }

    if types.iter().any(|schema_type| schema_type == "object") {
        if let Value::Object(map) = next_value {
            next_value = Value::Object(apply_schema_object_coercion(map, schema));
        }
    }
    if types.iter().any(|schema_type| schema_type == "array") {
        if let Value::Array(values) = next_value {
            next_value = Value::Array(apply_schema_array_coercion(values, schema));
        }
    }

    next_value
}

fn coerce_with_union_schema(value: Value, schemas: &[Value]) -> Value {
    for schema in schemas {
        let candidate = coerce_with_json_schema(value.clone(), schema);
        if validate_json_schema_value(&candidate, schema).is_empty() {
            return candidate;
        }
    }
    value
}

fn apply_schema_object_coercion(
    mut value: Map<String, Value>,
    schema: &JsonSchemaObject,
) -> Map<String, Value> {
    if let Some(properties) = &schema.properties {
        for (key, property_schema) in properties {
            let Some(property_value) = value.remove(key) else {
                continue;
            };
            value.insert(
                key.clone(),
                coerce_with_json_schema(property_value, property_schema),
            );
        }
    }

    if let Some(additional_schema) = schema
        .additional_properties
        .as_ref()
        .filter(|value| value.is_object())
    {
        let defined_keys = schema
            .properties
            .as_ref()
            .map(|properties| properties.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for (key, property_value) in value.clone() {
            if defined_keys.iter().any(|defined_key| defined_key == &key) {
                continue;
            }
            value.insert(
                key,
                coerce_with_json_schema(property_value, additional_schema),
            );
        }
    }

    value
}

fn apply_schema_array_coercion(mut values: Vec<Value>, schema: &JsonSchemaObject) -> Vec<Value> {
    let Some(items) = &schema.items else {
        return values;
    };
    if let Some(item_schemas) = items.as_array() {
        for (index, item_schema) in item_schemas.iter().enumerate() {
            if let Some(value) = values.get_mut(index) {
                *value = coerce_with_json_schema(value.clone(), item_schema);
            }
        }
        return values;
    }
    if items.is_object() {
        for value in &mut values {
            *value = coerce_with_json_schema(value.clone(), items);
        }
    }
    values
}

fn coerce_primitive_by_type(value: Value, schema_type: &str) -> Value {
    match schema_type {
        "number" => match value {
            Value::Null => Value::from(0),
            Value::String(text) if !text.trim().is_empty() => text
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .unwrap_or(Value::String(text)),
            Value::Bool(value) => Value::from(if value { 1 } else { 0 }),
            value => value,
        },
        "integer" => match value {
            Value::Null => Value::from(0),
            Value::String(text) if !text.trim().is_empty() => text
                .parse::<i64>()
                .map(Value::from)
                .unwrap_or(Value::String(text)),
            Value::Bool(value) => Value::from(if value { 1 } else { 0 }),
            value => value,
        },
        "boolean" => match value {
            Value::Null => Value::Bool(false),
            Value::String(text) if text == "true" => Value::Bool(true),
            Value::String(text) if text == "false" => Value::Bool(false),
            Value::Number(number) if number.as_i64() == Some(1) => Value::Bool(true),
            Value::Number(number) if number.as_i64() == Some(0) => Value::Bool(false),
            value => value,
        },
        "string" => match value {
            Value::Null => Value::String(String::new()),
            Value::Number(number) => Value::String(number.to_string()),
            Value::Bool(value) => Value::String(value.to_string()),
            value => value,
        },
        "null" => match value {
            Value::String(text) if text.is_empty() => Value::Null,
            Value::Number(number) if number.as_i64() == Some(0) => Value::Null,
            Value::Bool(false) => Value::Null,
            value => value,
        },
        _ => value,
    }
}

fn validate_json_schema_value_at(
    value: &Value,
    schema: &Value,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    let Some(schema) = schema.as_object() else {
        return;
    };
    let schema = parse_schema_object(schema);
    for nested in &schema.all_of {
        validate_json_schema_value_at(value, nested, path, errors);
    }
    if !schema.any_of.is_empty()
        && !schema
            .any_of
            .iter()
            .any(|nested| validate_json_schema_value(value, nested).is_empty())
    {
        errors.push(ValidationError {
            path: path.to_string(),
            message: "does not match any allowed schema".to_string(),
        });
    }
    if !schema.one_of.is_empty() {
        let matches = schema
            .one_of
            .iter()
            .filter(|nested| validate_json_schema_value(value, nested).is_empty())
            .count();
        if matches != 1 {
            errors.push(ValidationError {
                path: path.to_string(),
                message: "does not match exactly one allowed schema".to_string(),
            });
        }
    }

    let types = schema_types(&schema);
    if !types.is_empty()
        && !types
            .iter()
            .any(|schema_type| matches_json_type(value, schema_type))
    {
        errors.push(ValidationError {
            path: path.to_string(),
            message: format!("expected {}", types.join(" or ")),
        });
        return;
    }

    if let Some(enum_values) = &schema.r#enum {
        if !enum_values.iter().any(|enum_value| enum_value == value) {
            errors.push(ValidationError {
                path: path.to_string(),
                message: "must be one of the allowed values".to_string(),
            });
        }
    }

    if let Value::Object(map) = value {
        for required in &schema.required {
            if !map.contains_key(required) {
                errors.push(ValidationError {
                    path: join_path(path, required),
                    message: "is required".to_string(),
                });
            }
        }
        if let Some(properties) = &schema.properties {
            for (key, property_schema) in properties {
                if let Some(property_value) = map.get(key) {
                    validate_json_schema_value_at(
                        property_value,
                        property_schema,
                        &join_path(path, key),
                        errors,
                    );
                }
            }
        }
    }

    if let (Value::Array(values), Some(items)) = (value, schema.items.as_ref()) {
        if let Some(item_schemas) = items.as_array() {
            for (index, item_schema) in item_schemas.iter().enumerate() {
                if let Some(item_value) = values.get(index) {
                    validate_json_schema_value_at(
                        item_value,
                        item_schema,
                        &join_path(path, &index.to_string()),
                        errors,
                    );
                }
            }
        } else if items.is_object() {
            for (index, item_value) in values.iter().enumerate() {
                validate_json_schema_value_at(
                    item_value,
                    items,
                    &join_path(path, &index.to_string()),
                    errors,
                );
            }
        }
    }
}

fn parse_schema_object(schema: &Map<String, Value>) -> JsonSchemaObject {
    serde_json::from_value(Value::Object(schema.clone())).unwrap_or_default()
}

fn schema_types(schema: &JsonSchemaObject) -> Vec<String> {
    match &schema.schema_type {
        Some(Value::String(value)) => vec![value.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn matches_json_type(value: &Value, schema_type: &str) -> bool {
    match schema_type {
        "number" => value.as_f64().is_some(),
        "integer" => value.as_i64().is_some(),
        "boolean" => value.is_boolean(),
        "string" => value.is_string(),
        "null" => value.is_null(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => false,
    }
}

fn join_path(base: &str, key: &str) -> String {
    if base == "root" {
        key.to_string()
    } else {
        format!("{base}.{key}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn coerces_object_values_with_json_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": { "type": "integer" },
                "enabled": { "type": "boolean" },
                "name": { "type": "string" }
            }
        });
        let value = json!({"count":"42","enabled":"true","name":7});

        assert_eq!(
            coerce_with_json_schema(value, &schema),
            json!({"count":42,"enabled":true,"name":"7"})
        );
    }

    #[test]
    fn validates_required_and_type_errors() {
        let schema = json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": { "type": "string" },
                "recursive": { "type": "boolean" }
            }
        });
        let errors = validate_json_schema_value(&json!({"recursive":"no"}), &schema);

        assert!(errors.iter().any(|error| error.path == "path"));
        assert!(errors.iter().any(|error| error.path == "recursive"));
    }

    #[test]
    fn validates_tool_call_and_returns_coerced_arguments() {
        let tools = vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "读取文件".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }),
        }];
        let tool_call = ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: BTreeMap::from([
                ("path".to_string(), json!("/tmp/a")),
                ("limit".to_string(), json!("10")),
            ]),
            thought_signature: None,
        };

        let result = validate_tool_call(&tools, &tool_call).expect("valid call");

        assert_eq!(result["limit"], 10);
    }

    #[test]
    fn reports_missing_tool() {
        let error = validate_tool_call(
            &[],
            &ToolCall {
                id: "call-1".to_string(),
                name: "missing".to_string(),
                arguments: BTreeMap::new(),
                thought_signature: None,
            },
        )
        .expect_err("missing tool");

        assert_eq!(error, "Tool \"missing\" not found");
    }
}
