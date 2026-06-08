#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedTool {
    Fd,
    Rg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureToolPlan {
    UseLocal { path: String },
    UseSystem { command: String },
    SkipOffline { message: String },
    TermuxInstallHint { message: String },
    Download { asset_name: String },
    UnsupportedPlatform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureToolContext<'a> {
    pub platform: &'a str,
    pub architecture: &'a str,
    pub version: &'a str,
    pub local_path: Option<&'a str>,
    pub available_system_commands: &'a [&'a str],
    pub offline_value: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInstallLayout {
    pub archive_path: String,
    pub binary_path: String,
    pub extract_dir_prefix: String,
    pub extracted_binary_candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionCommandPlan {
    pub command: String,
    pub args: Vec<String>,
}

pub fn extraction_command_plans(
    archive_path: &str,
    extract_dir: &str,
    asset_name: &str,
    platform: &str,
    system_root: Option<&str>,
) -> Result<Vec<ExtractionCommandPlan>, String> {
    if asset_name.ends_with(".tar.gz") {
        return Ok(vec![ExtractionCommandPlan {
            command: "tar".to_string(),
            args: vec![
                "xzf".to_string(),
                archive_path.to_string(),
                "-C".to_string(),
                extract_dir.to_string(),
            ],
        }]);
    }

    if !asset_name.ends_with(".zip") {
        return Err(format!("Unsupported archive format: {asset_name}"));
    }

    if platform == "win32" {
        let script =
            "& { param($archive, $destination) $ErrorActionPreference = 'Stop'; Expand-Archive -LiteralPath $archive -DestinationPath $destination -Force }";
        return Ok(vec![
            ExtractionCommandPlan {
                command: windows_tar_command(system_root),
                args: vec![
                    "xf".to_string(),
                    archive_path.to_string(),
                    "-C".to_string(),
                    extract_dir.to_string(),
                ],
            },
            ExtractionCommandPlan {
                command: "powershell.exe".to_string(),
                args: vec![
                    "-NoLogo".to_string(),
                    "-NoProfile".to_string(),
                    "-NonInteractive".to_string(),
                    "-ExecutionPolicy".to_string(),
                    "Bypass".to_string(),
                    "-Command".to_string(),
                    script.to_string(),
                    archive_path.to_string(),
                    extract_dir.to_string(),
                ],
            },
        ]);
    }

    Ok(vec![
        ExtractionCommandPlan {
            command: "unzip".to_string(),
            args: vec![
                "-q".to_string(),
                archive_path.to_string(),
                "-d".to_string(),
                extract_dir.to_string(),
            ],
        },
        ExtractionCommandPlan {
            command: "tar".to_string(),
            args: vec![
                "xf".to_string(),
                archive_path.to_string(),
                "-C".to_string(),
                extract_dir.to_string(),
            ],
        },
    ])
}

pub fn tool_install_layout(
    tool: ManagedTool,
    tools_dir: &str,
    asset_name: &str,
    platform: &str,
) -> ToolInstallLayout {
    let binary_file_name = binary_file_name(tool, platform);
    let archive_path = join_tool_path(tools_dir, asset_name);
    let binary_path = join_tool_path(tools_dir, &binary_file_name);
    let extract_dir_prefix =
        join_tool_path(tools_dir, &format!("extract_tmp_{}_", binary_name(tool)));
    let extracted_dir_name = archive_root_name(asset_name);
    ToolInstallLayout {
        archive_path,
        binary_path,
        extracted_binary_candidates: vec![
            join_tool_path(
                &join_tool_path(&extract_dir_prefix, &extracted_dir_name),
                &binary_file_name,
            ),
            join_tool_path(&extract_dir_prefix, &binary_file_name),
        ],
        extract_dir_prefix,
    }
}

pub fn ensure_tool_plan(tool: ManagedTool, context: EnsureToolContext<'_>) -> EnsureToolPlan {
    if let Some(path) = context.local_path {
        return EnsureToolPlan::UseLocal {
            path: path.to_string(),
        };
    }

    for candidate in system_binary_names(tool) {
        if context
            .available_system_commands
            .iter()
            .any(|available| *available == *candidate)
        {
            return EnsureToolPlan::UseSystem {
                command: candidate.to_string(),
            };
        }
    }

    if is_offline_mode_value_enabled(context.offline_value) {
        return EnsureToolPlan::SkipOffline {
            message: format!(
                "{} not found. Offline mode enabled, skipping download.",
                tool_display_name(tool)
            ),
        };
    }

    if context.platform == "android" {
        return EnsureToolPlan::TermuxInstallHint {
            message: format!(
                "{} not found. Install with: {}",
                tool_display_name(tool),
                termux_install_hint(tool)
            ),
        };
    }

    let Some(asset_name) = tool_asset_name(
        tool,
        context.version,
        context.platform,
        context.architecture,
    ) else {
        return EnsureToolPlan::UnsupportedPlatform;
    };
    EnsureToolPlan::Download { asset_name }
}

pub fn tool_asset_name(
    tool: ManagedTool,
    version: &str,
    platform: &str,
    architecture: &str,
) -> Option<String> {
    match tool {
        ManagedTool::Fd => fd_asset_name(version, platform, architecture),
        ManagedTool::Rg => ripgrep_asset_name(version, platform, architecture),
    }
}

pub fn is_offline_mode_value_enabled(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
}

pub fn termux_install_hint(tool: ManagedTool) -> String {
    format!("pkg install {}", termux_package_name(tool))
}

fn fd_asset_name(version: &str, platform: &str, architecture: &str) -> Option<String> {
    let arch = rust_like_archive_arch(architecture);
    match platform {
        "darwin" => Some(format!("fd-v{version}-{arch}-apple-darwin.tar.gz")),
        "linux" => Some(format!("fd-v{version}-{arch}-unknown-linux-gnu.tar.gz")),
        "win32" => Some(format!("fd-v{version}-{arch}-pc-windows-msvc.zip")),
        _ => None,
    }
}

fn ripgrep_asset_name(version: &str, platform: &str, architecture: &str) -> Option<String> {
    let arch = rust_like_archive_arch(architecture);
    match platform {
        "darwin" => Some(format!("ripgrep-{version}-{arch}-apple-darwin.tar.gz")),
        "linux" if architecture == "arm64" => Some(format!(
            "ripgrep-{version}-aarch64-unknown-linux-gnu.tar.gz"
        )),
        "linux" => Some(format!(
            "ripgrep-{version}-x86_64-unknown-linux-musl.tar.gz"
        )),
        "win32" => Some(format!("ripgrep-{version}-{arch}-pc-windows-msvc.zip")),
        _ => None,
    }
}

fn rust_like_archive_arch(architecture: &str) -> &'static str {
    if architecture == "arm64" {
        "aarch64"
    } else {
        "x86_64"
    }
}

