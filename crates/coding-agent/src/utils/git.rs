#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSource {
    pub repo: String,
    pub host: String,
    pub path: String,
    pub reference: Option<String>,
    pub pinned: bool,
}

pub fn parse_git_url(source: &str) -> Option<GitSource> {
    let trimmed = source.trim();
    let has_git_prefix = trimmed.starts_with("git:");
    let url = if has_git_prefix {
        trimmed[4..].trim()
    } else {
        trimmed
    };

    if !has_git_prefix && !has_explicit_git_protocol(url) {
        return None;
    }

    parse_generic_git_url(url)
}

fn parse_generic_git_url(url: &str) -> Option<GitSource> {
    let (repo_without_ref, reference) = split_ref(url);
    let mut repo = repo_without_ref.clone();
    let (host, path) = if let Some((host, path)) = parse_scp_like(&repo_without_ref) {
        (host, path)
    } else if let Some((host, path, clone_repo)) = parse_hosted_shorthand(&repo_without_ref) {
        repo = clone_repo;
        (host, path)
    } else if has_explicit_git_protocol(&repo_without_ref) {
        parse_protocol_url(&repo_without_ref)?
    } else {
        let slash_index = repo_without_ref.find('/')?;
        let host = repo_without_ref[..slash_index].to_string();
        let path = repo_without_ref[slash_index + 1..].to_string();
        if !host.contains('.') && host != "localhost" {
            return None;
        }
        repo = format!("https://{repo_without_ref}");
        (host, path)
    };

    let normalized_path = path
        .trim_start_matches('/')
        .trim_end_matches(".git")
        .to_string();
    if host.is_empty() || normalized_path.is_empty() || normalized_path.split('/').count() < 2 {
        return None;
    }

    Some(GitSource {
        repo,
        host,
        path: normalized_path,
        pinned: reference.is_some(),
        reference,
    })
}

fn split_ref(url: &str) -> (String, Option<String>) {
    if let Some((host, path)) = parse_scp_like(url) {
        if let Some(index) = path.find('@') {
            let repo_path = &path[..index];
            let reference = &path[index + 1..];
            if !repo_path.is_empty() && !reference.is_empty() {
                return (
                    format!("git@{host}:{repo_path}"),
                    Some(reference.to_string()),
                );
            }
        }
        return (url.to_string(), None);
    }

    if let Some((prefix, rest)) = url.split_once("://") {
        let Some(path_start) = rest.find('/') else {
            return (url.to_string(), None);
        };
        let authority = &rest[..path_start];
        let path = &rest[path_start + 1..];
        if let Some(index) = path.find('#') {
            let repo_path = &path[..index];
            let reference = &path[index + 1..];
            if !repo_path.is_empty() && !reference.is_empty() {
                return (
                    format!("{prefix}://{authority}/{repo_path}"),
                    Some(reference.to_string()),
                );
            }
        }
        if let Some(index) = path.find('@') {
            let repo_path = &path[..index];
            let reference = &path[index + 1..];
            if !repo_path.is_empty() && !reference.is_empty() {
                return (
                    format!("{prefix}://{authority}/{repo_path}"),
                    Some(reference.to_string()),
                );
            }
        }
        return (url.to_string(), None);
    }

    let Some(slash_index) = url.find('/') else {
        return (url.to_string(), None);
    };
    let host = &url[..slash_index];
    let path = &url[slash_index + 1..];
    if let Some(index) = path.find('@') {
        let repo_path = &path[..index];
        let reference = &path[index + 1..];
        if !repo_path.is_empty() && !reference.is_empty() {
            return (format!("{host}/{repo_path}"), Some(reference.to_string()));
        }
    }

    (url.to_string(), None)
}

fn parse_scp_like(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("git@")?;
    let (host, path) = rest.split_once(':')?;
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some((host.to_string(), path.to_string()))
}

fn parse_hosted_shorthand(url: &str) -> Option<(String, String, String)> {
    let (service, path) = url.split_once(':')?;
    let host = match service {
        "github" => "github.com",
        "gitlab" => "gitlab.com",
        "bitbucket" => "bitbucket.org",
        _ => return None,
    };
    if path.split('/').count() < 2 {
        return None;
    }
    Some((
        host.to_string(),
        path.to_string(),
        format!("https://{host}/{path}"),
    ))
}

