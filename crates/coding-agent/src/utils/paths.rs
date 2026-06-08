use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathInputOptions {
    pub trim: bool,
    pub expand_tilde: Option<bool>,
    pub home_dir: Option<PathBuf>,
    pub strip_at_prefix: bool,
    pub normalize_unicode_spaces: bool,
}

pub fn canonicalize_path(path: impl AsRef<Path>) -> PathBuf {
    fs::canonicalize(path.as_ref()).unwrap_or_else(|_| path.as_ref().to_path_buf())
}

pub fn is_local_path(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.starts_with("npm:")
        && !trimmed.starts_with("git:")
        && !trimmed.starts_with("github:")
        && !trimmed.starts_with("http:")
        && !trimmed.starts_with("https:")
        && !trimmed.starts_with("ssh:")
}

pub fn normalize_path(input: &str, options: Option<&PathInputOptions>) -> PathBuf {
    let options = options.cloned().unwrap_or_default();
    let mut normalized = if options.trim {
        input.trim().to_string()
    } else {
        input.to_string()
    };

    if options.normalize_unicode_spaces {
        normalized = normalize_unicode_space_chars(&normalized);
    }

    if options.strip_at_prefix && normalized.starts_with('@') {
        normalized = normalized[1..].to_string();
    }

    if options.expand_tilde.unwrap_or(true) {
        if normalized == "~" {
            return home_dir(options.home_dir.as_deref());
        }
        if normalized.starts_with("~/") {
            return home_dir(options.home_dir.as_deref()).join(&normalized[2..]);
        }
    }

    if let Some(path) = file_url_to_path(&normalized) {
        return path;
    }

    PathBuf::from(normalized)
}

pub fn resolve_path(
    input: &str,
    base_dir: impl AsRef<Path>,
    options: Option<&PathInputOptions>,
) -> PathBuf {
    let normalized = normalize_path(input, options);
    normalize_dot_segments(if normalized.is_absolute() {
        normalized
    } else {
        normalize_path(&base_dir.as_ref().to_string_lossy(), None).join(normalized)
    })
}