fn termux_package_name(tool: ManagedTool) -> &'static str {
    match tool {
        ManagedTool::Fd => "fd",
        ManagedTool::Rg => "ripgrep",
    }
}

fn binary_name(tool: ManagedTool) -> &'static str {
    match tool {
        ManagedTool::Fd => "fd",
        ManagedTool::Rg => "rg",
    }
}

fn binary_file_name(tool: ManagedTool, platform: &str) -> String {
    if platform == "win32" {
        format!("{}.exe", binary_name(tool))
    } else {
        binary_name(tool).to_string()
    }
}

fn archive_root_name(asset_name: &str) -> String {
    asset_name
        .strip_suffix(".tar.gz")
        .or_else(|| asset_name.strip_suffix(".zip"))
        .unwrap_or(asset_name)
        .to_string()
}

fn join_tool_path(left: &str, right: &str) -> String {
    format!(
        "{}/{}",
        left.trim_end_matches('/'),
        right.trim_start_matches('/')
    )
}

fn windows_tar_command(system_root: Option<&str>) -> String {
    system_root
        .map(|root| join_tool_path(&join_tool_path(root, "System32"), "tar.exe"))
        .unwrap_or_else(|| "tar.exe".to_string())
}

fn tool_display_name(tool: ManagedTool) -> &'static str {
    match tool {
        ManagedTool::Fd => "fd",
        ManagedTool::Rg => "ripgrep",
    }
}