fn parse_protocol_url(url: &str) -> Option<(String, String)> {
    let (_, rest) = url.split_once("://")?;
    let path_start = rest.find('/')?;
    let authority = &rest[..path_start];
    let host = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority)
        .to_string();
    let path = rest[path_start + 1..].to_string();
    Some((host, path))
}

fn has_explicit_git_protocol(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ssh://")
        || lower.starts_with("git://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_https_git_url() {
        let source = parse_git_url("https://github.com/user/repo.git").expect("git source");
        assert_eq!(source.repo, "https://github.com/user/repo.git");
        assert_eq!(source.host, "github.com");
        assert_eq!(source.path, "user/repo");
        assert_eq!(source.reference, None);
        assert!(!source.pinned);
    }

    #[test]
    fn parses_explicit_https_git_url_with_hash_ref_like_pi() {
        let source = parse_git_url("https://github.com/user/repo#main").expect("git source");
        assert_eq!(source.repo, "https://github.com/user/repo");
        assert_eq!(source.host, "github.com");
        assert_eq!(source.path, "user/repo");
        assert_eq!(source.reference.as_deref(), Some("main"));
        assert!(source.pinned);
    }

    #[test]
    fn parses_git_prefix_shorthand_with_ref() {
        let source = parse_git_url("git:github.com/user/repo@main").expect("git source");
        assert_eq!(source.repo, "https://github.com/user/repo");
        assert_eq!(source.host, "github.com");
        assert_eq!(source.path, "user/repo");
        assert_eq!(source.reference.as_deref(), Some("main"));
        assert!(source.pinned);
    }

    #[test]
    fn parses_github_hosted_shorthand_with_git_prefix_like_pi() {
        let source = parse_git_url("git:github:user/repo@v1").expect("git source");
        assert_eq!(source.repo, "https://github.com/user/repo");
        assert_eq!(source.host, "github.com");
        assert_eq!(source.path, "user/repo");
        assert_eq!(source.reference.as_deref(), Some("v1"));
        assert!(source.pinned);
    }

    #[test]
    fn parses_gitlab_hosted_shorthand_with_git_prefix_like_pi() {
        let source = parse_git_url("git:gitlab:user/repo@v1").expect("git source");
        assert_eq!(source.repo, "https://gitlab.com/user/repo");
        assert_eq!(source.host, "gitlab.com");
        assert_eq!(source.path, "user/repo");
        assert_eq!(source.reference.as_deref(), Some("v1"));
        assert!(source.pinned);
    }

    #[test]
    fn parses_bitbucket_hosted_shorthand_with_git_prefix_like_pi() {
        let source = parse_git_url("git:bitbucket:user/repo").expect("git source");
        assert_eq!(source.repo, "https://bitbucket.org/user/repo");
        assert_eq!(source.host, "bitbucket.org");
        assert_eq!(source.path, "user/repo");
        assert_eq!(source.reference, None);
        assert!(!source.pinned);
    }

    #[test]
    fn parses_codeberg_https_git_url_like_pi() {
        let source = parse_git_url("https://codeberg.org/user/repo").expect("git source");
        assert_eq!(source.repo, "https://codeberg.org/user/repo");
        assert_eq!(source.host, "codeberg.org");
        assert_eq!(source.path, "user/repo");
        assert_eq!(source.reference, None);
        assert!(!source.pinned);
    }

    #[test]
    fn parses_scp_like_url() {
        let source = parse_git_url("git:git@github.com:user/repo@v1").expect("git source");
        assert_eq!(source.repo, "git@github.com:user/repo");
        assert_eq!(source.host, "github.com");
        assert_eq!(source.path, "user/repo");
        assert_eq!(source.reference.as_deref(), Some("v1"));
    }

    #[test]
    fn rejects_bare_names_without_git_prefix() {
        assert!(parse_git_url("github.com/user/repo").is_none());
        assert!(parse_git_url("git@github.com:user/repo").is_none());
    }
}
