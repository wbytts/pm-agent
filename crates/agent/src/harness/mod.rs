pub mod compaction;
pub mod env;
pub mod messages;
pub mod prompt_templates;
pub mod session;
pub mod shell_output;
pub mod skills;
pub mod truncate;
pub mod types;

pub use compaction::{
    calculate_context_tokens, collect_entries_for_branch_summary, compute_file_lists,
    estimate_context_tokens, estimate_tokens, finalize_branch_summary, find_cut_point,
    find_turn_start_index, format_file_operations, generate_branch_summary, prepare_branch_entries,
    prepare_compaction, serialize_conversation, should_compact, BranchPreparation,
    BranchSummaryEntries, BranchSummaryError, BranchSummaryErrorCode, BranchSummaryResult,
    CompactionPreparation, CompactionSettings, ContextUsageEstimate, CutPointResult,
    FileOperations, BRANCH_SUMMARY_PREAMBLE, BRANCH_SUMMARY_PROMPT, DEFAULT_COMPACTION_SETTINGS,
    SUMMARIZATION_SYSTEM_PROMPT,
};
#[cfg(unix)]
pub use env::file_info_from_unix_mode;
pub use env::{
    file_error_code_from_io_kind, file_error_from_io, file_info_from_metadata, resolve_path,
    resolve_shell_config, AbortFlag, ExecOptions, ExecOutput, ExecutionError, ExecutionErrorCode,
    FileError, FileErrorCode, FileInfo, FileKind, LocalExecutionEnv, RemoveOptions, ShellConfig,
    TempFileOptions,
};
pub use messages::{
    bash_execution_to_text, BashExecutionMessage, BranchSummaryMessage, CompactionSummaryMessage,
    CustomMessage, BRANCH_SUMMARY_PREFIX, BRANCH_SUMMARY_SUFFIX, COMPACTION_SUMMARY_PREFIX,
    COMPACTION_SUMMARY_SUFFIX,
};
pub use prompt_templates::{
    format_prompt_template_invocation, load_prompt_templates, parse_command_args, substitute_args,
    PromptTemplateDiagnostic, PromptTemplateDiagnosticCode,
};
pub use session::{
    build_session_context, InMemorySessionStorage, JsonlSessionStorage, Session, SessionContext,
    SessionMetadata, SessionStorage, SessionTreeEntry,
};
pub use shell_output::{
    capture_shell_output, capture_shell_output_with_options, sanitize_binary_output,
    ShellCaptureResult,
};
pub use skills::format_skills_for_system_prompt;
pub use truncate::{
    format_size, truncate_head, truncate_line, truncate_tail, TruncatedBy, TruncationOptions,
    TruncationResult, DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, GREP_MAX_LINE_LENGTH,
};
pub use types::{PromptTemplate, SessionError, SessionErrorCode, Skill};
