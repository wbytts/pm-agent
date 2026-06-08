use crate::keybindings::migrate_keybindings_config;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
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

pub fn migrate_auth_to_auth_json(agent_dir: impl AsRef<Path>) -> Result<Vec<String>, String> {
    let agent_dir = agent_dir.as_ref();
    let auth_path = agent_dir.join("auth.json");
    let oauth_path = agent_dir.join("oauth.json");
    let settings_path = agent_dir.join("settings.json");

    if auth_path.exists() {
        return Ok(Vec::new());
    }

    let mut migrated = Map::new();
    let mut providers = Vec::new();

    if oauth_path.exists() {
        let oauth = read_json_object(&oauth_path)?;
        for (provider, credential) in oauth {
            let mut value = object_from_value(credential);
            value.insert("type".to_string(), Value::String("oauth".to_string()));
            migrated.insert(provider.clone(), Value::Object(value));
            providers.push(provider);
        }
        fs::rename(
            &oauth_path,
            oauth_path.with_file_name("oauth.json.migrated"),
        )
        .map_err(|error| {
            format!(
                "迁移 oauth.json 到 oauth.json.migrated 失败：{}：{error}",
                oauth_path.display()
            )
        })?;
    }

    if settings_path.exists() {
        let mut settings = read_json_object(&settings_path)?;
        if let Some(api_keys) = settings.remove("apiKeys") {
            if let Some(api_keys) = api_keys.as_object() {
                for (provider, key) in api_keys {
                    if migrated.contains_key(provider) {
                        continue;
                    }
                    if let Some(key) = key.as_str() {
                        migrated.insert(
                            provider.clone(),
                            serde_json::json!({
                                "type": "api_key",
                                "key": key
                            }),
                        );
                        providers.push(provider.clone());
                    }
                }
            }
            write_json_object(&settings_path, &settings)?;
        }
    }

    if !migrated.is_empty() {
        write_json_object(&auth_path, &migrated)?;
        set_owner_only_permissions(&auth_path)?;
    }

    Ok(providers)
}

pub fn migrate_tools_to_bin(agent_dir: impl AsRef<Path>) -> Result<bool, String> {
    let agent_dir = agent_dir.as_ref();
    let tools_dir = agent_dir.join("tools");
    let bin_dir = agent_dir.join("bin");

    if !tools_dir.exists() {
        return Ok(false);
    }

    let mut moved_any = false;
    for binary in ["fd", "rg", "fd.exe", "rg.exe"] {
        let old_path = tools_dir.join(binary);
        if !old_path.exists() {
            continue;
        }

        fs::create_dir_all(&bin_dir)
            .map_err(|error| format!("创建 bin 目录 {} 失败：{error}", bin_dir.display()))?;
        let new_path = bin_dir.join(binary);
        if new_path.exists() {
            fs::remove_file(&old_path)
                .map_err(|error| format!("删除旧工具 {} 失败：{error}", old_path.display()))?;
            continue;
        }

        fs::rename(&old_path, &new_path).map_err(|error| {
            format!(
                "迁移工具到 bin/ 失败：{} -> {}：{error}",
                old_path.display(),
                new_path.display()
            )
        })?;
        moved_any = true;
    }

    Ok(moved_any)
}

pub fn migrate_keybindings_config_file(path: impl AsRef<Path>) -> Result<bool, String> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(path)
        .map_err(|error| format!("读取 keybindings 文件 {} 失败：{error}", path.display()))?;
    let value: Value = serde_json::from_str(&content)
        .map_err(|error| format!("解析 keybindings 文件 {} 失败：{error}", path.display()))?;
    let Some(object) = value.as_object() else {
        return Ok(false);
    };

    let raw = object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let (config, migrated) = migrate_keybindings_config(&raw);
    if !migrated {
        return Ok(false);
    }

    let content = format!(
        "{}\n",
        serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?
    );
    fs::write(path, content)
        .map_err(|error| format!("写入 keybindings 文件 {} 失败：{error}", path.display()))?;
    Ok(true)
}

