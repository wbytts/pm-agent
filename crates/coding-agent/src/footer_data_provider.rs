use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::exec::{exec_command, ExecOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPaths {
    pub repo_dir: PathBuf,
    pub common_git_dir: PathBuf,
    pub head_path: PathBuf,
}

pub struct FooterDataProvider {
    cwd: PathBuf,
    extension_statuses: BTreeMap<String, String>,
    cached_branch: Option<Option<String>>,
    git_paths: Option<GitPaths>,
    branch_change_callbacks: Vec<Box<dyn Fn() + Send + Sync>>,
    available_provider_count: usize,
    disposed: bool,
}

impl FooterDataProvider {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        let git_paths = find_git_paths(&cwd);
        Self {
            cwd,
            extension_statuses: BTreeMap::new(),
            cached_branch: None,
            git_paths,
            branch_change_callbacks: Vec::new(),
            available_provider_count: 0,
            disposed: false,
        }
    }

    pub fn git_branch(&mut self) -> Option<String> {
        if self.cached_branch.is_none() {
            self.cached_branch = Some(self.resolve_git_branch());
        }
        self.cached_branch.clone().flatten()
    }

    pub fn extension_statuses(&self) -> &BTreeMap<String, String> {
        &self.extension_statuses
    }

    pub fn set_extension_status(&mut self, key: impl Into<String>, text: Option<String>) {
        let key = key.into();
        if let Some(text) = text {
            self.extension_statuses.insert(key, text);
        } else {
            self.extension_statuses.remove(&key);
        }
    }

    pub fn clear_extension_statuses(&mut self) {
        self.extension_statuses.clear();
    }

    pub fn available_provider_count(&self) -> usize {
        self.available_provider_count
    }

    pub fn set_available_provider_count(&mut self, count: usize) {
        self.available_provider_count = count;
    }

    pub fn on_branch_change<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.branch_change_callbacks.push(Box::new(callback));
    }

    pub fn set_cwd(&mut self, cwd: impl Into<PathBuf>) {
        let cwd = cwd.into();
        if self.cwd == cwd {
            return;
        }

        self.cwd = cwd;
        self.cached_branch = None;
        self.git_paths = find_git_paths(&self.cwd);
        self.notify_branch_change();
    }

    pub fn refresh_git_branch(&mut self) {
        if self.disposed {
            return;
        }

        let next_branch = self.resolve_git_branch();
        if let Some(cached_branch) = &self.cached_branch {
            if cached_branch != &next_branch {
                self.cached_branch = Some(next_branch);
                self.notify_branch_change();
                return;
            }
        }
        self.cached_branch = Some(next_branch);
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
        self.branch_change_callbacks.clear();
    }

    fn notify_branch_change(&self) {
        if self.disposed {
            return;
        }
        for callback in &self.branch_change_callbacks {
            callback();
        }
    }

    fn resolve_git_branch(&self) -> Option<String> {
        let git_paths = self.git_paths.as_ref()?;
        let content = fs::read_to_string(&git_paths.head_path).ok()?;
        let content = content.trim();
        if let Some(branch) = content.strip_prefix("ref: refs/heads/") {
            if branch == ".invalid" {
                return resolve_branch_with_git(&git_paths.repo_dir)
                    .or_else(|| Some("detached".to_string()));
            }
            return Some(branch.to_string());
        }
        Some("detached".to_string())
    }
}

pub fn find_git_paths(cwd: impl AsRef<Path>) -> Option<GitPaths> {
    let mut dir = cwd.as_ref().to_path_buf();
    loop {
        let git_path = dir.join(".git");
        if git_path.exists() {
            if git_path.is_file() {
                if let Some(paths) = read_worktree_git_paths(&dir, &git_path) {
                    return Some(paths);
                }
            } else if git_path.is_dir() {
                let head_path = git_path.join("HEAD");
                if head_path.exists() {
                    return Some(GitPaths {
                        repo_dir: dir,
                        common_git_dir: git_path,
                        head_path,
                    });
                }
            }
            return None;
        }

        if !dir.pop() {
            return None;
        }
    }
}

fn read_worktree_git_paths(repo_dir: &Path, git_file: &Path) -> Option<GitPaths> {
    let content = fs::read_to_string(git_file).ok()?;
    let git_dir_raw = content.trim().strip_prefix("gitdir: ")?.trim();
    let git_dir = absolutize(repo_dir, git_dir_raw);
    let head_path = git_dir.join("HEAD");
    if !head_path.exists() {
        return None;
    }

    let common_dir_path = git_dir.join("commondir");
    let common_git_dir = if common_dir_path.exists() {
        let common_dir = fs::read_to_string(common_dir_path).ok()?;
        absolutize(&git_dir, common_dir.trim())
    } else {
        git_dir.clone()
    };

    Some(GitPaths {
        repo_dir: repo_dir.to_path_buf(),
        common_git_dir,
        head_path,
    })
}

