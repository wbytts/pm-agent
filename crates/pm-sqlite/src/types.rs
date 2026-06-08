use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct SqliteExecuteResult {
    pub rows_affected: usize,
    pub last_insert_rowid: i64,
}

pub type SqliteRow = BTreeMap<String, JsonValue>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub owner: String,
    pub due_date: String,
    pub members: i64,
    pub total_requirements: i64,
    pub completed_requirements: i64,
    pub progress: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectVersion {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub requirement_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirement {
    pub id: String,
    pub title: String,
    pub project_id: String,
    pub project: String,
    pub version_id: String,
    pub version_name: String,
    pub priority: String,
    pub r#type: String,
    pub status: String,
    pub assignee: String,
    pub due_date: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDraft {
    pub name: String,
    pub description: String,
    pub due_date: String,
    pub owner: String,
    pub members: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectVersionDraft {
    pub project_id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequirementDraft {
    pub title: String,
    pub project_id: String,
    pub version_id: String,
    pub priority: String,
    pub r#type: String,
    pub assignee: String,
    pub due_date: String,
    pub description: String,
}
