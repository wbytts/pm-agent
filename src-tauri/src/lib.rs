mod database;

use ai::{ApiProviderInfo, Model};
use coding_agent::{CodingToolRequest, CodingToolResult};
use pm_agent::{PmAgentResponse, PmAgentSession};
use std::time::{SystemTime, UNIX_EPOCH};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! PM Agent is ready.")
}

#[tauri::command]
fn pm_agent_create_session() -> PmAgentSession {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    pm_agent::create_session_with_workspace(format!("pm-agent-{timestamp}"), "PM Agent", cwd)
}

#[tauri::command]
fn pm_agent_send_prompt(
    session: PmAgentSession,
    prompt: String,
) -> Result<PmAgentResponse, String> {
    pm_agent::send_prompt(session, prompt)
}

#[tauri::command]
fn pm_agent_list_models() -> Vec<Model> {
    pm_agent::available_models()
}

#[tauri::command]
fn pm_agent_list_providers() -> Vec<ApiProviderInfo> {
    pm_agent::available_providers()
}

#[tauri::command]
fn pm_agent_set_session_model(session: PmAgentSession, model: Model) -> PmAgentSession {
    pm_agent::set_session_model(session, model)
}

#[tauri::command]
fn pm_agent_execute_tool(
    cwd: String,
    request: CodingToolRequest,
) -> Result<CodingToolResult, String> {
    pm_agent::execute_coding_tool(cwd, request)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            database::sqlite_database_path,
            database::sqlite_execute,
            database::sqlite_query,
            database::project_initialize_database,
            database::project_list_projects,
            database::project_list_versions,
            database::project_list_requirements,
            database::project_create_project,
            database::project_create_version,
            database::project_create_requirement,
            pm_agent_create_session,
            pm_agent_send_prompt,
            pm_agent_list_models,
            pm_agent_list_providers,
            pm_agent_set_session_model,
            pm_agent_execute_tool
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
