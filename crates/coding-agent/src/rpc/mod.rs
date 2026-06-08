mod client;
mod command_registry;
mod dispatcher;
mod jsonl;
mod mode;
mod prompt_input;
mod resources;
mod session_backend;
mod types;

pub use client::{RpcClient, RpcClientError, RpcTransport};
pub use dispatcher::{RpcDispatcher, RpcSessionBackend};
pub use jsonl::{serialize_json_line, JsonlLineReader};
pub use mode::{RpcMode, RpcModeOutput};
pub use resources::RpcResourceSnapshot;
pub use session_backend::ManagedRpcSessionBackend;
pub use types::{
    RpcCommand, RpcCommandType, RpcEvent, RpcExtensionUiRequest, RpcExtensionUiResponse, RpcOutput,
    RpcResponse, RpcSessionState, RpcSlashCommand, RpcSlashCommandSource,
};
