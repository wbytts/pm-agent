use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("用户输入不能为空")]
    EmptyPrompt,
    #[error("AI 调用失败：{0}")]
    Ai(String),
    #[error("Cannot continue: no messages in context")]
    EmptyContinueContext,
    #[error("Cannot continue from message role: assistant")]
    ContinueFromAssistant,
}

pub type AgentResult<T> = Result<T, AgentError>;