fn system_binary_names(tool: ManagedTool) -> &'static [&'static str] {
    match tool {
        ManagedTool::Fd => &["fd", "fdfind"],
        ManagedTool::Rg => &["rg"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_fd_asset_names_like_pi_tools_manager() {
        assert_eq!(
            tool_asset_name(ManagedTool::Fd, "10.3.0", "darwin", "arm64"),
            Some("fd-v10.3.0-aarch64-apple-darwin.tar.gz".to_string())
        );
        assert_eq!(
            tool_asset_name(ManagedTool::Fd, "10.3.0", "linux", "x64"),
            Some("fd-v10.3.0-x86_64-unknown-linux-gnu.tar.gz".to_string())
        );
        assert_eq!(
            tool_asset_name(ManagedTool::Fd, "10.3.0", "win32", "x64"),
            Some("fd-v10.3.0-x86_64-pc-windows-msvc.zip".to_string())
        );
    }

    #[test]
    fn builds_ripgrep_asset_names_like_pi_tools_manager() {
        assert_eq!(
            tool_asset_name(ManagedTool::Rg, "14.1.1", "darwin", "x64"),
            Some("ripgrep-14.1.1-x86_64-apple-darwin.tar.gz".to_string())
        );
        assert_eq!(
            tool_asset_name(ManagedTool::Rg, "14.1.1", "linux", "arm64"),
            Some("ripgrep-14.1.1-aarch64-unknown-linux-gnu.tar.gz".to_string())
        );
        assert_eq!(
            tool_asset_name(ManagedTool::Rg, "14.1.1", "linux", "x64"),
            Some("ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz".to_string())
        );
        assert_eq!(
            tool_asset_name(ManagedTool::Rg, "14.1.1", "win32", "arm64"),
            Some("ripgrep-14.1.1-aarch64-pc-windows-msvc.zip".to_string())
        );
    }

    #[test]
    fn parses_offline_mode_like_pi_tools_manager() {
        assert!(!is_offline_mode_value_enabled(None));
        assert!(!is_offline_mode_value_enabled(Some("0")));
        assert!(is_offline_mode_value_enabled(Some("1")));
        assert!(is_offline_mode_value_enabled(Some("true")));
        assert!(is_offline_mode_value_enabled(Some("TRUE")));
        assert!(is_offline_mode_value_enabled(Some("yes")));
    }

    #[test]
    fn formats_termux_install_hints_like_pi_tools_manager() {
        assert_eq!(termux_install_hint(ManagedTool::Fd), "pkg install fd");
        assert_eq!(termux_install_hint(ManagedTool::Rg), "pkg install ripgrep");
    }

    #[test]
    fn ensure_tool_plan_prefers_local_then_system_like_pi_tools_manager() {
        assert_eq!(
            ensure_tool_plan(
                ManagedTool::Fd,
                EnsureToolContext {
                    platform: "darwin",
                    architecture: "arm64",
                    version: "10.3.0",
                    local_path: Some("/tmp/fd"),
                    available_system_commands: &["fd"],
                    offline_value: None,
                }
            ),
            EnsureToolPlan::UseLocal {
                path: "/tmp/fd".to_string()
            }
        );

        assert_eq!(
            ensure_tool_plan(
                ManagedTool::Fd,
                EnsureToolContext {
                    platform: "linux",
                    architecture: "x64",
                    version: "10.3.0",
                    local_path: None,
                    available_system_commands: &["fdfind"],
                    offline_value: None,
                }
            ),
            EnsureToolPlan::UseSystem {
                command: "fdfind".to_string()
            }
        );
    }

    #[test]
    fn ensure_tool_plan_handles_offline_android_and_download_like_pi_tools_manager() {
        assert_eq!(
            ensure_tool_plan(
                ManagedTool::Rg,
                EnsureToolContext {
                    platform: "linux",
                    architecture: "x64",
                    version: "14.1.1",
                    local_path: None,
                    available_system_commands: &[],
                    offline_value: Some("yes"),
                }
            ),
            EnsureToolPlan::SkipOffline {
                message: "ripgrep not found. Offline mode enabled, skipping download.".to_string()
            }
        );

        assert_eq!(
            ensure_tool_plan(
                ManagedTool::Rg,
                EnsureToolContext {
                    platform: "android",
                    architecture: "arm64",
                    version: "14.1.1",
                    local_path: None,
                    available_system_commands: &[],
                    offline_value: None,
                }
            ),
            EnsureToolPlan::TermuxInstallHint {
                message: "ripgrep not found. Install with: pkg install ripgrep".to_string()
            }
        );

        assert_eq!(
            ensure_tool_plan(
                ManagedTool::Rg,
                EnsureToolContext {
                    platform: "darwin",
                    architecture: "arm64",
                    version: "14.1.1",
                    local_path: None,
                    available_system_commands: &[],
                    offline_value: None,
                }
            ),
            EnsureToolPlan::Download {
                asset_name: "ripgrep-14.1.1-aarch64-apple-darwin.tar.gz".to_string()
            }
        );
    }

    #[test]
    fn plans_tool_install_layout_like_pi_tools_manager() {
        assert_eq!(
            tool_install_layout(
                ManagedTool::Rg,
                "/tmp/pi-bin",
                "ripgrep-14.1.1-aarch64-apple-darwin.tar.gz",
                "darwin",
            ),
            ToolInstallLayout {
                archive_path: "/tmp/pi-bin/ripgrep-14.1.1-aarch64-apple-darwin.tar.gz".to_string(),
                binary_path: "/tmp/pi-bin/rg".to_string(),
                extract_dir_prefix: "/tmp/pi-bin/extract_tmp_rg_".to_string(),
                extracted_binary_candidates: vec![
                    "/tmp/pi-bin/extract_tmp_rg_/ripgrep-14.1.1-aarch64-apple-darwin/rg"
                        .to_string(),
                    "/tmp/pi-bin/extract_tmp_rg_/rg".to_string(),
                ],
            }
        );

        assert_eq!(
            tool_install_layout(
                ManagedTool::Fd,
                "C:/pi/bin",
                "fd-v10.3.0-x86_64-pc-windows-msvc.zip",
                "win32",
            )
            .binary_path,
            "C:/pi/bin/fd.exe"
        );
    }

    #[test]
    fn plans_tar_gz_extraction_like_pi_tools_manager() {
        assert_eq!(
            extraction_command_plans(
                "/tmp/fd.tar.gz",
                "/tmp/extract",
                "fd-v10.3.0-aarch64-apple-darwin.tar.gz",
                "darwin",
                None,
            )
            .expect("tar.gz should be supported"),
            vec![ExtractionCommandPlan {
                command: "tar".to_string(),
                args: vec![
                    "xzf".to_string(),
                    "/tmp/fd.tar.gz".to_string(),
                    "-C".to_string(),
                    "/tmp/extract".to_string(),
                ],
            }]
        );
    }

    #[test]
    fn plans_zip_extraction_fallbacks_like_pi_tools_manager() {
        assert_eq!(
            extraction_command_plans("/tmp/rg.zip", "/tmp/extract", "rg.zip", "darwin", None)
                .expect("zip should be supported"),
            vec![
                ExtractionCommandPlan {
                    command: "unzip".to_string(),
                    args: vec![
                        "-q".to_string(),
                        "/tmp/rg.zip".to_string(),
                        "-d".to_string(),
                        "/tmp/extract".to_string(),
                    ],
                },
                ExtractionCommandPlan {
                    command: "tar".to_string(),
                    args: vec![
                        "xf".to_string(),
                        "/tmp/rg.zip".to_string(),
                        "-C".to_string(),
                        "/tmp/extract".to_string(),
                    ],
                },
            ]
        );
    }

    #[test]
    fn plans_windows_zip_extraction_like_pi_tools_manager() {
        let plans = extraction_command_plans(
            "C:/tmp/rg.zip",
            "C:/tmp/extract",
            "rg.zip",
            "win32",
            Some("C:/Windows"),
        )
        .expect("windows zip should be supported");

        assert_eq!(
            plans[0],
            ExtractionCommandPlan {
                command: "C:/Windows/System32/tar.exe".to_string(),
                args: vec![
                    "xf".to_string(),
                    "C:/tmp/rg.zip".to_string(),
                    "-C".to_string(),
                    "C:/tmp/extract".to_string(),
                ],
            }
        );
        assert_eq!(plans[1].command, "powershell.exe");
        assert!(plans[1].args.iter().any(|arg| arg.contains(
            "Expand-Archive -LiteralPath $archive -DestinationPath $destination -Force"
        )));
    }

    #[test]
    fn rejects_unsupported_archive_like_pi_tools_manager() {
        assert_eq!(
            extraction_command_plans("/tmp/tool.bin", "/tmp/extract", "tool.bin", "darwin", None),
            Err("Unsupported archive format: tool.bin".to_string())
        );
    }
}
