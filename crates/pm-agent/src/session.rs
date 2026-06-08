use agent::AgentMessage;
use ai::Model;
use coding_agent::{default_tools, CodingTool};
use serde::{Deserialize, Serialize};

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
