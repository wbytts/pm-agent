use rusqlite::params;

use crate::connection::open_connection;
use crate::project::helpers::{collect_rows, normalize_project_status, unique_id};
use crate::project::versions::create_project_version_with_connection;
use crate::types::{Project, ProjectDraft, ProjectVersionDraft};

pub fn list_projects() -> Result<Vec<Project>, String> {
    let conn = open_connection()?;
    let mut statement = conn
        .prepare(
            "
            SELECT
              p.id,
              p.name,
              p.description,
              p.status,
              p.owner,
              p.due_date,
              p.members,
              COUNT(r.id) AS total_requirements,
              SUM(CASE WHEN r.status = 'done' THEN 1 ELSE 0 END) AS completed_requirements
            FROM projects p
            LEFT JOIN requirements r ON r.project_id = p.id
            GROUP BY p.id
            ORDER BY p.created_at DESC, p.name ASC
            ",
        )
        .map_err(|error| format!("准备项目列表查询失败：{error}"))?;

    let rows = statement
        .query_map([], |row| {
            let total_requirements: i64 = row.get(7)?;
            let completed_requirements: i64 = row.get::<_, Option<i64>>(8)?.unwrap_or(0);
            let progress = if total_requirements == 0 {
                0
            } else {
                ((completed_requirements as f64 / total_requirements as f64) * 100.0).round() as i64
            };

            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                status: normalize_project_status(row.get::<_, String>(3)?.as_str()).to_string(),
                owner: row.get(4)?,
                due_date: row.get(5)?,
                members: row.get(6)?,
                total_requirements,
                completed_requirements,
                progress,
            })
        })
        .map_err(|error| format!("查询项目列表失败：{error}"))?;

    collect_rows(rows, "读取项目列表失败")
}

pub fn create_project(draft: ProjectDraft) -> Result<String, String> {
    let conn = open_connection()?;
    let id = unique_id("project");
    conn.execute(
        "
        INSERT INTO projects (id, name, description, status, owner, due_date, members)
        VALUES (?, ?, ?, 'planning', ?, ?, ?)
        ",
        params![
            id,
            draft.name,
            draft.description,
            draft.owner,
            draft.due_date,
            draft.members
        ],
    )
    .map_err(|error| format!("创建项目失败：{error}"))?;

    create_project_version_with_connection(
        &conn,
        ProjectVersionDraft {
            project_id: id.clone(),
            name: "默认版本".to_string(),
            description: String::new(),
        },
    )?;

    Ok(id)
}