pub fn migrate_sessions_from_agent_root(agent_dir: impl AsRef<Path>) -> Result<usize, String> {
    let agent_dir = agent_dir.as_ref();
    if !agent_dir.exists() {
        return Ok(0);
    }

    let mut migrated = 0;
    let entries = fs::read_dir(agent_dir)
        .map_err(|error| format!("读取 agent 目录 {} 失败：{error}", agent_dir.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("读取 agent 目录项 {} 失败：{error}", agent_dir.display()))?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }

        let Some(cwd) = session_cwd_from_first_line(&path)? else {
            continue;
        };
        let Some(file_name) = path.file_name() else {
            continue;
        };
        let target_dir = agent_dir.join("sessions").join(encoded_session_dir(&cwd));
        let target_path = target_dir.join(file_name);
        if target_path.exists() {
            continue;
        }

        fs::create_dir_all(&target_dir)
            .map_err(|error| format!("创建 session 目录 {} 失败：{error}", target_dir.display()))?;
        fs::rename(&path, &target_path).map_err(|error| {
            format!(
                "迁移 session 文件失败：{} -> {}：{error}",
                path.display(),
                target_path.display()
            )
        })?;
        migrated += 1;
    }

    Ok(migrated)
}

pub fn deprecated_extension_dir_warnings(
    base_dir: impl AsRef<Path>,
    label: &str,
) -> Result<Vec<String>, String> {
    let base_dir = base_dir.as_ref();
    let hooks_dir = base_dir.join("hooks");
    let tools_dir = base_dir.join("tools");
    let mut warnings = Vec::new();

    if hooks_dir.exists() {
        warnings.push(format!(
            "{label} hooks/ directory found. Hooks have been renamed to extensions."
        ));
    }

    if tools_dir.exists() && tools_dir_has_custom_entries(&tools_dir)? {
        warnings.push(format!(
            "{label} tools/ directory contains custom tools. Custom tools have been merged into extensions."
        ));
    }

    Ok(warnings)
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

fn read_json_object(path: &Path) -> Result<Map<String, Value>, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
    let value: Value = serde_json::from_str(&content)
        .map_err(|error| format!("解析 {} 失败：{error}", path.display()))?;
    Ok(value.as_object().cloned().unwrap_or_default())
}

fn write_json_object(path: &Path, object: &Map<String, Value>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建目录 {} 失败：{error}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(object).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| format!("写入 {} 失败：{error}", path.display()))
}

fn object_from_value(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn set_owner_only_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, permissions)
            .map_err(|error| format!("设置认证文件权限 {} 失败：{error}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn session_cwd_from_first_line(path: &Path) -> Result<Option<String>, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("读取 session 文件 {} 失败：{error}", path.display()))?;
    let Some(first_line) = content
        .lines()
        .next()
        .filter(|line| !line.trim().is_empty())
    else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(first_line)
        .map_err(|error| format!("解析 session 文件头 {} 失败：{error}", path.display()))?;
    if value.get("type").and_then(Value::as_str) != Some("session") {
        return Ok(None);
    }
    Ok(value
        .get("cwd")
        .and_then(Value::as_str)
        .map(ToString::to_string))
}

fn encoded_session_dir(cwd: &str) -> String {
    let trimmed = cwd.trim_start_matches(['/', '\\']);
    let safe = trimmed.replace(['/', '\\', ':'], "-");
    format!("--{safe}--")
}

fn tools_dir_has_custom_entries(path: &Path) -> Result<bool, String> {
    let entries = fs::read_dir(path)
        .map_err(|error| format!("读取 tools 目录 {} 失败：{error}", path.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("读取 tools 目录项 {} 失败：{error}", path.display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if !is_managed_tool_name(&name) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_managed_tool_name(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "fd" | "rg" | "fd.exe" | "rg.exe"
    )
}

pub(super) fn object_mut(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("value should be object")
}
