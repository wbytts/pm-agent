use pm_sqlite::{
    Project, ProjectDraft, ProjectVersion, ProjectVersionDraft, Requirement, RequirementDraft,
    SqliteExecuteResult, SqliteRow,
};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn sqlite_database_path() -> Result<String, String> {
    pm_sqlite::sqlite_database_path()
}

#[tauri::command]
pub fn sqlite_execute(
    sql: String,
    params: Option<Vec<JsonValue>>,
) -> Result<SqliteExecuteResult, String> {
    pm_sqlite::sqlite_execute(sql, params)
}

#[tauri::command]
pub fn sqlite_query(sql: String, params: Option<Vec<JsonValue>>) -> Result<Vec<SqliteRow>, String> {
    pm_sqlite::sqlite_query(sql, params)
}

#[tauri::command]
pub fn project_initialize_database() -> Result<(), String> {
    pm_sqlite::initialize_project_database()
}

#[tauri::command]
pub fn project_list_projects() -> Result<Vec<Project>, String> {
    pm_sqlite::list_projects()
}

#[tauri::command]
pub fn project_list_versions() -> Result<Vec<ProjectVersion>, String> {
    pm_sqlite::list_project_versions()
}

#[tauri::command]
pub fn project_list_requirements() -> Result<Vec<Requirement>, String> {
    pm_sqlite::list_requirements()
}

#[tauri::command]
pub fn project_create_project(draft: ProjectDraft) -> Result<String, String> {
    pm_sqlite::create_project(draft)
}

#[tauri::command]
pub fn project_create_version(draft: ProjectVersionDraft) -> Result<String, String> {
    pm_sqlite::create_project_version(draft)
}

#[tauri::command]
pub fn project_create_requirement(draft: RequirementDraft) -> Result<String, String> {
    pm_sqlite::create_requirement(draft)
}
