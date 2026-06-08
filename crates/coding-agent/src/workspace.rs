use std::path::PathBuf;

use crate::types::{CodingAgentError, CodingAgentResult, CodingWorkspace};
use crate::utils::paths::{resolve_path, PathInputOptions};

pub fn validate_workspace(workspace: &CodingWorkspace) -> CodingAgentResult<()> {
    if workspace.cwd.exists() {
        Ok(())
    } else {
        Err(CodingAgentError::MissingWorkspace(
            workspace.cwd.to_string_lossy().to_string(),
        ))
    }
}

pub(crate) fn resolve_workspace_path(
    workspace: &CodingWorkspace,
    path: &str,
) -> CodingAgentResult<PathBuf> {
    let relative = PathBuf::from(path);
    if relative.is_absolute() || path.split('/').any(|part| part == "..") {
        return Err(CodingAgentError::UnsafePath(path.to_string()));
    }

    let cwd = workspace.cwd.canonicalize().map_err(|error| {
        CodingAgentError::MissingWorkspace(format!("{}：{error}", workspace.cwd.display()))
    })?;
    let resolved = cwd.join(relative);
    if !resolved.starts_with(&cwd) {
        return Err(CodingAgentError::UnsafePath(path.to_string()));
    }
    Ok(resolved)
}

pub(crate) fn resolve_read_workspace_path(
    workspace: &CodingWorkspace,
    path: &str,
) -> CodingAgentResult<PathBuf> {
    let cwd = workspace.cwd.canonicalize().map_err(|error| {
        CodingAgentError::MissingWorkspace(format!("{}：{error}", workspace.cwd.display()))
    })?;
    let options = PathInputOptions {
        trim: true,
        strip_at_prefix: true,
        normalize_unicode_spaces: true,
        ..PathInputOptions::default()
    };
    let resolved = ensure_inside_workspace(&cwd, resolve_path(path, &cwd, Some(&options)), path)?;
    if resolved.exists() {
        return Ok(resolved);
    }

    for candidate in read_path_variants(&resolved) {
        if candidate.exists() {
            return ensure_inside_workspace(&cwd, candidate, path);
        }
    }
    Ok(resolved)
}

fn ensure_inside_workspace(
    cwd: &std::path::Path,
    path: PathBuf,
    input: &str,
) -> CodingAgentResult<PathBuf> {
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(CodingAgentError::UnsafePath(input.to_string()));
    }
    if path.is_absolute() && !path.starts_with(cwd) {
        return Err(CodingAgentError::UnsafePath(input.to_string()));
    }
    Ok(path)
}

fn read_path_variants(path: &std::path::Path) -> Vec<PathBuf> {
    let value = path.to_string_lossy();
    let am_pm = value
        .replace(" AM.", "\u{202f}AM.")
        .replace(" PM.", "\u{202f}PM.")
        .replace(" am.", "\u{202f}am.")
        .replace(" pm.", "\u{202f}pm.");
    let nfd = decompose_latin1(&value);
    let curly = value.replace('\'', "\u{2019}");
    let nfd_curly = decompose_latin1(&curly);
    [am_pm, nfd, curly, nfd_curly]
        .into_iter()
        .filter(|candidate| candidate != &*value)
        .map(PathBuf::from)
        .collect()
}

fn decompose_latin1(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            'À' => "A\u{0300}".chars().collect::<Vec<_>>(),
            'Á' => "A\u{0301}".chars().collect::<Vec<_>>(),
            'Â' => "A\u{0302}".chars().collect::<Vec<_>>(),
            'Ã' => "A\u{0303}".chars().collect::<Vec<_>>(),
            'Ä' => "A\u{0308}".chars().collect::<Vec<_>>(),
            'Å' => "A\u{030a}".chars().collect::<Vec<_>>(),
            'Ç' => "C\u{0327}".chars().collect::<Vec<_>>(),
            'È' => "E\u{0300}".chars().collect::<Vec<_>>(),
            'É' => "E\u{0301}".chars().collect::<Vec<_>>(),
            'Ê' => "E\u{0302}".chars().collect::<Vec<_>>(),
            'Ë' => "E\u{0308}".chars().collect::<Vec<_>>(),
            'Ì' => "I\u{0300}".chars().collect::<Vec<_>>(),
            'Í' => "I\u{0301}".chars().collect::<Vec<_>>(),
            'Î' => "I\u{0302}".chars().collect::<Vec<_>>(),
            'Ï' => "I\u{0308}".chars().collect::<Vec<_>>(),
            'Ñ' => "N\u{0303}".chars().collect::<Vec<_>>(),
            'Ò' => "O\u{0300}".chars().collect::<Vec<_>>(),
            'Ó' => "O\u{0301}".chars().collect::<Vec<_>>(),
            'Ô' => "O\u{0302}".chars().collect::<Vec<_>>(),
            'Õ' => "O\u{0303}".chars().collect::<Vec<_>>(),
            'Ö' => "O\u{0308}".chars().collect::<Vec<_>>(),
            'Ù' => "U\u{0300}".chars().collect::<Vec<_>>(),
            'Ú' => "U\u{0301}".chars().collect::<Vec<_>>(),
            'Û' => "U\u{0302}".chars().collect::<Vec<_>>(),
            'Ü' => "U\u{0308}".chars().collect::<Vec<_>>(),
            'Ý' => "Y\u{0301}".chars().collect::<Vec<_>>(),
            'à' => "a\u{0300}".chars().collect::<Vec<_>>(),
            'á' => "a\u{0301}".chars().collect::<Vec<_>>(),
            'â' => "a\u{0302}".chars().collect::<Vec<_>>(),
            'ã' => "a\u{0303}".chars().collect::<Vec<_>>(),
            'ä' => "a\u{0308}".chars().collect::<Vec<_>>(),
            'å' => "a\u{030a}".chars().collect::<Vec<_>>(),
            'ç' => "c\u{0327}".chars().collect::<Vec<_>>(),
            'è' => "e\u{0300}".chars().collect::<Vec<_>>(),
            'é' => "e\u{0301}".chars().collect::<Vec<_>>(),
            'ê' => "e\u{0302}".chars().collect::<Vec<_>>(),
            'ë' => "e\u{0308}".chars().collect::<Vec<_>>(),
            'ì' => "i\u{0300}".chars().collect::<Vec<_>>(),
            'í' => "i\u{0301}".chars().collect::<Vec<_>>(),
            'î' => "i\u{0302}".chars().collect::<Vec<_>>(),
            'ï' => "i\u{0308}".chars().collect::<Vec<_>>(),
            'ñ' => "n\u{0303}".chars().collect::<Vec<_>>(),
            'ò' => "o\u{0300}".chars().collect::<Vec<_>>(),
            'ó' => "o\u{0301}".chars().collect::<Vec<_>>(),
            'ô' => "o\u{0302}".chars().collect::<Vec<_>>(),
            'õ' => "o\u{0303}".chars().collect::<Vec<_>>(),
            'ö' => "o\u{0308}".chars().collect::<Vec<_>>(),
            'ù' => "u\u{0300}".chars().collect::<Vec<_>>(),
            'ú' => "u\u{0301}".chars().collect::<Vec<_>>(),
            'û' => "u\u{0302}".chars().collect::<Vec<_>>(),
            'ü' => "u\u{0308}".chars().collect::<Vec<_>>(),
            'ý' => "y\u{0301}".chars().collect::<Vec<_>>(),
            'ÿ' => "y\u{0308}".chars().collect::<Vec<_>>(),
            _ => vec![ch],
        })
        .collect()
}
