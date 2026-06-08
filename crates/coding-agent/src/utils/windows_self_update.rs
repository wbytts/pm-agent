use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const QUARANTINE_DIR_NAME: &str = ".pi-native-quarantine";

pub fn windows_self_update_quarantine_root(package_dir: impl AsRef<Path>) -> Option<PathBuf> {
    let mut current = absolute_path(package_dir.as_ref());
    loop {
        let is_node_modules = current
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("node_modules"));
        if is_node_modules {
            return Some(current.join(QUARANTINE_DIR_NAME));
        }
        let parent = current.parent()?;
        if parent == current {
            return None;
        }
        current = parent.to_path_buf();
    }
}

pub fn cleanup_windows_self_update_quarantine(package_dir: impl AsRef<Path>) {
    let Some(quarantine_root) = windows_self_update_quarantine_root(package_dir) else {
        return;
    };
    let _ = fs::remove_dir_all(quarantine_root);
}

pub fn loaded_files_in_package_dir(
    package_dir: impl AsRef<Path>,
    loaded_files: impl IntoIterator<Item = impl AsRef<Path>>,
) -> Vec<PathBuf> {
    let package_dir = absolute_path(package_dir.as_ref());
    let package_prefix = case_fold_path(&package_dir);
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for loaded_file in loaded_files {
        let path = absolute_path(loaded_file.as_ref());
        let comparison = case_fold_path(&path);
        if !comparison.starts_with(&package_prefix) || !seen.insert(comparison) {
            continue;
        }
        files.push(path);
    }
    files
}

pub fn quarantine_windows_native_dependencies(
    package_dir: impl AsRef<Path>,
    loaded_files: impl IntoIterator<Item = impl AsRef<Path>>,
    run_id: &str,
) -> std::io::Result<Vec<PathBuf>> {
    let package_dir = absolute_path(package_dir.as_ref());
    let Some(quarantine_root) = windows_self_update_quarantine_root(&package_dir) else {
        return Ok(Vec::new());
    };
    let loaded_files = loaded_files_in_package_dir(&package_dir, loaded_files);
    let quarantine_run_dir = quarantine_root.join(run_id);
    let mut quarantined = Vec::new();

    for loaded_file in loaded_files {
        if !loaded_file.exists() {
            continue;
        }
        let relative = loaded_file
            .strip_prefix(&package_dir)
            .unwrap_or(loaded_file.as_path());
        let quarantine_path = quarantine_run_dir.join(relative);
        if let Some(parent) = quarantine_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&loaded_file, &quarantine_path)?;
        fs::copy(&quarantine_path, &loaded_file)?;
        quarantined.push(quarantine_path);
    }

    Ok(quarantined)
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn case_fold_path(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn quarantine_root_uses_nearest_node_modules_like_pi() {
        let package_dir = temp_dir()
            .join("node_modules")
            .join("@scope")
            .join("pi")
            .join("node_modules")
            .join("nested")
            .join("pkg");

        assert_eq!(
            windows_self_update_quarantine_root(&package_dir),
            Some(
                package_dir
                    .join("..")
                    .join("..")
                    .join(".pi-native-quarantine")
            )
            .map(normalize_components)
        );
        assert_eq!(windows_self_update_quarantine_root(temp_dir()), None);
    }

    #[test]
    fn loaded_files_in_package_dir_keeps_unique_files_under_package_like_pi() {
        let package_dir = temp_dir().join("node_modules").join("pi");
        let inside = package_dir.join("native").join("addon.node");
        let outside = temp_dir().join("addon.node");

        let loaded = loaded_files_in_package_dir(
            &package_dir,
            [
                inside.clone(),
                inside.clone(),
                PathBuf::from(inside.to_string_lossy().to_ascii_uppercase()),
                outside,
                package_dir.join("other.node"),
            ],
        );

        assert_eq!(loaded, vec![inside, package_dir.join("other.node")]);
    }

    #[test]
    fn quarantine_moves_loaded_files_and_copies_them_back_like_pi() {
        let package_dir = temp_dir().join("node_modules").join("pi");
        let loaded_file = package_dir.join("native").join("addon.node");
        fs::create_dir_all(loaded_file.parent().expect("native parent")).expect("dir write");
        fs::write(&loaded_file, "binary").expect("loaded file write");

        let quarantined =
            quarantine_windows_native_dependencies(&package_dir, [&loaded_file], "run-1")
                .expect("quarantine should succeed");

        let expected_quarantine = package_dir
            .parent()
            .expect("node_modules")
            .join(".pi-native-quarantine")
            .join("run-1")
            .join("native")
            .join("addon.node");
        assert_eq!(quarantined, vec![expected_quarantine.clone()]);
        assert_eq!(
            fs::read_to_string(&expected_quarantine).expect("quarantine read"),
            "binary"
        );
        assert_eq!(
            fs::read_to_string(&loaded_file).expect("copy read"),
            "binary"
        );
    }

    #[test]
    fn cleanup_removes_quarantine_root_and_ignores_missing_root_like_pi() {
        let package_dir = temp_dir().join("node_modules").join("pi");
        let quarantine = package_dir
            .parent()
            .expect("node_modules")
            .join(".pi-native-quarantine");
        fs::create_dir_all(quarantine.join("old")).expect("quarantine write");

        cleanup_windows_self_update_quarantine(&package_dir);
        cleanup_windows_self_update_quarantine(&package_dir);

        assert!(!quarantine.exists());
    }

    fn normalize_components(path: PathBuf) -> PathBuf {
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                _ => normalized.push(component.as_os_str()),
            }
        }
        normalized
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("pm-agent-windows-self-update-test-{id}"))
    }
}
