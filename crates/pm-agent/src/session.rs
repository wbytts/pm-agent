use agent::AgentMessage;
use ai::Model;
use coding_agent::{
    default_tools, settings_manager::run_startup_migrations, utils::agent_dir,
    utils::AppConfigPaths, CodingTool,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::model_catalog::default_model;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PmAgentSession {
    pub id: String,
    pub title: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<CodingTool>,
    pub workspace_cwd: Option<String>,
    pub model: Model,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PmAgentRequest {
    pub session_id: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PmAgentResponse {
    pub events: Vec<agent::AgentEvent>,
    pub session: PmAgentSession,
}

pub fn create_session(id: impl Into<String>, title: impl Into<String>) -> PmAgentSession {
    PmAgentSession {
        id: id.into(),
        title: title.into(),
        messages: Vec::new(),
        tools: default_tools(),
        workspace_cwd: None,
        model: default_model(),
    }
}

pub fn create_session_with_workspace(
    id: impl Into<String>,
    title: impl Into<String>,
    cwd: impl Into<String>,
) -> PmAgentSession {
    create_session_with_workspace_unchecked(id, title, cwd)
}

pub fn try_create_session_with_workspace(
    id: impl Into<String>,
    title: impl Into<String>,
    cwd: impl Into<String>,
) -> Result<PmAgentSession, String> {
    let cwd = cwd.into();
    let home_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "无法定位 HOME 目录，不能运行启动迁移".to_string())?;
    let config = AppConfigPaths::new(home_dir);
    let agent_dir = agent_dir(&config);
    create_session_with_workspace_and_migrations(id, title, cwd, agent_dir)
}

fn create_session_with_workspace_and_migrations(
    id: impl Into<String>,
    title: impl Into<String>,
    cwd: impl Into<String>,
    agent_dir: impl AsRef<Path>,
) -> Result<PmAgentSession, String> {
    let cwd = cwd.into();
    run_startup_migrations(agent_dir, &cwd)?;
    Ok(create_session_with_workspace_unchecked(id, title, cwd))
}

fn create_session_with_workspace_unchecked(
    id: impl Into<String>,
    title: impl Into<String>,
    cwd: impl Into<String>,
) -> PmAgentSession {
    PmAgentSession {
        id: id.into(),
        title: title.into(),
        messages: Vec::new(),
        tools: default_tools(),
        workspace_cwd: Some(cwd.into()),
        model: default_model(),
    }
}

pub fn set_session_model(mut session: PmAgentSession, model: Model) -> PmAgentSession {
    session.model = model;
    session
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const CONFIG_DIR_NAME: &str = ".pm-agent";

    fn temp_workspace(name: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间应晚于 UNIX_EPOCH")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("pm-agent-{name}-{timestamp}"));
        fs::create_dir_all(&path).expect("应能创建临时目录");
        path
    }

    #[test]
    fn create_session_with_workspace_and_migrations_runs_startup_migrations() {
        let workspace = temp_workspace("startup-migrations");
        let project_config = workspace.join(CONFIG_DIR_NAME);
        let commands = project_config.join("commands");
        fs::create_dir_all(&commands).expect("应能创建 legacy commands 目录");
        fs::write(commands.join("draft.md"), "content").expect("应能写入测试 prompt");

        let agent_dir = workspace.join("agent");
        let session = create_session_with_workspace_and_migrations(
            "session",
            "Session",
            workspace.to_string_lossy(),
            &agent_dir,
        )
        .expect("启动迁移应成功");

        assert_eq!(
            session.workspace_cwd.as_deref(),
            Some(workspace.to_string_lossy().as_ref())
        );
        assert!(!commands.exists());
        assert!(project_config.join("prompts").join("draft.md").exists());

        fs::remove_dir_all(workspace).expect("应能清理临时目录");
    }
}
