use rusqlite::Connection;

use crate::connection::open_connection;

pub fn initialize_project_database() -> Result<(), String> {
    let conn = open_connection()?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS projects (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          description TEXT NOT NULL DEFAULT '',
          status TEXT NOT NULL DEFAULT 'planning',
          owner TEXT NOT NULL DEFAULT '',
          due_date TEXT NOT NULL DEFAULT '',
          members INTEGER NOT NULL DEFAULT 1,
          created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS project_versions (
          id TEXT PRIMARY KEY,
          project_id TEXT NOT NULL,
          name TEXT NOT NULL,
          description TEXT NOT NULL DEFAULT '',
          status TEXT NOT NULL DEFAULT 'active',
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
          FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS requirements (
          id TEXT PRIMARY KEY,
          project_id TEXT NOT NULL,
          version_id TEXT,
          title TEXT NOT NULL,
          priority TEXT NOT NULL DEFAULT 'P2',
          type TEXT NOT NULL DEFAULT 'Task',
          status TEXT NOT NULL DEFAULT 'todo',
          assignee TEXT NOT NULL DEFAULT '',
          due_date TEXT NOT NULL DEFAULT '',
          description TEXT NOT NULL DEFAULT '',
          acceptance_criteria TEXT NOT NULL DEFAULT '[]',
          created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
          FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
          FOREIGN KEY (version_id) REFERENCES project_versions(id) ON DELETE SET NULL
        );
        ",
    )
    .map_err(|error| format!("初始化项目数据库失败：{error}"))?;

    ensure_requirement_version_column(&conn)?;
    ensure_default_versions(&conn)
}

fn ensure_requirement_version_column(conn: &Connection) -> Result<(), String> {
    let mut statement = conn
        .prepare("PRAGMA table_info(requirements)")
        .map_err(|error| format!("读取需求表结构失败：{error}"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("查询需求表结构失败：{error}"))?;
    let mut has_version_id = false;
    for column in columns {
        if column.map_err(|error| format!("读取需求表字段失败：{error}"))? == "version_id"
        {
            has_version_id = true;
            break;
        }
    }
    if !has_version_id {
        conn.execute("ALTER TABLE requirements ADD COLUMN version_id TEXT", [])
            .map_err(|error| format!("迁移需求版本字段失败：{error}"))?;
    }
    Ok(())
}

fn ensure_default_versions(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "
        INSERT OR IGNORE INTO project_versions (id, project_id, name, description, status, sort_order)
        SELECT id || '-default', id, '默认版本', '', 'active', 0
        FROM projects
        ",
        [],
    )
    .map_err(|error| format!("创建默认版本失败：{error}"))?;

    conn.execute(
        "
        UPDATE requirements
        SET version_id = project_id || '-default'
        WHERE version_id IS NULL OR version_id = ''
        ",
        [],
    )
    .map_err(|error| format!("迁移需求默认版本失败：{error}"))?;

    Ok(())
}
