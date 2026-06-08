use rusqlite::Connection;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn unique_id(prefix: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_micros());
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{timestamp:x}-{counter:x}")
}

pub(crate) fn next_requirement_id(conn: &Connection) -> Result<String, String> {
    let next_id: i64 = conn
        .query_row(
            "
            SELECT COALESCE(MAX(CAST(SUBSTR(id, 5) AS INTEGER)), 100) + 1
            FROM requirements
            WHERE id LIKE 'REQ-%'
            ",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("生成需求编号失败：{error}"))?;

    Ok(format!("REQ-{next_id}"))
}

pub(crate) fn normalize_project_status(value: &str) -> &str {
    if value == "active" {
        "active"
    } else {
        "planning"
    }
}

pub(crate) fn normalize_version_status(value: &str) -> &str {
    if value == "archived" {
        "archived"
    } else {
        "active"
    }
}

pub(crate) fn normalize_priority(value: &str) -> &str {
    match value {
        "P0" | "P1" | "P2" => value,
        _ => "P2",
    }
}

pub(crate) fn normalize_requirement_type(value: &str) -> &str {
    match value {
        "Epic" | "Story" | "Task" => value,
        _ => "Task",
    }
}

pub(crate) fn normalize_requirement_status(value: &str) -> &str {
    match value {
        "todo" | "doing" | "review" | "done" => value,
        _ => "todo",
    }
}

pub(crate) fn parse_acceptance_criteria(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
}

pub(crate) fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
    message: &str,
) -> Result<Vec<T>, String> {
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|error| format!("{message}：{error}"))?);
    }
    Ok(result)
}
