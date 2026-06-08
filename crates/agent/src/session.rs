use ai::{LanguageModelProvider, Model};

use crate::error::AgentResult;
use crate::runtime::Agent;
use crate::state::{AgentEvent, AgentState};

pub struct AgentSession<P: LanguageModelProvider> {
    agent: Agent<P>,
}

impl<P: LanguageModelProvider> AgentSession<P> {
    pub fn new(
        session_id: impl Into<String>,
        system_prompt: impl Into<String>,
        model: Model,
        provider: P,
    ) -> Self {
        Self::from_state(
            AgentState {
                session_id: session_id.into(),
                system_prompt: system_prompt.into(),
                model,
                messages: Vec::new(),
                is_streaming: false,
            },
            provider,
        )
    }

    pub fn from_state(state: AgentState, provider: P) -> Self {
        Self {
            agent: Agent::new(state, provider),
        }
    }

    pub fn send_user_message(&mut self, prompt: impl Into<String>) -> AgentResult<Vec<AgentEvent>> {
        self.agent.prompt(prompt)
    }

    pub fn state(&self) -> &AgentState {
        self.agent.state()
    }

    pub fn into_state(self) -> AgentState {
        self.agent.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::{MessageRole, RegisteredProvider};

    #[test]
    fn session_keeps_transcript_across_turns() {
        let model = Model {
            id: "echo".to_string(),
            provider: "local".to_string(),
            api: "local-echo".to_string(),
            display_name: "Local Echo".to_string(),
            context_window: 32_000,
            ..Model::default()
        };
        let mut session = AgentSession::new(
            "session-1",
            "system",
            model,
            RegisteredProvider::Echo(ai::EchoProvider),
        );

        session
            .send_user_message("hello")
            .expect("first turn should work");
        session
            .send_user_message("again")
            .expect("second turn should work");

        assert_eq!(session.state().messages.len(), 4);
        assert_eq!(session.state().messages[2].role, MessageRole::User);
        assert_eq!(session.state().messages[2].content, "again");
    }
}
