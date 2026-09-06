use super::types::{LocalSource, NpmSource, ParsedSource, SourceScope};
use crate::settings_manager::CONFIG_DIR_NAME;
use crate::utils::git::{parse_git_url, GitSource};
use crate::utils::paths::is_local_path;
use std::path::{Path, PathBuf};

pub fn parse_source(source: &str) -> ParsedSource {
    if let Some(spec) = source.strip_prefix("npm:") {
        let spec = spec.trim().to_string();
        let (name, version) = parse_npm_spec(&spec);
        return ParsedSource::Npm(NpmSource {
            spec,
            name,
            pinned: version.is_some(),
            version,
        });
    }

    if is_local_path(source) {
        return ParsedSource::Local(LocalSource {
            path: source.to_string(),
        });
    }

    if let Some(source) = parse_git_url(source) {
        return ParsedSource::Git(source);
    }

    ParsedSource::Local(LocalSource {
        path: source.to_string(),
    })
}

pub fn managed_npm_install_path(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &NpmSource,
    scope: SourceScope,
) -> PathBuf {
    let root = match scope {
        SourceScope::Temporary => temp_package_dir("npm"),
        SourceScope::Project => cwd.as_ref().join(CONFIG_DIR_NAME).join("npm"),
        SourceScope::User => agent_dir.as_ref().join("npm"),
    };
    root.join("node_modules").join(&source.name)
}

pub fn git_install_path(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &GitSource,
    scope: SourceScope,
) -> PathBuf {
    if scope == SourceScope::Temporary {
        return temp_package_dir_with_suffix(&format!("git-{}", source.host), Some(&source.path));
    }
    git_install_root(agent_dir, cwd, source, scope).join(&source.path)
}

pub fn git_install_root(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &GitSource,
    scope: SourceScope,
) -> PathBuf {
    match scope {
        SourceScope::Temporary => temp_package_dir(&format!("git-{}", source.host)),
        SourceScope::Project => cwd
            .as_ref()
            .join(CONFIG_DIR_NAME)
            .join("git")
            .join(&source.host),
        SourceScope::User => agent_dir.as_ref().join("git").join(&source.host),
    }
}

pub fn git_storage_root(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    scope: SourceScope,
) -> Option<PathBuf> {
    match scope {
        SourceScope::Temporary => None,
        SourceScope::Project => Some(cwd.as_ref().join(CONFIG_DIR_NAME).join("git")),
        SourceScope::User => Some(agent_dir.as_ref().join("git")),
    }
}

pub fn installed_path_for_source(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    scope: SourceScope,
) -> Option<PathBuf> {
    installed_path_for_source_with_npm_fallback(agent_dir, cwd, source, scope, |_| None)
}

pub fn installed_path_for_source_with_npm_fallback<F>(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    scope: SourceScope,
    mut legacy_npm_path: F,
) -> Option<PathBuf>
where
    F: FnMut(&NpmSource) -> Option<PathBuf>,
{
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    match parse_source(source) {
        ParsedSource::Npm(source) => {
            let managed_path = managed_npm_install_path(agent_dir, cwd, &source, scope);
            if scope != SourceScope::User || managed_path.exists() {
                return managed_path.exists().then_some(managed_path);
            }
            legacy_npm_path(&source).filter(|path| path.exists())
        }
        ParsedSource::Git(source) => {
            let path = git_install_path(agent_dir, cwd, &source, scope);
            path.exists().then_some(path)
        }
        ParsedSource::Local(source) => {
            let path = super::paths::resolve_package_local_path(
                &source.path,
                package_source_base_dir(agent_dir, cwd, scope),
            );
            path.exists().then_some(path)
        }
    }
}

pub fn source_identity(source: &str) -> String {
    match parse_source(source) {
        ParsedSource::Npm(source) => format!("npm:{}", source.name),
        ParsedSource::Git(source) => format!("git:{}/{}", source.host, source.path),
        ParsedSource::Local(source) => format!("local:{}", source.path),
    }
}