fn normalize_dot_segments(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

pub fn get_cwd_relative_path(file_path: impl AsRef<Path>, cwd: impl AsRef<Path>) -> Option<String> {
    let resolved_cwd = resolve_path(&cwd.as_ref().to_string_lossy(), "", None);
    let resolved_path = resolve_path(&file_path.as_ref().to_string_lossy(), &resolved_cwd, None);
    let relative = resolved_path.strip_prefix(&resolved_cwd).ok()?;

    if relative.as_os_str().is_empty() {
        Some(".".to_string())
    } else {
        Some(path_to_slash_string(relative))
    }
}

pub fn format_path_relative_to_cwd_or_absolute(
    file_path: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> String {
    get_cwd_relative_path(file_path.as_ref(), cwd.as_ref()).unwrap_or_else(|| {
        path_to_slash_string(&resolve_path(
            &file_path.as_ref().to_string_lossy(),
            cwd,
            None,
        ))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudSyncIgnoreCommand {
    pub program: String,
    pub args: Vec<String>,
}

pub fn mark_path_ignored_by_cloud_sync(path: impl AsRef<Path>) -> io::Result<()> {
    for command in cloud_sync_ignore_commands(path.as_ref()) {
        let status = Command::new(&command.program)
            .args(&command.args)
            .status()?;
        if !status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("{} exited with status {status}", command.program),
            ));
        }
    }
    Ok(())
}

pub fn cloud_sync_ignore_commands(path: impl AsRef<Path>) -> Vec<CloudSyncIgnoreCommand> {
    cloud_sync_ignore_commands_for_platform(path.as_ref(), std::env::consts::OS)
}

pub fn cloud_sync_ignore_commands_for_platform(
    path: impl AsRef<Path>,
    platform: &str,
) -> Vec<CloudSyncIgnoreCommand> {
    let path = path.as_ref().to_string_lossy().to_string();
    match platform {
        "macos" | "darwin" => ["com.dropbox.ignored", "com.apple.fileprovider.ignore#P"]
            .into_iter()
            .map(|attr| CloudSyncIgnoreCommand {
                program: "xattr".to_string(),
                args: vec![
                    "-w".to_string(),
                    attr.to_string(),
                    "1".to_string(),
                    path.clone(),
                ],
            })
            .collect(),
        "linux" => vec![CloudSyncIgnoreCommand {
            program: "setfattr".to_string(),
            args: vec![
                "-n".to_string(),
                "user.com.dropbox.ignored".to_string(),
                "-v".to_string(),
                "1".to_string(),
                path,
            ],
        }],
        _ => Vec::new(),
    }
}

fn normalize_unicode_space_chars(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            let cp = ch as u32;
            if cp == 0x00a0
                || (0x2000..=0x200a).contains(&cp)
                || cp == 0x202f
                || cp == 0x205f
                || cp == 0x3000
            {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

fn home_dir(override_home: Option<&Path>) -> PathBuf {
    override_home
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("~"))
}

fn file_url_to_path(value: &str) -> Option<PathBuf> {
    let rest = value.strip_prefix("file://")?;
    let decoded = percent_decode_utf8(rest).unwrap_or_else(|| rest.to_string());
    if cfg!(target_os = "windows") {
        Some(PathBuf::from(decoded.trim_start_matches('/')))
    } else {
        Some(PathBuf::from(format!(
            "/{}",
            decoded.trim_start_matches('/')
        )))
    }
}

fn path_to_slash_string(path: impl AsRef<Path>) -> String {
    let mut parts = path
        .as_ref()
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>();

    if parts.first().is_some_and(|part| part.as_ref() == "/") {
        parts.remove(0);
        format!("/{}", parts.join("/"))
    } else {
        parts.join("/")
    }
}

fn percent_decode_utf8(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = bytes.get(index + 1).and_then(|byte| hex_value(*byte))?;
            let lo = bytes.get(index + 2).and_then(|byte| hex_value(*byte))?;
            decoded.push((hi << 4) | lo);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_tilde_at_prefix_and_unicode_spaces() {
        let options = PathInputOptions {
            trim: true,
            expand_tilde: Some(true),
            home_dir: Some(PathBuf::from("/home/test")),
            strip_at_prefix: true,
            normalize_unicode_spaces: true,
        };
        assert_eq!(
            normalize_path(" @~/a\u{00a0}b ", Some(&options)),
            PathBuf::from("/home/test/a b")
        );
    }

    #[test]
    fn file_url_paths_decode_percent_escapes_like_node_file_url_to_path() {
        assert_eq!(
            normalize_path("file:///tmp/a%20b/%E4%B8%AD.txt", None),
            PathBuf::from("/tmp/a b/中.txt")
        );
    }

    #[test]
    fn detects_local_and_remote_sources() {
        assert!(is_local_path("./local"));
        assert!(is_local_path("file:///tmp/a"));
        assert!(is_local_path("git@github.com:user/repo.git"));
        assert!(!is_local_path("npm:pkg"));
        assert!(!is_local_path("https://example.com/repo.git"));
    }

    #[test]
    fn formats_relative_path_with_slashes() {
        let cwd = std::env::current_dir().expect("cwd");
        let child = cwd.join("crates").join("coding-agent");
        assert_eq!(
            format_path_relative_to_cwd_or_absolute(&child, &cwd),
            "crates/coding-agent"
        );
    }

    #[test]
    fn resolve_path_normalizes_dot_segments_like_pi_node_resolve() {
        assert_eq!(
            resolve_path("./pkg/../package", "/tmp/work", None),
            PathBuf::from("/tmp/work/package")
        );
    }

    #[test]
    fn formats_relative_path_without_canonicalizing_symlinked_cwd_like_pi() {
        let root = std::env::temp_dir().join(format!("pm-agent-paths-{}", std::process::id()));
        let real = root.join("real");
        let link = root.join("link");
        let child = link.join("child.txt");

        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&real).expect("create real dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).expect("create symlink");

        #[cfg(unix)]
        {
            assert_eq!(
                format_path_relative_to_cwd_or_absolute(&child, &link),
                "child.txt"
            );
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn formats_outside_absolute_path_with_single_leading_slash_like_pi() {
        assert_eq!(
            format_path_relative_to_cwd_or_absolute("/tmp/outside.txt", "/var/work"),
            "/tmp/outside.txt"
        );
    }

    #[test]
    fn cloud_sync_ignore_commands_match_pi_platform_rules() {
        let path = Path::new("/tmp/project-cache");

        let macos = cloud_sync_ignore_commands_for_platform(path, "darwin");
        assert_eq!(macos.len(), 2);
        assert_eq!(macos[0].program, "xattr");
        assert_eq!(
            macos[0].args,
            vec!["-w", "com.dropbox.ignored", "1", "/tmp/project-cache"]
        );
        assert_eq!(
            macos[1].args,
            vec![
                "-w",
                "com.apple.fileprovider.ignore#P",
                "1",
                "/tmp/project-cache"
            ]
        );

        let linux = cloud_sync_ignore_commands_for_platform(path, "linux");
        assert_eq!(linux.len(), 1);
        assert_eq!(linux[0].program, "setfattr");
        assert_eq!(
            linux[0].args,
            vec![
                "-n",
                "user.com.dropbox.ignored",
                "-v",
                "1",
                "/tmp/project-cache"
            ]
        );

        assert!(cloud_sync_ignore_commands_for_platform(path, "win32").is_empty());
    }
}
