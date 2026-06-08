mod args;
mod file_processor;
mod initial_message;
mod list_models;
mod session_manager;

pub use args::{
    is_valid_thinking_level, parse_args, resolve_app_mode, AppMode, CliArgs, CliDiagnostic,
    CliDiagnosticType, CliMode, UnknownFlagValue,
};
pub use file_processor::{process_file_arguments, ProcessFileOptions, ProcessedFiles};
pub use initial_message::{build_initial_message, InitialMessageInput, InitialMessageResult};
pub use list_models::{list_models_output, ListModelsOutput};
pub use session_manager::{
    create_cli_session_manager, validate_fork_flags, CliSessionError, CliSessionManager,
    GlobalSessionPolicy,
};
