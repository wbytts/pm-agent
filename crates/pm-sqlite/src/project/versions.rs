use rusqlite::{params, Connection};

use crate::connection::open_connection;
use crate::project::helpers::{collect_rows, normalize_version_status, unique_id};
use crate::types::{ProjectVersion, ProjectVersionDraft};

pub fn list_project_versions() -> Result<Vec<ProjectVersion>, String> {
    let conn = open_connection()?;
    let mut statement = conn
        .prepare(
            "
            SELECT
              v.id,
              v.project_id,
              v.name,
              v.description,
              v.status,
              COUNT(r.id) AS requirement_count
            FROM project_versions v
            LEFT JOIN requirements r ON r.version_id = v.id
            GROUP BY v.id
            ORDER BY v.project_id ASC, v.sort_order ASC, v.created_at ASC
            ",
        )
        .map_err(|error| format!("准备版本列表查询失败：{error}"))?;

    let rows = statement
        .query_map([], |row| {
            Ok(ProjectVersion {
                id: row.get(0)?,
                project_id: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                status: normalize_version_status(row.get::<_, String>(4)?.as_str()).to_string(),
                requirement_count: row.get(5)?,
            })
        })
        .map_err(|error| format!("查询版本列表失败：{error}"))?;

    collect_rows(rows, "读取版本列表失败")
}

pub fn create_project_version(draft: ProjectVersionDraft) -> Result<String, String> {
    let conn = open_connection()?;
    create_project_version_with_connection(&conn, draft)
}

pub(crate) fn create_project_version_with_connection(
    conn: &Connection,
    draft: ProjectVersionDraft,
) -> Result<String, String> {
    let id = unique_id("version");
    let sort_order: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM project_versions WHERE project_id = ?",
            params![draft.project_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("读取版本排序失败：{error}"))?;

    conn.execute(
        "
        INSERT INTO project_versions (id, project_id, name, description, status, sort_order)
        VALUES (?, ?, ?, ?, 'active', ?)
        ",
        params![
            id,
            draft.project_id,
            draft.name,
            draft.description,
            sort_order
        ],
    )
    .map_err(|error| format!("创建项目版本失败：{error}"))?;

    Ok(id)
}
