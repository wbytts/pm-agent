mod agent_events;
mod commands;
mod context;
mod flags;
mod input;
pub mod loader;
mod messages;
mod provider_events;
mod providers;
mod resources;
pub mod runner;
mod tool_calls;
mod tool_results;
mod tools;
pub mod types;
mod user_bash;
pub mod wrapper;

pub use agent_events::{emit_before_agent_start, BeforeAgentStartEvent, BeforeAgentStartResult};
pub use commands::{resolve_registered_commands, ResolvedCommand};
pub use context::{emit_context, ContextEventResult};
pub use flags::{
    apply_extension_flag_values, resolve_flags, AppliedExtensionFlagValues, ExtensionFlagDiagnostic,
};
pub use input::{emit_input, InputEvent, InputEventResult, InputSource};
pub use loader::{
    create_extension_runtime, discover_and_load_extensions, load_extension_from_factory,
    load_extensions,
};
pub use messages::emit_message_end;
pub use provider_events::emit_before_provider_request;
pub use providers::to_model_provider_config;
pub use resources::{
    emit_resources_discover, DiscoveredExtensionResources, ExtensionResourcePath,
    ResourcesDiscoverReason,
};
pub use runner::{emit_session_shutdown_event, ExtensionRunner};
pub use tool_calls::{emit_tool_call, ExtensionToolCallEvent, ToolCallDecision};
pub use tool_results::{emit_tool_result, ExtensionToolResultEvent, ToolResultUpdate};
pub use tools::{find_tool_definition, resolve_registered_tools};
pub use types::{
    Extension, ExtensionApi, ExtensionCommandContext, ExtensionContext, ExtensionError,
    ExtensionEvent, ExtensionFactory, ExtensionFlag, ExtensionFlagType, ExtensionRuntime,
    LoadExtensionsResult, ProviderConfig, RegisteredCommand, RegisteredTool, ToolDefinition,
};
pub use user_bash::{emit_user_bash, UserBashEvent, UserBashResult};
pub use wrapper::{wrap_registered_tool, wrap_registered_tools};
