use rusqlite::params_from_iter;
use serde_json::Value as JsonValue;

use crate::connection::{
    column_names, json_params_to_sqlite, open_connection, sqlite_value_to_json,
};
use crate::types::{SqliteExecuteResult, SqliteRow};

pub fn sqlite_execute(
    sql: String,
    params: Option<Vec<JsonValue>>,
) -> Result<SqliteExecuteResult, String> {
    let params = params.unwrap_or_default();
    eprintln!("[sqlite_execute] sql={}, params_len={}", sql, params.len());

    let conn = open_connection().map_err(|error| {
        eprintln!("[sqlite_execute] open_connection error={error}");
        error
    })?;
    let sqlite_params = json_params_to_sqlite(params).map_err(|error| {
        eprintln!("[sqlite_execute] params error={error}");
        error
    })?;

    let rows_affected = conn
        .execute(&sql, params_from_iter(sqlite_params.iter()))
        .map_err(|error| {
            let error = format!("执行 SQL 失败：{error}");
            eprintln!("[sqlite_execute] execute error={error}");
            error
        })?;

    Ok(SqliteExecuteResult {
        rows_affected,
        last_insert_rowid: conn.last_insert_rowid(),
    })
}

pub fn sqlite_query(sql: String, params: Option<Vec<JsonValue>>) -> Result<Vec<SqliteRow>, String> {
    let params = params.unwrap_or_default();
    eprintln!("[sqlite_query] sql={}, params_len={}", sql, params.len());

    let conn = open_connection().map_err(|error| {
        eprintln!("[sqlite_query] open_connection error={error}");
        error
    })?;
    let sqlite_params = json_params_to_sqlite(params).map_err(|error| {
        eprintln!("[sqlite_query] params error={error}");
        error
    })?;

    let mut statement = conn.prepare(&sql).map_err(|error| {
        let error = format!("准备查询失败：{error}");
        eprintln!("[sqlite_query] prepare error={error}");
        error
    })?;
    let column_names = column_names(&statement);
    let mut rows = statement
        .query(params_from_iter(sqlite_params.iter()))
        .map_err(|error| {
            let error = format!("执行查询失败：{error}");
            eprintln!("[sqlite_query] query error={error}");
            error
        })?;

    let mut result = Vec::new();
    while let Some(row) = rows.next().map_err(|error| {
        let error = format!("读取查询结果失败：{error}");
        eprintln!("[sqlite_query] row error={error}");
        error
    })? {
        let mut item = SqliteRow::new();
        for (index, name) in column_names.iter().enumerate() {
            let value = row.get_ref(index).map_err(|error| {
                let error = format!("读取列 {name} 失败：{error}");
                eprintln!("[sqlite_query] column error={error}");
                error
            })?;
            item.insert(name.clone(), sqlite_value_to_json(value));
        }
        result.push(item);
    }

    Ok(result)
}