fn resolve_branch_with_git(repo_dir: &Path) -> Option<String> {
    let result = exec_command(
        "git",
        &[
            "--no-optional-locks".to_string(),
            "symbolic-ref".to_string(),
            "--quiet".to_string(),
            "--short".to_string(),
            "HEAD".to_string(),
        ],
        &repo_dir.to_string_lossy(),
        Some(ExecOptions {
            timeout_ms: Some(5_000),
            cwd: Some(repo_dir.to_string_lossy().to_string()),
            ..ExecOptions::default()
        }),
    )
    .ok()?;

    (result.code == 0)
        .then(|| result.stdout.trim().to_string())
        .filter(|branch| !branch.is_empty())
}

fn absolutize(base: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        base.join(path)
    };
    fs::canonicalize(&path).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_branch_from_regular_git_head() {
        let repo = test_dir("regular");
        let git = repo.join(".git");
        fs::create_dir_all(&git).expect("git dir");
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n").expect("head");

        let mut provider = FooterDataProvider::new(&repo);
        assert_eq!(provider.git_branch().as_deref(), Some("main"));
        fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn resolves_detached_head() {
        let repo = test_dir("detached");
        let git = repo.join(".git");
        fs::create_dir_all(&git).expect("git dir");
        fs::write(git.join("HEAD"), "0123456789abcdef\n").expect("head");

        let mut provider = FooterDataProvider::new(&repo);
        assert_eq!(provider.git_branch().as_deref(), Some("detached"));
        fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn resolves_worktree_git_file_and_common_dir() {
        let root = test_dir("worktree");
        let repo = root.join("repo");
        let git_dir = root.join("actual-git");
        let common_dir = root.join("common-git");
        fs::create_dir_all(&repo).expect("repo");
        fs::create_dir_all(&git_dir).expect("git dir");
        fs::create_dir_all(&common_dir).expect("common dir");
        fs::write(repo.join(".git"), "gitdir: ../actual-git\n").expect("git file");
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/dev\n").expect("head");
        fs::write(git_dir.join("commondir"), "../common-git\n").expect("commondir");

        let paths = find_git_paths(&repo).expect("paths");
        assert_eq!(paths.repo_dir, repo);
        assert_eq!(
            paths.common_git_dir,
            fs::canonicalize(&common_dir).expect("canonical common dir")
        );
        assert_eq!(
            paths.head_path,
            fs::canonicalize(git_dir.join("HEAD")).expect("canonical head")
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn manages_statuses_provider_count_and_callbacks() {
        let repo = test_dir("state");
        let mut provider = FooterDataProvider::new(&repo);
        provider.set_extension_status("ext", Some("ready".to_string()));
        assert_eq!(
            provider.extension_statuses().get("ext").map(String::as_str),
            Some("ready")
        );
        provider.set_extension_status("ext", None);
        assert!(provider.extension_statuses().is_empty());
        provider.set_available_provider_count(3);
        assert_eq!(provider.available_provider_count(), 3);

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_callback = calls.clone();
        provider.on_branch_change(move || {
            calls_for_callback.fetch_add(1, Ordering::SeqCst);
        });
        provider.set_cwd(repo.join("nested"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        provider.dispose();
        provider.set_cwd(repo.join("other"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn refresh_git_branch_notifies_only_on_change() {
        let repo = test_dir("refresh");
        let git = repo.join(".git");
        let head = git.join("HEAD");
        fs::create_dir_all(&git).expect("git dir");
        fs::write(&head, "ref: refs/heads/main\n").expect("head");
        let mut provider = FooterDataProvider::new(&repo);
        assert_eq!(provider.git_branch().as_deref(), Some("main"));

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_callback = calls.clone();
        provider.on_branch_change(move || {
            calls_for_callback.fetch_add(1, Ordering::SeqCst);
        });

        provider.refresh_git_branch();
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        fs::write(&head, "ref: refs/heads/dev\n").expect("head changed");
        provider.refresh_git_branch();
        assert_eq!(provider.git_branch().as_deref(), Some("dev"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        fs::remove_dir_all(repo).ok();
    }

    fn test_dir(name: &str) -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let path = std::env::temp_dir().join(format!("pm-agent-footer-{name}-{id}"));
        fs::create_dir_all(&path).expect("test dir");
        path
    }
}