pub fn source_identity_from_base(source: &str, base_dir: impl AsRef<Path>) -> String {
    match parse_source(source) {
        ParsedSource::Npm(source) => format!("npm:{}", source.name),
        ParsedSource::Git(source) => format!("git:{}/{}", source.host, source.path),
        ParsedSource::Local(source) => {
            let resolved = super::paths::resolve_package_local_path(&source.path, base_dir);
            format!("local:{}", display_path(&resolved))
        }
    }
}

pub fn scoped_source_identity(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    source: &str,
    scope: SourceScope,
) -> String {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    source_identity_from_base(source, package_source_base_dir(agent_dir, cwd, scope))
}

pub fn package_source_base_dir(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    scope: SourceScope,
) -> PathBuf {
    match scope {
        SourceScope::Project => cwd.as_ref().join(CONFIG_DIR_NAME),
        SourceScope::User => agent_dir.as_ref().to_path_buf(),
        SourceScope::Temporary => cwd.as_ref().to_path_buf(),
    }
}

fn parse_npm_spec(spec: &str) -> (String, Option<String>) {
    let version_start = if spec.starts_with('@') {
        spec.get(1..)
            .and_then(|rest| rest.find('@').map(|index| index + 1))
    } else {
        spec.find('@')
    };
    let Some(version_start) = version_start else {
        return (spec.to_string(), None);
    };
    if version_start == 0 {
        return (spec.to_string(), None);
    }
    let name = &spec[..version_start];
    let version = &spec[version_start + 1..];
    if version.is_empty() {
        return (spec.to_string(), None);
    }
    (name.to_string(), Some(version.to_string()))
}

