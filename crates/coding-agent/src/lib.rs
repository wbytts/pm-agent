pub mod armin;
pub mod assistant_message;
pub mod auth_guidance;
pub mod auth_storage;
pub mod bash_execution;
pub mod bash_executor;
pub mod cli;
pub mod compaction;
pub mod config_selector;
pub mod custom_editor;
pub mod daxnuts;
pub mod defaults;
pub mod diagnostics;
pub mod diff_view;
pub mod earendil_announcement;
pub mod event_bus;
pub mod exec;
pub mod export_html;
pub mod extension_editor;
pub mod extension_input;
pub mod extension_selector;
pub mod extensions;
pub mod footer;
pub mod footer_data_provider;
pub mod http_dispatcher;
pub mod keybinding_hints;
pub mod keybindings;
pub mod login_dialog;
pub mod messages;
pub mod model_registry;
pub mod model_resolver;
pub mod model_selector;
pub mod oauth_selector;
pub mod output_guard;
pub mod package_manager;
pub mod print_mode;
pub mod prompt_templates;
pub mod provider_display_names;
pub mod resolve_config_value;
pub mod resource_loader;
pub mod rpc;
pub mod scoped_models_selector;
pub mod session_cwd;
pub mod session_manager;
pub mod session_selector;
pub mod session_selector_search;
pub mod settings_manager;
pub mod settings_selector;
pub mod show_images_selector;
pub mod skill_commands;
pub mod slash_commands;
pub mod source_info;
pub mod summary_message;
pub mod system_prompt;
pub mod telemetry;
pub mod theme_selector;
pub mod thinking_selector;
pub mod timings;
pub mod tool_execution;
pub mod tree_selector;
pub mod user_message;
pub mod user_message_selector;
pub mod utils;

mod tools;
mod types;
mod workspace;

pub use tools::{
    default_tools, execute_tool, plan_tool_activation, NoToolsMode, ToolActivationPlan,
};
pub use types::{
    CodingAgentError, CodingAgentResult, CodingContentBlock, CodingTool, CodingToolEdit,
    CodingToolKind, CodingToolRequest, CodingToolResult, CodingWorkspace,
};
pub use workspace::validate_workspace;
