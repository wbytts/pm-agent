use agent::{AgentEvent, AgentMessage, AgentSession, AgentState};
use ai::{MessageRole, ProviderRegistry};
use coding_agent::CodingToolRequest;

use crate::coding_tools::execute_coding_tool;
use crate::session::{PmAgentResponse, PmAgentSession};
use crate::tool_command::parse_tool_prompt;

pub fn send_prompt(
    mut session: PmAgentSession,
    prompt: impl Into<String>,
) -> Result<PmAgentResponse, String> {
    let prompt = prompt.into();
    if let Some(tool_request) = parse_tool_prompt(&prompt)? {
        return send_tool_prompt(session, prompt, tool_request);
    }

    let model = session.model.clone();
    let provider = ProviderRegistry::builtins()
        .provider_for(&model)
        .map_err(|error| error.to_string())?;

    let state = AgentState {
        session_id: session.id.clone(),
        system_prompt: "你是 PM Agent 的项目管理辅助执行 agent。".to_string(),
        model,
        messages: session.messages.clone(),
        is_streaming: false,
    };
    let mut agent_session = AgentSession::from_state(state, provider);
    let events = agent_session
        .send_user_message(prompt)
        .map_err(|error| error.to_string())?;
    session.messages = agent_session.state().messages.clone();

    Ok(PmAgentResponse { events, session })
}

pub fn user_message(content: impl Into<String>) -> AgentMessage {
    AgentMessage::new(MessageRole::User, content)
}

fn send_tool_prompt(
    mut session: PmAgentSession,
    prompt: String,
    request: CodingToolRequest,
) -> Result<PmAgentResponse, String> {
    let cwd = session
        .workspace_cwd
        .clone()
        .ok_or_else(|| "当前 agent 会话缺少工作区路径".to_string())?;
    let result = execute_coding_tool(cwd, request)?;
    let status = if result.success { "成功" } else { "失败" };
    let assistant_content = format!("工具执行{status}\n{}", result.output);
    let user = user_message(prompt);
    let assistant = AgentMessage::new(MessageRole::Assistant, assistant_content);

    session.messages.push(user);
    session.messages.push(assistant.clone());

    let events = vec![
        AgentEvent::AgentStart,
        AgentEvent::MessageEnd { message: assistant },
        AgentEvent::AgentEnd {
            messages: session.messages.clone(),
        },
    ];

    Ok(PmAgentResponse { events, session })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{create_session, create_session_with_workspace};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_WORKSPACE_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn tool_prompt_reads_workspace_file() {
        let cwd = temp_workspace();
        fs::write(cwd.join("note.txt"), "hello agent").expect("file should be written");
        let session = create_session_with_workspace("read-basic", "test", cwd.to_string_lossy());

        let response = send_prompt(session, "/read note.txt").expect("tool prompt should run");
        let message = response.session.messages.last().expect("assistant message");
        assert_eq!(message.role, MessageRole::Assistant);
        assert!(message.content.contains("hello agent"));
    }

    #[test]
    fn tool_prompt_reads_file_with_offset_and_limit() {
        let cwd = temp_workspace();
        fs::write(cwd.join("note.txt"), "one\ntwo\nthree").expect("file should be written");
        let session = create_session_with_workspace("read-window", "test", cwd.to_string_lossy());

        let response = send_prompt(session, "/read note.txt offset=2 limit=1")
            .expect("tool prompt should run");
        let message = response.session.messages.last().expect("assistant message");

        assert!(message.content.contains("two"));
        assert!(!message.content.contains("one"));
        assert!(message.content.contains("Use offset=3 to continue"));
    }

    #[test]
    fn tool_prompt_edits_multiple_blocks() {
        let cwd = temp_workspace();
        fs::write(cwd.join("note.txt"), "alpha\nbeta\ngamma\n").expect("file should be written");
        let session = create_session_with_workspace("edit-blocks", "test", cwd.to_string_lossy());

        let response = send_prompt(
            session,
            "/edit note.txt\nalpha\n=>\nALPHA\n---\ngamma\n=>\nGAMMA",
        )
        .expect("tool prompt should run");

        let message = response.session.messages.last().expect("assistant message");
        let written = fs::read_to_string(cwd.join("note.txt")).expect("file should be readable");
        assert!(message.content.contains("2 block(s)"));
        assert_eq!(written, "ALPHA\nbeta\nGAMMA\n");
    }

    #[test]
    fn normal_prompt_uses_agent_provider() {
        let session = create_session("normal", "test");
        let response = send_prompt(session, "hello").expect("prompt should run");
        assert_eq!(response.session.messages.len(), 2);
        assert_eq!(response.session.messages[1].content, "hello");
    }

    fn temp_workspace() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let counter = TEMP_WORKSPACE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let cwd = std::env::temp_dir().join(format!("pm-agent-session-test-{id}-{counter}"));
        fs::create_dir_all(&cwd).expect("workspace should be created");
        cwd
    }
}
