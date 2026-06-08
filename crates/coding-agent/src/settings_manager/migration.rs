use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

pub fn migrate_settings(mut settings: Value) -> Value {
    let object = object_mut(&mut settings);
    if let Some(queue_mode) = object.remove("queueMode") {
        object
            .entry("steeringMode".to_string())
            .or_insert(queue_mode);
    }

    if !object.contains_key("transport") {
        if let Some(websockets) = object
            .remove("websockets")
            .and_then(|value| value.as_bool())
        {
            object.insert(
                "transport".to_string(),
                Value::String(if websockets { "websocket" } else { "sse" }.to_string()),
            );
        }
    }

    migrate_legacy_skills(object);
    migrate_retry_max_delay(object);
    settings
}

pub fn migrate_commands_to_prompts(base_dir: impl AsRef<Path>) -> Result<bool, String> {
    let base_dir = base_dir.as_ref();
    let commands_dir = base_dir.join("commands");
    let prompts_dir = base_dir.join("prompts");

    if !commands_dir.exists() || prompts_dir.exists() {
        return Ok(false);
    }

    fs::rename(&commands_dir, &prompts_dir)
        .map(|_| true)
        .map_err(|error| {
            format!(
                "迁移 commands/ 到 prompts/ 失败：{} -> {}：{error}",
                commands_dir.display(),
                prompts_dir.display()
            )
        })
}

fn migrate_legacy_skills(object: &mut Map<String, Value>) {
    let Some(skills) = object.get("skills") else {
        return;
    };
    if !skills.is_object() || skills.is_array() {
        return;
    }
    let skills = skills.clone();
    if let Some(enabled) = skills.get("enableSkillCommands").and_then(Value::as_bool) {
        object
            .entry("enableSkillCommands".to_string())
            .or_insert(Value::Bool(enabled));
    }
    if let Some(custom_directories) = skills.get("customDirectories").and_then(Value::as_array) {
        if !custom_directories.is_empty() {
            object.insert(
                "skills".to_string(),
                Value::Array(custom_directories.clone()),
            );
            return;
        }
    }
    object.remove("skills");
}

fn migrate_retry_max_delay(object: &mut Map<String, Value>) {
    let Some(retry) = object.get_mut("retry").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(max_delay) = retry.remove("maxDelayMs") else {
        return;
    };
    let provider = retry
        .entry("provider".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    object_mut(provider)
        .entry("maxRetryDelayMs".to_string())
        .or_insert(max_delay);
}

pub(super) fn object_mut(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("value should be object")
}
