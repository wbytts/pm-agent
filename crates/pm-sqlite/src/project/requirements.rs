use rusqlite::params;

use crate::connection::open_connection;
use crate::project::helpers::{
    collect_rows, next_requirement_id, normalize_priority, normalize_requirement_status,
    normalize_requirement_type, parse_acceptance_criteria,
};
use crate::types::{Requirement, RequirementDraft};

pub fn list_requirements() -> Result<Vec<Requirement>, String> {
    let conn = open_connection()?;
    let mut statement = conn
        .prepare(
            "
            SELECT
              r.id,
              r.project_id,
              p.name AS project,
              r.version_id,
              v.name AS version_name,
              r.title,
              r.priority,
              r.type,
              r.status,
              r.assignee,
              r.due_date,
              r.description,
              r.acceptance_criteria
            FROM requirements r
            INNER JOIN projects p ON p.id = r.project_id
            LEFT JOIN project_versions v ON v.id = r.version_id
            ORDER BY r.created_at DESC, r.id DESC
            ",
        )
        .map_err(|error| format!("准备需求列表查询失败：{error}"))?;

    let rows = statement
        .query_map([], |row| {
            Ok(Requirement {
                id: row.get(0)?,
                project_id: row.get(1)?,
                project: row.get(2)?,
                version_id: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                version_name: row
                    .get::<_, Option<String>>(4)?
                    .unwrap_or_else(|| "默认版本".to_string()),
                title: row.get(5)?,
                priority: normalize_priority(row.get::<_, String>(6)?.as_str()).to_string(),
                r#type: normalize_requirement_type(row.get::<_, String>(7)?.as_str()).to_string(),
                status: normalize_requirement_status(row.get::<_, String>(8)?.as_str()).to_string(),
                assignee: row.get(9)?,
                due_date: row.get(10)?,
                description: row.get(11)?,
                acceptance_criteria: parse_acceptance_criteria(row.get::<_, String>(12)?.as_str()),
            })
        })
        .map_err(|error| format!("查询需求列表失败：{error}"))?;

    collect_rows(rows, "读取需求列表失败")
}

pub fn create_requirement(draft: RequirementDraft) -> Result<String, String> {
    let conn = open_connection()?;
    let id = next_requirement_id(&conn)?;
    let acceptance_criteria = draft
        .description
        .lines()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .take(5)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let acceptance_criteria = serde_json::to_string(&acceptance_criteria)
        .map_err(|error| format!("序列化验收标准失败：{error}"))?;

    conn.execute(
        "
        INSERT INTO requirements (
          id, project_id, version_id, title, priority, type, status, assignee, due_date, description, acceptance_criteria
        )
        VALUES (?, ?, ?, ?, ?, ?, 'todo', ?, ?, ?, ?)
        ",
        params![
            id,
            draft.project_id,
            draft.version_id,
            draft.title,
            normalize_priority(draft.priority.as_str()),
            normalize_requirement_type(draft.r#type.as_str()),
            draft.assignee,
            draft.due_date,
            draft.description,
            acceptance_criteria
        ],
    )
    .map_err(|error| format!("创建需求失败：{error}"))?;

    Ok(id)
}
