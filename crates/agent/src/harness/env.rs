use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub name: String,
    pub path: PathBuf,
    pub kind: FileKind,
    pub size: u64,
    pub mtime_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileErrorCode {
    Aborted,
    NotFound,
    PermissionDenied,
    NotDirectory,
    IsDirectory,
    Invalid,
    NotSupported,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileError {
    pub code: FileErrorCode,
    pub message: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionErrorCode {
    Aborted,
    Timeout,
    ShellUnavailable,
    SpawnError,
    CallbackError,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionError {
    pub code: ExecutionErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct AbortFlag {
    aborted: Arc<AtomicBool>,
}

impl AbortFlag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn abort(&self) {
        self.aborted.store(true, Ordering::SeqCst);
    }

    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }
}

impl ExecutionError {
    pub fn new(code: ExecutionErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellConfig {
    pub shell: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ExecOptions {
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub timeout_seconds: Option<f64>,
    pub abort: Option<AbortFlag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveOptions {
    pub recursive: bool,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempFileOptions {
    pub prefix: String,
    pub suffix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalExecutionEnv {
    cwd: PathBuf,
}

impl LocalExecutionEnv {
    pub fn new(cwd: impl AsRef<Path>) -> Self {
        Self {
            cwd: cwd.as_ref().to_path_buf(),
        }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn absolute_path(&self, path: impl AsRef<Path>) -> PathBuf {
        resolve_path(&self.cwd, path)
    }

    pub fn join_path<I, P>(&self, parts: I) -> PathBuf
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        parts.into_iter().collect()
    }

    pub fn read_text_file(&self, path: impl AsRef<Path>) -> Result<String, FileError> {
        let path = self.absolute_path(path);
        std::fs::read_to_string(&path).map_err(|error| file_error_from_io(error, Some(path)))
    }

    pub fn read_text_lines(
        &self,
        path: impl AsRef<Path>,
        max_lines: Option<usize>,
    ) -> Result<Vec<String>, FileError> {
        if max_lines == Some(0) {
            return Ok(Vec::new());
        }
        let content = self.read_text_file(path)?;
        let mut lines = Vec::new();
        for line in content.lines() {
            lines.push(line.to_string());
            if max_lines.is_some_and(|max_lines| lines.len() >= max_lines) {
                break;
            }
        }
        Ok(lines)
    }

    pub fn read_binary_file(&self, path: impl AsRef<Path>) -> Result<Vec<u8>, FileError> {
        let path = self.absolute_path(path);
        std::fs::read(&path).map_err(|error| file_error_from_io(error, Some(path)))
    }

    pub fn write_file(&self, path: impl AsRef<Path>, content: &[u8]) -> Result<(), FileError> {
        let path = self.absolute_path(path);
        ensure_parent_dir(&path)?;
        std::fs::write(&path, content).map_err(|error| file_error_from_io(error, Some(path)))
    }

    pub fn append_file(&self, path: impl AsRef<Path>, content: &[u8]) -> Result<(), FileError> {
        let path = self.absolute_path(path);
        ensure_parent_dir(&path)?;
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| file_error_from_io(error, Some(path.clone())))?;
        file.write_all(content)
            .map_err(|error| file_error_from_io(error, Some(path)))
    }

    pub fn file_info(&self, path: impl AsRef<Path>) -> Result<FileInfo, FileError> {
        let path = self.absolute_path(path);
        std::fs::symlink_metadata(&path)
            .map(|metadata| file_info_from_metadata(&path, &metadata))
            .map_err(|error| file_error_from_io(error, Some(path)))
    }

    pub fn list_dir(&self, path: impl AsRef<Path>) -> Result<Vec<FileInfo>, FileError> {
        let path = self.absolute_path(path);
        let mut infos = Vec::new();
        let entries = std::fs::read_dir(&path)
            .map_err(|error| file_error_from_io(error, Some(path.clone())))?;
        for entry in entries {
            let entry = entry.map_err(|error| file_error_from_io(error, Some(path.clone())))?;
            let entry_path = entry.path();
            let metadata = std::fs::symlink_metadata(&entry_path)
                .map_err(|error| file_error_from_io(error, Some(entry_path.clone())))?;
            infos.push(file_info_from_metadata(entry_path, &metadata));
        }
        infos.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(infos)
    }

    pub fn canonical_path(&self, path: impl AsRef<Path>) -> Result<PathBuf, FileError> {
        let path = self.absolute_path(path);
        std::fs::canonicalize(&path).map_err(|error| file_error_from_io(error, Some(path)))
    }

    pub fn exists(&self, path: impl AsRef<Path>) -> Result<bool, FileError> {
        match self.file_info(path) {
            Ok(_) => Ok(true),
            Err(error) if error.code == FileErrorCode::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn create_dir(&self, path: impl AsRef<Path>, recursive: bool) -> Result<(), FileError> {
        let path = self.absolute_path(path);
        let result = if recursive {
            std::fs::create_dir_all(&path)
        } else {
            std::fs::create_dir(&path)
        };
        result.map_err(|error| file_error_from_io(error, Some(path)))
    }

    pub fn remove(&self, path: impl AsRef<Path>, options: RemoveOptions) -> Result<(), FileError> {
        let path = self.absolute_path(path);
        if options.force && !path.exists() {
            return Ok(());
        }
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| file_error_from_io(error, Some(path.clone())))?;
        let result = if metadata.is_dir() && !metadata.file_type().is_symlink() {
            if options.recursive {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_dir(&path)
            }
        } else {
            std::fs::remove_file(&path)
        };
        result.map_err(|error| file_error_from_io(error, Some(path)))
    }

    pub fn create_temp_dir(&self, prefix: &str) -> Result<PathBuf, FileError> {
        let path = std::env::temp_dir().join(format!("{prefix}{}", unique_id()));
        std::fs::create_dir(&path)
            .map_err(|error| file_error_from_io(error, Some(path.clone())))?;
        Ok(path)
    }

    pub fn create_temp_file(&self, options: TempFileOptions) -> Result<PathBuf, FileError> {
        let dir = self.create_temp_dir("tmp-")?;
        let path = dir.join(format!(
            "{}{}{}",
            options.prefix,
            unique_id(),
            options.suffix
        ));
        std::fs::write(&path, b"")
            .map_err(|error| file_error_from_io(error, Some(path.clone())))?;
        Ok(path)
    }

    pub fn exec(
        &self,
        command: impl AsRef<str>,
        options: ExecOptions,
    ) -> Result<ExecOutput, ExecutionError> {
        self.exec_with_callbacks(command, options, None, None)
    }

    pub fn exec_with_callbacks(
        &self,
        command: impl AsRef<str>,
        options: ExecOptions,
        mut on_stdout: Option<&mut dyn FnMut(&str) -> Result<(), ExecutionError>>,
        mut on_stderr: Option<&mut dyn FnMut(&str) -> Result<(), ExecutionError>>,
    ) -> Result<ExecOutput, ExecutionError> {
        if options.abort.as_ref().is_some_and(AbortFlag::is_aborted) {
            return Err(ExecutionError::new(ExecutionErrorCode::Aborted, "aborted"));
        }
        let shell = resolve_shell_config(None)?;
        let cwd = options
            .cwd
            .as_ref()
            .map(|cwd| resolve_path(&self.cwd, cwd))
            .unwrap_or_else(|| self.cwd.clone());
        let mut process = Command::new(&shell.shell);
        process
            .args(&shell.args)
            .arg(command.as_ref())
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in options.env {
            process.env(key, value);
        }

        let start = std::time::Instant::now();
        let mut child = process.spawn().map_err(|error| {
            ExecutionError::new(ExecutionErrorCode::SpawnError, error.to_string())
        })?;
        let timeout = options
            .timeout_seconds
            .map(std::time::Duration::from_secs_f64);
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (chunk_tx, chunk_rx) = std::sync::mpsc::channel::<StreamChunk>();
        spawn_reader(StreamKind::Stdout, stdout, chunk_tx.clone());
        spawn_reader(StreamKind::Stderr, stderr, chunk_tx);
        let mut stdout = String::new();
        let mut stderr = String::new();

        loop {
            while let Ok(chunk) = chunk_rx.try_recv() {
                match chunk {
                    StreamChunk::Data {
                        kind: StreamKind::Stdout,
                        text,
                    } => {
                        stdout.push_str(&text);
                        if let Some(on_stdout) = on_stdout.as_mut() {
                            if let Err(error) = on_stdout(&text) {
                                kill_child(&mut child);
                                return Err(error);
                            }
                        }
                    }
                    StreamChunk::Data {
                        kind: StreamKind::Stderr,
                        text,
                    } => {
                        stderr.push_str(&text);
                        if let Some(on_stderr) = on_stderr.as_mut() {
                            if let Err(error) = on_stderr(&text) {
                                kill_child(&mut child);
                                return Err(error);
                            }
                        }
                    }
                }
            }
            if options.abort.as_ref().is_some_and(AbortFlag::is_aborted) {
                kill_child(&mut child);
                return Err(ExecutionError::new(ExecutionErrorCode::Aborted, "aborted"));
            }
            if let Some(timeout) = timeout {
                if start.elapsed() >= timeout {
                    kill_child(&mut child);
                    return Err(ExecutionError::new(
                        ExecutionErrorCode::Timeout,
                        format!("timeout:{}", format_timeout_seconds(timeout.as_secs_f64())),
                    ));
                }
            }
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(error) => {
                    kill_child(&mut child);
                    return Err(ExecutionError::new(
                        ExecutionErrorCode::SpawnError,
                        error.to_string(),
                    ));
                }
            }
        }

        let status = child.wait().map_err(|error| {
            ExecutionError::new(ExecutionErrorCode::SpawnError, error.to_string())
        })?;
        drain_stream_chunks(&chunk_rx, &mut stdout, &mut stderr)?;
        let exit_code = status.code().unwrap_or(0);
        Ok(ExecOutput {
            stdout,
            stderr,
            exit_code,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    Stdout,
    Stderr,
}

enum StreamChunk {
    Data { kind: StreamKind, text: String },
}

fn spawn_reader(
    kind: StreamKind,
    stream: Option<impl Read + Send + 'static>,
    tx: std::sync::mpsc::Sender<StreamChunk>,
) {
    let Some(mut stream) = stream else {
        return;
    };
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    if tx.send(StreamChunk::Data { kind, text }).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn drain_stream_chunks(
    rx: &std::sync::mpsc::Receiver<StreamChunk>,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<(), ExecutionError> {
    while let Ok(chunk) = rx.try_recv() {
        match chunk {
            StreamChunk::Data {
                kind: StreamKind::Stdout,
                text,
            } => stdout.push_str(&text),
            StreamChunk::Data {
                kind: StreamKind::Stderr,
                text,
            } => stderr.push_str(&text),
        }
    }
    Ok(())
}

impl FileError {
    pub fn new(code: FileErrorCode, message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            code,
            message: message.into(),
            path,
        }
    }
}

pub fn resolve_path(cwd: impl AsRef<Path>, path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.as_ref().join(path)
    }
}

pub fn resolve_shell_config(custom_shell: Option<&Path>) -> Result<ShellConfig, ExecutionError> {
    if let Some(custom_shell) = custom_shell {
        if custom_shell.exists() {
            return Ok(ShellConfig {
                shell: custom_shell.to_path_buf(),
                args: vec!["-c".to_string()],
            });
        }
        return Err(ExecutionError::new(
            ExecutionErrorCode::ShellUnavailable,
            format!("Custom shell path not found: {}", custom_shell.display()),
        ));
    }

    let bin_bash = PathBuf::from("/bin/bash");
    if bin_bash.exists() {
        return Ok(ShellConfig {
            shell: bin_bash,
            args: vec!["-c".to_string()],
        });
    }

    if let Some(path_bash) = find_on_path("bash") {
        return Ok(ShellConfig {
            shell: path_bash,
            args: vec!["-c".to_string()],
        });
    }

    Ok(ShellConfig {
        shell: PathBuf::from("sh"),
        args: vec!["-c".to_string()],
    })
}

fn find_on_path(program: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for path in std::env::split_paths(&paths) {
        let candidate = path.join(program);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn kill_child(child: &mut std::process::Child) {
    child.kill().ok();
    child.wait().ok();
}

fn format_timeout_seconds(seconds: f64) -> String {
    let formatted = format!("{seconds}");
    formatted
        .strip_suffix(".0")
        .map_or(formatted.clone(), ToString::to_string)
}

pub fn file_error_code_from_io_kind(kind: std::io::ErrorKind) -> FileErrorCode {
    match kind {
        std::io::ErrorKind::NotFound => FileErrorCode::NotFound,
        std::io::ErrorKind::PermissionDenied => FileErrorCode::PermissionDenied,
        std::io::ErrorKind::NotADirectory => FileErrorCode::NotDirectory,
        std::io::ErrorKind::IsADirectory => FileErrorCode::IsDirectory,
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
            FileErrorCode::Invalid
        }
        std::io::ErrorKind::Unsupported => FileErrorCode::NotSupported,
        _ => FileErrorCode::Unknown,
    }
}

pub fn file_error_from_io(error: std::io::Error, path: Option<PathBuf>) -> FileError {
    FileError::new(
        file_error_code_from_io_kind(error.kind()),
        error.to_string(),
        path,
    )
}

fn ensure_parent_dir(path: &Path) -> Result<(), FileError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    std::fs::create_dir_all(parent)
        .map_err(|error| file_error_from_io(error, Some(parent.to_path_buf())))
}

fn unique_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{nanos:x}-{count:x}")
}

pub fn file_info_from_metadata(path: impl AsRef<Path>, metadata: &std::fs::Metadata) -> FileInfo {
    let path = path.as_ref().to_path_buf();
    let kind = if metadata.is_file() {
        FileKind::File
    } else if metadata.is_dir() {
        FileKind::Directory
    } else if metadata.file_type().is_symlink() {
        FileKind::Symlink
    } else {
        FileKind::File
    };
    build_file_info(path, kind, metadata.len(), metadata_mtime_ms(metadata))
}

#[cfg(unix)]
pub fn file_info_from_unix_mode(
    path: impl AsRef<Path>,
    mode: u32,
    size: u64,
    mtime_ms: u64,
) -> Option<FileInfo> {
    let kind = match mode & unix_mode::S_IFMT {
        unix_mode::S_IFREG => FileKind::File,
        unix_mode::S_IFDIR => FileKind::Directory,
        unix_mode::S_IFLNK => FileKind::Symlink,
        _ => return None,
    };
    Some(build_file_info(
        path.as_ref().to_path_buf(),
        kind,
        size,
        mtime_ms,
    ))
}

fn build_file_info(path: PathBuf, kind: FileKind, size: u64, mtime_ms: u64) -> FileInfo {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    FileInfo {
        name,
        path,
        kind,
        size,
        mtime_ms,
    }
}

fn metadata_mtime_ms(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_millis() as u64)
}

#[cfg(unix)]
mod unix_mode {
    pub const S_IFMT: u32 = 0o170000;
    pub const S_IFREG: u32 = 0o100000;
    pub const S_IFDIR: u32 = 0o040000;
    pub const S_IFLNK: u32 = 0o120000;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resolves_relative_paths_against_cwd_like_node_env() {
        assert_eq!(
            resolve_path(PathBuf::from("/workspace"), "src/main.rs"),
            PathBuf::from("/workspace/src/main.rs")
        );
        assert_eq!(
            resolve_path(PathBuf::from("/workspace"), "/tmp/file"),
            PathBuf::from("/tmp/file")
        );
    }

    #[test]
    fn maps_platform_file_errors_to_pi_codes() {
        assert_eq!(
            file_error_code_from_io_kind(std::io::ErrorKind::NotFound),
            FileErrorCode::NotFound
        );
        assert_eq!(
            file_error_code_from_io_kind(std::io::ErrorKind::PermissionDenied),
            FileErrorCode::PermissionDenied
        );
        assert_eq!(
            file_error_code_from_io_kind(std::io::ErrorKind::InvalidInput),
            FileErrorCode::Invalid
        );
    }

    #[cfg(unix)]
    #[test]
    fn builds_file_info_from_unix_file_type_like_node_stats() {
        let info =
            file_info_from_unix_mode("/workspace/link", 0o120000, 42, 1234).expect("file info");

        assert_eq!(info.name, "link");
        assert_eq!(info.path, PathBuf::from("/workspace/link"));
        assert_eq!(info.kind, FileKind::Symlink);
        assert_eq!(info.size, 42);
        assert_eq!(info.mtime_ms, 1234);
    }

    #[test]
    fn local_execution_env_reads_writes_lists_and_removes_files_like_node_env() {
        let root = unique_temp_dir("agent-env-local");
        let env = LocalExecutionEnv::new(&root);

        env.write_file("nested/file.txt", b"one\ntwo\nthree")
            .expect("write");
        env.append_file("nested/file.txt", b"\nfour")
            .expect("append");

        assert_eq!(
            env.read_text_file("nested/file.txt").expect("read text"),
            "one\ntwo\nthree\nfour"
        );
        assert_eq!(
            env.read_text_lines("nested/file.txt", Some(2))
                .expect("read lines"),
            vec!["one".to_string(), "two".to_string()]
        );
        assert!(env.exists("nested/file.txt").expect("exists"));

        let info = env.file_info("nested/file.txt").expect("file info");
        assert_eq!(info.name, "file.txt");
        assert_eq!(info.kind, FileKind::File);

        let entries = env.list_dir("nested").expect("list dir");
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["file.txt"]
        );

        let canonical = env.canonical_path("nested/file.txt").expect("canonical");
        assert!(canonical.ends_with("nested/file.txt"));

        env.remove(
            "nested",
            RemoveOptions {
                recursive: true,
                force: false,
            },
        )
        .expect("remove");
        assert!(!env.exists("nested/file.txt").expect("missing"));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn local_execution_env_creates_temp_files_with_prefix_and_suffix() {
        let root = unique_temp_dir("agent-env-temp");
        let env = LocalExecutionEnv::new(&root);

        let file = env
            .create_temp_file(TempFileOptions {
                prefix: "bash-".to_string(),
                suffix: ".log".to_string(),
            })
            .expect("temp file");

        let name = file.file_name().expect("name").to_string_lossy();
        assert!(name.starts_with("bash-"));
        assert!(name.ends_with(".log"));
        assert!(file.exists());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn local_execution_env_exec_runs_shell_command_with_cwd_env_and_exit_code() {
        let root = unique_temp_dir("agent-env-exec");
        let env = LocalExecutionEnv::new(&root);
        env.write_file("marker.txt", b"marker")
            .expect("write marker");

        let result = env
            .exec(
                "printf \"$PI_TEST_VALUE:\"; pwd; test -f marker.txt; printf err >&2; exit 7",
                ExecOptions {
                    cwd: None,
                    env: [("PI_TEST_VALUE".to_string(), "ok".to_string())].into(),
                    ..ExecOptions::default()
                },
            )
            .expect("exec result");

        assert_eq!(result.exit_code, 7);
        assert!(result.stdout.starts_with("ok:"));
        assert!(result.stdout.contains(root.to_string_lossy().as_ref()));
        assert_eq!(result.stderr, "err");

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn resolves_shell_config_like_node_env_custom_shell_validation() {
        let shell = resolve_shell_config(Some(Path::new("/definitely/missing/shell")))
            .expect_err("missing");

        assert_eq!(shell.code, ExecutionErrorCode::ShellUnavailable);
    }

    #[test]
    fn local_execution_env_exec_times_out_like_node_env() {
        let root = unique_temp_dir("agent-env-timeout");
        let env = LocalExecutionEnv::new(&root);

        let error = env
            .exec(
                "sleep 2",
                ExecOptions {
                    timeout_seconds: Some(0.01),
                    ..ExecOptions::default()
                },
            )
            .expect_err("timeout");

        assert_eq!(error.code, ExecutionErrorCode::Timeout);
        assert!(error.message.contains("timeout:0.01"));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn local_execution_env_exec_maps_stdout_callback_failure_like_node_env() {
        let root = unique_temp_dir("agent-env-callback");
        let env = LocalExecutionEnv::new(&root);

        let error = env
            .exec_with_callbacks(
                "printf hello",
                ExecOptions::default(),
                Some(&mut |_| {
                    Err(ExecutionError::new(
                        ExecutionErrorCode::CallbackError,
                        "callback failed",
                    ))
                }),
                None,
            )
            .expect_err("callback error");

        assert_eq!(error.code, ExecutionErrorCode::CallbackError);
        assert_eq!(error.message, "callback failed");

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn local_execution_env_exec_returns_aborted_before_spawn_like_node_env() {
        let root = unique_temp_dir("agent-env-abort-before");
        let env = LocalExecutionEnv::new(&root);
        let abort = AbortFlag::new();
        abort.abort();

        let error = env
            .exec(
                "printf never",
                ExecOptions {
                    abort: Some(abort.clone()),
                    ..ExecOptions::default()
                },
            )
            .expect_err("aborted");

        assert_eq!(error.code, ExecutionErrorCode::Aborted);
        assert_eq!(error.message, "aborted");

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn local_execution_env_exec_kills_running_command_when_abort_flag_is_set() {
        let root = unique_temp_dir("agent-env-abort-running");
        let env = LocalExecutionEnv::new(&root);
        let abort = AbortFlag::new();
        let start = std::time::Instant::now();

        let error = env
            .exec_with_callbacks(
                "printf started; sleep 2",
                ExecOptions {
                    abort: Some(abort.clone()),
                    ..ExecOptions::default()
                },
                Some(&mut |_| {
                    abort.abort();
                    Ok(())
                }),
                None,
            )
            .expect_err("aborted");

        assert_eq!(error.code, ExecutionErrorCode::Aborted);
        assert!(start.elapsed() < std::time::Duration::from_secs(1));

        std::fs::remove_dir_all(root).ok();
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }
}
