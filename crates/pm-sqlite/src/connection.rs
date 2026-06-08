use rusqlite::types::{Value, ValueRef};
use rusqlite::Connection;
use serde_json::{Number, Value as JsonValue};
use std::fs;
use std::path::{Path, PathBuf};

pub fn sqlite_database_path() -> Result<String, String> {
    let path = database_path()?;
    Ok(path.to_string_lossy().to_string())
}

pub(crate) fn open_connection() -> Result<Connection, String> {
    let path = database_path()?;
    ensure_parent_dir(&path)?;

    eprintln!("[sqlite] open path={}", path.to_string_lossy());
    let conn = Connection::open(&path).map_err(|error| format!("打开数据库失败：{error}"))?;

    // 每次打开连接都启用关键运行参数，保证外键约束和写入并发策略稳定。
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;
        PRAGMA busy_timeout = 5000;
        ",
    )
    .map_err(|error| format!("初始化数据库连接失败：{error}"))?;

    Ok(conn)
}

fn database_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "读取 HOME 环境变量失败".to_string())?;
    Ok(database_path_from_home(Path::new(&home)))
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Err("数据库路径缺少父目录".to_string());
    };

    fs::create_dir_all(parent).map_err(|error| format!("创建数据库目录失败：{error}"))
}

pub(crate) fn json_params_to_sqlite(params: Vec<JsonValue>) -> Result<Vec<Value>, String> {
    params.into_iter().map(json_value_to_sqlite).collect()
}

fn json_value_to_sqlite(value: JsonValue) -> Result<Value, String> {
    match value {
        JsonValue::Null => Ok(Value::Null),
        JsonValue::Bool(value) => Ok(Value::Integer(i64::from(value))),
        JsonValue::Number(value) => json_number_to_sqlite(value),
        JsonValue::String(value) => Ok(Value::Text(value)),
        JsonValue::Array(value) => {
            let mut bytes = Vec::with_capacity(value.len());
            for item in value {
                let Some(byte) = item.as_u64().and_then(|value| u8::try_from(value).ok()) else {
                    return Err("Blob 参数必须是 0-255 的数字数组".to_string());
                };
                bytes.push(byte);
            }
            Ok(Value::Blob(bytes))
        }
        JsonValue::Object(_) => Err("SQLite 参数不支持对象类型".to_string()),
    }
}

fn json_number_to_sqlite(value: Number) -> Result<Value, String> {
    if let Some(value) = value.as_i64() {
        return Ok(Value::Integer(value));
    }
    if let Some(value) = value.as_u64() {
        let value = i64::try_from(value).map_err(|_| "数字超出 SQLite INTEGER 范围".to_string())?;
        return Ok(Value::Integer(value));
    }
    if let Some(value) = value.as_f64() {
        return Ok(Value::Real(value));
    }

    Err("不支持的数字参数".to_string())
}

pub(crate) fn column_names(statement: &rusqlite::Statement<'_>) -> Vec<String> {
    (0..statement.column_count())
        .map(|index| {
            statement
                .column_name(index)
                .map_or_else(|_| format!("column_{index}"), ToString::to_string)
        })
        .collect()
}

pub(crate) fn sqlite_value_to_json(value: ValueRef<'_>) -> JsonValue {
    match value {
        ValueRef::Null => JsonValue::Null,
        ValueRef::Integer(value) => JsonValue::Number(value.into()),
        ValueRef::Real(value) => Number::from_f64(value).map_or(JsonValue::Null, JsonValue::Number),
        ValueRef::Text(value) => JsonValue::String(String::from_utf8_lossy(value).to_string()),
        ValueRef::Blob(value) => JsonValue::Array(
            value
                .iter()
                .map(|byte| JsonValue::Number(Number::from(*byte)))
                .collect(),
        ),
    }
}

#[cfg(test)]
pub(crate) fn database_path_from_home(home: &Path) -> PathBuf {
    home.join(".pm-agent").join("data.sqlite3")
}

#[cfg(not(test))]
fn database_path_from_home(home: &Path) -> PathBuf {
    home.join(".pm-agent").join("data.sqlite3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_path_uses_pm_agent_directory() {
        let path = database_path_from_home(Path::new("/tmp/home"));
        assert_eq!(path, PathBuf::from("/tmp/home/.pm-agent/data.sqlite3"));
    }

    #[test]
    fn json_params_support_basic_sqlite_types() {
        let params = json_params_to_sqlite(vec![
            JsonValue::Null,
            JsonValue::Bool(true),
            JsonValue::Number(12.into()),
            JsonValue::String("title".to_string()),
            JsonValue::Array(vec![
                JsonValue::Number(1.into()),
                JsonValue::Number(255.into()),
            ]),
        ])
        .expect("params should convert");

        assert!(matches!(params[0], Value::Null));
        assert!(matches!(params[1], Value::Integer(1)));
        assert!(matches!(params[2], Value::Integer(12)));
        assert!(matches!(params[3], Value::Text(_)));
        assert!(matches!(params[4], Value::Blob(_)));
    }
}
