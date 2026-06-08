pub mod ansi;
pub mod base64;
pub mod changelog;
pub mod child_process;
pub mod clipboard;
pub mod clipboard_image;
pub mod frontmatter;
pub mod fs_watch;
pub mod git;
pub mod html;
pub mod image_dimensions;
pub mod mime;
pub mod paths;
pub mod pi_user_agent;
pub mod shell;
pub mod sleep;
pub mod version_check;
pub mod windows_self_update;

pub use ansi::strip_ansi;
pub use changelog::{compare_versions, get_new_entries, parse_changelog, ChangelogEntry};
pub use child_process::{
    format_spawn_failure, run_extraction_command, run_sync_command, SpawnOutput,
};
pub use clipboard::{
    clipboard_command_plan, copy_to_clipboard, copy_to_clipboard_with_runner,
    is_remote_session_env, osc52_sequence, ClipboardCommand, ClipboardCommandMode,
    ClipboardContext, ClipboardEnvironment, ClipboardError, ClipboardPlatform, ClipboardRunner,
    SystemClipboardRunner, MAX_OSC52_ENCODED_LENGTH,
};
pub use clipboard_image::{
    clipboard_image_read_plan, extension_for_image_mime_type, is_wayland_session,
    select_preferred_image_mime_type, ClipboardImageReadBackend,
};
pub use frontmatter::{parse_frontmatter, strip_frontmatter, ParsedFrontmatter};
pub use fs_watch::{close_watcher, watch_with_error_handler, FsWatcher, FS_WATCH_RETRY_DELAY_MS};
pub use git::{parse_git_url, GitSource};
pub use html::{decode_html_entity, decode_html_entity_at, DecodedHtmlEntity};
pub use image_dimensions::{
    detect_image_dimensions, format_image_dimensions_note, get_gif_dimensions, get_jpeg_dimensions,
    get_png_dimensions, get_webp_dimensions, ImageDimensions,
};
pub use mime::{detect_supported_image_mime_type, detect_supported_image_mime_type_from_file};
pub use paths::{
    canonicalize_path, cloud_sync_ignore_commands, cloud_sync_ignore_commands_for_platform,
    format_path_relative_to_cwd_or_absolute, get_cwd_relative_path, is_local_path,
    mark_path_ignored_by_cloud_sync, normalize_path, resolve_path, CloudSyncIgnoreCommand,
    PathInputOptions,
};
pub use pi_user_agent::get_pi_user_agent;
pub use shell::{get_shell_config, sanitize_binary_output, ShellConfig};
pub use sleep::sleep;
pub use version_check::{
    check_for_new_pi_version, compare_package_versions, get_latest_pi_release,
    get_latest_pi_version, is_newer_package_version, LatestPiRelease, VersionCheckOptions,
};
pub use windows_self_update::{
    cleanup_windows_self_update_quarantine, loaded_files_in_package_dir,
    quarantine_windows_native_dependencies, windows_self_update_quarantine_root,
};
