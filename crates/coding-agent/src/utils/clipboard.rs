use crate::utils::base64::encode_base64;
use std::io::{self, Write};
use std::process::{Command, Stdio};

pub const MAX_OSC52_ENCODED_LENGTH: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardError {
    Osc52PayloadTooLarge,
    CopyFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardPlatform {
    Macos,
    Windows,
    Linux,
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClipboardEnvironment {
    pub termux: bool,
    pub wayland_display: bool,
    pub x11_display: bool,
    pub wayland_session: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardCommandMode {
    Exec,
    Spawn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardCommand {
    pub mode: ClipboardCommandMode,
    pub command: String,
}

impl ClipboardCommand {
    pub fn exec(command: impl Into<String>) -> Self {
        Self {
            mode: ClipboardCommandMode::Exec,
            command: command.into(),
        }
    }

    pub fn spawn(command: impl Into<String>) -> Self {
        Self {
            mode: ClipboardCommandMode::Spawn,
            command: command.into(),
        }
    }

    pub fn program_and_args(&self) -> (String, Vec<String>) {
        let mut parts = self.command.split_whitespace();
        let program = parts.next().unwrap_or_default().to_string();
        let args = parts.map(str::to_string).collect();
        (program, args)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardContext {
    pub platform: ClipboardPlatform,
    pub environment: ClipboardEnvironment,
    pub remote: bool,
}

impl ClipboardContext {
    pub fn current() -> Self {
        let env = std::env::vars().collect::<Vec<_>>();
        Self::from_platform_and_env(current_clipboard_platform(), env)
    }

    pub fn from_platform_and_env<K, V>(
        platform: ClipboardPlatform,
        env: impl IntoIterator<Item = (K, V)> + Clone,
    ) -> Self
    where
        K: AsRef<str>,
        V: AsRef<str>,
    {
        Self {
            platform,
            environment: ClipboardEnvironment::from_env(env.clone()),
            remote: is_remote_session_env(env),
        }
    }
}

impl ClipboardEnvironment {
    pub fn from_env<K, V>(env: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let mut environment = ClipboardEnvironment::default();
        for (key, value) in env {
            let key = key.as_ref();
            let value = value.as_ref();
            if value.is_empty() {
                continue;
            }
            match key {
                "TERMUX_VERSION" => environment.termux = true,
                "WAYLAND_DISPLAY" => environment.wayland_display = true,
                "DISPLAY" => environment.x11_display = true,
                "XDG_SESSION_TYPE" if value.eq_ignore_ascii_case("wayland") => {
                    environment.wayland_session = true;
                }
                _ => {}
            }
        }
        environment
    }
}

pub trait ClipboardRunner {
    fn run_command(&mut self, command: &ClipboardCommand, text: &str) -> bool;

    fn emit_osc52(&mut self, sequence: &str) -> bool;
}

#[derive(Debug, Default)]
pub struct SystemClipboardRunner;

impl ClipboardRunner for SystemClipboardRunner {
    fn run_command(&mut self, command: &ClipboardCommand, text: &str) -> bool {
        match command.mode {
            ClipboardCommandMode::Exec => run_clipboard_exec(command, text),
            ClipboardCommandMode::Spawn => run_clipboard_spawn(command, text),
        }
    }

    fn emit_osc52(&mut self, sequence: &str) -> bool {
        let mut stdout = io::stdout();
        stdout.write_all(sequence.as_bytes()).is_ok() && stdout.flush().is_ok()
    }
}

pub fn is_remote_session_env<K, V>(env: impl IntoIterator<Item = (K, V)>) -> bool
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    env.into_iter().any(|(key, value)| {
        let key = key.as_ref();
        !value.as_ref().is_empty()
            && matches!(key, "SSH_CONNECTION" | "SSH_CLIENT" | "MOSH_CONNECTION")
    })
}

pub fn clipboard_command_plan(
    platform: ClipboardPlatform,
    environment: ClipboardEnvironment,
) -> Vec<ClipboardCommand> {
    match platform {
        ClipboardPlatform::Macos => vec![ClipboardCommand::exec("pbcopy")],
        ClipboardPlatform::Windows => vec![ClipboardCommand::exec("clip")],
        ClipboardPlatform::Linux => linux_clipboard_command_plan(environment),
        ClipboardPlatform::Other => Vec::new(),
    }
}

pub fn copy_to_clipboard(text: &str) -> Result<(), ClipboardError> {
    let context = ClipboardContext::current();
    let mut runner = SystemClipboardRunner;
    copy_to_clipboard_with_runner(
        text,
        context.platform,
        context.environment,
        context.remote,
        &mut runner,
    )
}

pub fn copy_to_clipboard_with_runner(
    text: &str,
    platform: ClipboardPlatform,
    environment: ClipboardEnvironment,
    remote: bool,
    runner: &mut impl ClipboardRunner,
) -> Result<(), ClipboardError> {
    let mut copied = false;

    for command in clipboard_command_plan(platform, environment) {
        if runner.run_command(&command, text) {
            copied = true;
            break;
        }
    }

    if remote || !copied {
        let sequence = osc52_sequence(text)?;
        let osc52_copied = runner.emit_osc52(&sequence);
        copied = copied || osc52_copied;
    }

    if copied {
        Ok(())
    } else {
        Err(ClipboardError::CopyFailed)
    }
}

fn current_clipboard_platform() -> ClipboardPlatform {
    if cfg!(target_os = "macos") {
        ClipboardPlatform::Macos
    } else if cfg!(target_os = "windows") {
        ClipboardPlatform::Windows
    } else if cfg!(target_os = "linux") {
        ClipboardPlatform::Linux
    } else {
        ClipboardPlatform::Other
    }
}

pub fn osc52_sequence(text: &str) -> Result<String, ClipboardError> {
    let encoded = encode_base64(text.as_bytes());
    if encoded.len() > MAX_OSC52_ENCODED_LENGTH {
        return Err(ClipboardError::Osc52PayloadTooLarge);
    }
    Ok(format!("\x1b]52;c;{encoded}\x07"))
}

fn linux_clipboard_command_plan(environment: ClipboardEnvironment) -> Vec<ClipboardCommand> {
    let mut commands = Vec::new();

    if environment.termux {
        commands.push(ClipboardCommand::exec("termux-clipboard-set"));
    }

    if environment.wayland_session && environment.wayland_display {
        commands.push(ClipboardCommand::spawn("wl-copy"));
        if environment.x11_display {
            commands.extend(x11_clipboard_commands());
        }
    } else if environment.x11_display {
        commands.extend(x11_clipboard_commands());
    }

    commands
}

fn x11_clipboard_commands() -> Vec<ClipboardCommand> {
    vec![
        ClipboardCommand::exec("xclip -selection clipboard"),
        ClipboardCommand::exec("xsel --clipboard --input"),
    ]
}

fn run_clipboard_exec(command: &ClipboardCommand, text: &str) -> bool {
    let (program, args) = command.program_and_args();
    if program.is_empty() {
        return false;
    }

    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    let wrote_stdin = child
        .stdin
        .as_mut()
        .map(|stdin| stdin.write_all(text.as_bytes()).is_ok())
        .unwrap_or(false);
    drop(child.stdin.take());

    wrote_stdin && child.wait().map(|status| status.success()).unwrap_or(false)
}

fn run_clipboard_spawn(command: &ClipboardCommand, text: &str) -> bool {
    let (program, args) = command.program_and_args();
    if program.is_empty() {
        return false;
    }

    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    let wrote_stdin = child
        .stdin
        .as_mut()
        .map(|stdin| stdin.write_all(text.as_bytes()).is_ok())
        .unwrap_or(false);
    drop(child.stdin.take());

    wrote_stdin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct FakeClipboardRunner {
        command_results: Vec<bool>,
        osc52_result: bool,
        calls: Vec<String>,
    }

    impl ClipboardRunner for FakeClipboardRunner {
        fn run_command(&mut self, command: &ClipboardCommand, _text: &str) -> bool {
            self.calls.push(format!("command:{}", command.command));
            self.command_results.remove(0)
        }

        fn emit_osc52(&mut self, sequence: &str) -> bool {
            self.calls.push(format!("osc52:{sequence}"));
            self.osc52_result
        }
    }

    #[test]
    fn detects_remote_sessions_like_pi_clipboard() {
        assert!(!is_remote_session_env(Vec::<(&str, &str)>::new()));
        assert!(is_remote_session_env([("SSH_CONNECTION", "1")]));
        assert!(is_remote_session_env([("SSH_CLIENT", "1")]));
        assert!(is_remote_session_env([("MOSH_CONNECTION", "1")]));
    }

    #[test]
    fn builds_osc52_sequence_with_length_limit() {
        assert_eq!(
            osc52_sequence("copy").expect("osc52"),
            "\x1b]52;c;Y29weQ==\x07"
        );

        let max_payload = "a".repeat((MAX_OSC52_ENCODED_LENGTH / 4) * 3);
        assert!(osc52_sequence(&max_payload).is_ok());

        let oversized = format!("{max_payload}a");
        assert_eq!(
            osc52_sequence(&oversized).unwrap_err(),
            ClipboardError::Osc52PayloadTooLarge
        );
    }

    #[test]
    fn plans_platform_clipboard_commands_like_pi() {
        assert_eq!(
            clipboard_command_plan(ClipboardPlatform::Macos, ClipboardEnvironment::default(),),
            vec![ClipboardCommand::exec("pbcopy")]
        );
        assert_eq!(
            clipboard_command_plan(ClipboardPlatform::Windows, ClipboardEnvironment::default(),),
            vec![ClipboardCommand::exec("clip")]
        );
        assert_eq!(
            clipboard_command_plan(
                ClipboardPlatform::Linux,
                ClipboardEnvironment {
                    termux: true,
                    wayland_display: true,
                    x11_display: true,
                    wayland_session: true,
                },
            ),
            vec![
                ClipboardCommand::exec("termux-clipboard-set"),
                ClipboardCommand::spawn("wl-copy"),
                ClipboardCommand::exec("xclip -selection clipboard"),
                ClipboardCommand::exec("xsel --clipboard --input"),
            ]
        );
        assert_eq!(
            clipboard_command_plan(
                ClipboardPlatform::Linux,
                ClipboardEnvironment {
                    x11_display: true,
                    ..ClipboardEnvironment::default()
                },
            ),
            vec![
                ClipboardCommand::exec("xclip -selection clipboard"),
                ClipboardCommand::exec("xsel --clipboard --input"),
            ]
        );
        assert!(
            clipboard_command_plan(ClipboardPlatform::Other, ClipboardEnvironment::default(),)
                .is_empty()
        );
    }

    #[test]
    fn copy_with_runner_skips_osc52_when_local_command_succeeds() {
        let mut runner = FakeClipboardRunner {
            command_results: vec![true],
            osc52_result: false,
            calls: Vec::new(),
        };

        copy_to_clipboard_with_runner(
            "hello",
            ClipboardPlatform::Macos,
            ClipboardEnvironment::default(),
            false,
            &mut runner,
        )
        .expect("copy");

        assert_eq!(runner.calls, vec!["command:pbcopy"]);
    }

    #[test]
    fn copy_with_runner_emits_osc52_for_remote_even_after_command_success() {
        let mut runner = FakeClipboardRunner {
            command_results: vec![true],
            osc52_result: true,
            calls: Vec::new(),
        };

        copy_to_clipboard_with_runner(
            "hello",
            ClipboardPlatform::Macos,
            ClipboardEnvironment::default(),
            true,
            &mut runner,
        )
        .expect("copy");

        assert_eq!(
            runner.calls,
            vec!["command:pbcopy", "osc52:\x1b]52;c;aGVsbG8=\x07"]
        );
    }

    #[test]
    fn copy_with_runner_falls_back_to_osc52_when_commands_fail() {
        let mut runner = FakeClipboardRunner {
            command_results: vec![false, false],
            osc52_result: true,
            calls: Vec::new(),
        };

        copy_to_clipboard_with_runner(
            "hello",
            ClipboardPlatform::Linux,
            ClipboardEnvironment {
                x11_display: true,
                ..ClipboardEnvironment::default()
            },
            false,
            &mut runner,
        )
        .expect("copy");

        assert_eq!(
            runner.calls,
            vec![
                "command:xclip -selection clipboard",
                "command:xsel --clipboard --input",
                "osc52:\x1b]52;c;aGVsbG8=\x07",
            ]
        );
    }

    #[test]
    fn copy_with_runner_errors_when_no_command_or_osc52_succeeds() {
        let mut runner = FakeClipboardRunner {
            command_results: Vec::new(),
            osc52_result: false,
            calls: Vec::new(),
        };

        assert_eq!(
            copy_to_clipboard_with_runner(
                "hello",
                ClipboardPlatform::Other,
                ClipboardEnvironment::default(),
                false,
                &mut runner,
            )
            .unwrap_err(),
            ClipboardError::CopyFailed
        );
    }

    #[test]
    fn clipboard_commands_expose_program_and_args() {
        assert_eq!(
            ClipboardCommand::exec("xclip -selection clipboard").program_and_args(),
            (
                "xclip".to_string(),
                vec!["-selection".to_string(), "clipboard".to_string()]
            )
        );
        assert_eq!(
            ClipboardCommand::spawn("wl-copy").program_and_args(),
            ("wl-copy".to_string(), Vec::<String>::new())
        );
    }

    #[test]
    fn derives_clipboard_context_from_platform_and_env_like_pi() {
        let context = ClipboardContext::from_platform_and_env(
            ClipboardPlatform::Linux,
            [
                ("TERMUX_VERSION", "1"),
                ("WAYLAND_DISPLAY", "wayland-1"),
                ("DISPLAY", ":0"),
                ("XDG_SESSION_TYPE", "wayland"),
                ("SSH_CONNECTION", "remote"),
            ],
        );

        assert_eq!(context.platform, ClipboardPlatform::Linux);
        assert_eq!(
            context.environment,
            ClipboardEnvironment {
                termux: true,
                wayland_display: true,
                x11_display: true,
                wayland_session: true,
            }
        );
        assert!(context.remote);
    }
}