fn display_path(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn temp_package_dir(prefix: &str) -> PathBuf {
    temp_package_dir_with_suffix(prefix, None)
}

fn temp_package_dir_with_suffix(prefix: &str, suffix: Option<&str>) -> PathBuf {
    let suffix = suffix.unwrap_or_default();
    let hash = sha256_hex_prefix(format!("{prefix}-{suffix}").as_bytes(), 8);
    std::env::temp_dir()
        .join("pi-extensions")
        .join(prefix)
        .join(hash)
        .join(suffix)
}

fn sha256_hex_prefix(input: &[u8], hex_len: usize) -> String {
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut message = input.to_vec();
    let bit_len = (message.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    let mut h = H0;
    for chunk in message.chunks_exact(64) {
        let mut w = [0_u32; 64];
        for index in 0..16 {
            w[index] = u32::from_be_bytes([
                chunk[index * 4],
                chunk[index * 4 + 1],
                chunk[index * 4 + 2],
                chunk[index * 4 + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut output = String::with_capacity(hex_len);
    for word in h {
        for byte in word.to_be_bytes() {
            if output.len() >= hex_len {
                return output;
            }
            output.push(hex_digit(byte >> 4));
            if output.len() >= hex_len {
                return output;
            }
            output.push(hex_digit(byte & 0x0f));
        }
    }
    output
}

fn hex_digit(value: u8) -> char {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    HEX[value as usize] as char
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_npm_sources_with_pin_detection() {
        match parse_source("npm:@scope/pkg@1.2.3") {
            ParsedSource::Npm(source) => {
                assert_eq!(source.name, "@scope/pkg");
                assert!(source.pinned);
                assert_eq!(source.version.as_deref(), Some("1.2.3"));
            }
            other => panic!("expected npm source, got {other:?}"),
        }
        match parse_source("npm:plain-pkg") {
            ParsedSource::Npm(source) => {
                assert_eq!(source.name, "plain-pkg");
                assert!(!source.pinned);
                assert_eq!(source.version, None);
            }
            other => panic!("expected npm source, got {other:?}"),
        }
    }

    #[test]
    fn parses_npm_alias_specs_like_pi() {
        match parse_source("npm:alias@npm:real@1.0.0") {
            ParsedSource::Npm(source) => {
                assert_eq!(source.name, "alias");
                assert!(source.pinned);
                assert_eq!(source.version.as_deref(), Some("npm:real@1.0.0"));
            }
            other => panic!("expected npm source, got {other:?}"),
        }
    }

    #[test]
    fn parses_git_and_local_sources_like_pi() {
        assert!(matches!(
            parse_source("https://github.com/user/repo.git"),
            ParsedSource::Git(_)
        ));
        assert!(matches!(
            parse_source("git:git@github.com:user/repo@main"),
            ParsedSource::Git(_)
        ));
        assert!(matches!(parse_source("./local"), ParsedSource::Local(_)));
        assert!(matches!(parse_source("bare-name"), ParsedSource::Local(_)));
        assert!(matches!(
            parse_source("git@github.com:user/repo@main"),
            ParsedSource::Local(_)
        ));
    }

    #[test]
    fn local_source_path_preserves_original_whitespace_like_pi() {
        match parse_source("./local ") {
            ParsedSource::Local(source) => assert_eq!(source.path, "./local "),
            other => panic!("expected local source, got {other:?}"),
        }
        match parse_source(" bare-name ") {
            ParsedSource::Local(source) => assert_eq!(source.path, " bare-name "),
            other => panic!("expected local source, got {other:?}"),
        }
    }

    #[test]
    fn git_source_identity_dedupes_supported_url_formats_like_pi() {
        let urls = [
            "https://github.com/user/repo",
            "https://github.com/user/repo.git",
            "ssh://git@github.com/user/repo",
            "git:https://github.com/user/repo",
            "git:github.com/user/repo",
            "git:github:user/repo",
            "git:git@github.com:user/repo",
            "git:git@github.com:user/repo.git",
        ];

        let identities = urls.map(source_identity);

        assert_eq!(identities, ["git:github.com/user/repo"; 8]);
    }

    #[test]
    fn builds_managed_install_paths_by_scope() {
        let source = match parse_source("npm:@scope/pkg") {
            ParsedSource::Npm(source) => source,
            other => panic!("expected npm source, got {other:?}"),
        };
        assert_eq!(
            managed_npm_install_path("/agent", "/work", &source, SourceScope::Project),
            PathBuf::from("/work/.pm-agent/npm/node_modules/@scope/pkg")
        );

        let git = match parse_source("https://github.com/user/repo.git") {
            ParsedSource::Git(source) => source,
            other => panic!("expected git source, got {other:?}"),
        };
        assert_eq!(
            git_install_path("/agent", "/work", &git, SourceScope::User),
            PathBuf::from("/agent/git/github.com/user/repo")
        );
    }

    #[test]
    fn temporary_install_paths_use_pi_extensions_hashed_layout_like_pi() {
        let npm = match parse_source("npm:pkg") {
            ParsedSource::Npm(source) => source,
            other => panic!("expected npm source, got {other:?}"),
        };
        assert!(
            managed_npm_install_path("/agent", "/work", &npm, SourceScope::Temporary)
                .ends_with(Path::new("pi-extensions/npm/f35b2129/node_modules/pkg"))
        );

        let git = match parse_source("git:https://github.com/user/repo") {
            ParsedSource::Git(source) => source,
            other => panic!("expected git source, got {other:?}"),
        };
        assert!(
            git_install_path("/agent", "/work", &git, SourceScope::Temporary)
                .ends_with(Path::new("pi-extensions/git-github.com/338a1076/user/repo"))
        );
    }

    #[test]
    fn user_npm_installed_path_falls_back_to_legacy_global_path_like_pi() {
        let agent_dir = temp_dir().join("agent");
        let cwd = temp_dir().join("work");
        let legacy_path = temp_dir().join("global").join("node_modules").join("pkg");
        std::fs::create_dir_all(&legacy_path).expect("legacy package path should be created");

        let user_path = installed_path_for_source_with_npm_fallback(
            &agent_dir,
            &cwd,
            "npm:pkg",
            SourceScope::User,
            |_| Some(legacy_path.clone()),
        );
        assert_eq!(user_path.as_deref(), Some(legacy_path.as_path()));

        let project_path = installed_path_for_source_with_npm_fallback(
            &agent_dir,
            &cwd,
            "npm:pkg",
            SourceScope::Project,
            |_| Some(legacy_path.clone()),
        );
        assert_eq!(project_path, None);
    }

    fn temp_dir() -> PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-package-source-test-{id}"));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
