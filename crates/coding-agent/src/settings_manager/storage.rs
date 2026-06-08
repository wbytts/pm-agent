use super::types::SettingsScope;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_DIR_NAME: &str = ".pm-agent";

pub trait SettingsStorage {
    fn read(&self, scope: SettingsScope) -> Result<Option<String>, String>;
    fn write(&mut self, scope: SettingsScope, content: String) -> Result<(), String>;
}

#[derive(Debug, Clone)]
pub struct InMemorySettingsStorage {
    global: Option<String>,
    project: Option<String>,
}

impl InMemorySettingsStorage {
    pub fn new() -> Self {
        Self {
            global: None,
            project: None,
        }
    }
}

impl Default for InMemorySettingsStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsStorage for InMemorySettingsStorage {
    fn read(&self, scope: SettingsScope) -> Result<Option<String>, String> {
        Ok(match scope {
            SettingsScope::Global => self.global.clone(),
            SettingsScope::Project => self.project.clone(),
        })
    }

    fn write(&mut self, scope: SettingsScope, content: String) -> Result<(), String> {
        match scope {
            SettingsScope::Global => self.global = Some(content),
            SettingsScope::Project => self.project = Some(content),
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FileSettingsStorage {
    global_settings_path: PathBuf,
    project_settings_path: PathBuf,
}

impl FileSettingsStorage {
    pub fn new(cwd: impl AsRef<Path>, agent_dir: impl AsRef<Path>) -> Self {
        Self {
            global_settings_path: agent_dir.as_ref().join("settings.json"),
            project_settings_path: cwd.as_ref().join(CONFIG_DIR_NAME).join("settings.json"),
        }
    }

    fn path_for_scope(&self, scope: SettingsScope) -> &Path {
        match scope {
            SettingsScope::Global => &self.global_settings_path,
            SettingsScope::Project => &self.project_settings_path,
        }
    }
}

impl SettingsStorage for FileSettingsStorage {
    fn read(&self, scope: SettingsScope) -> Result<Option<String>, String> {
        let path = self.path_for_scope(scope);
        if !path.exists() {
            return Ok(None);
        }
        fs::read_to_string(path)
            .map(Some)
            .map_err(|error| format!("读取设置文件 {} 失败：{error}", path.display()))
    }

    fn write(&mut self, scope: SettingsScope, content: String) -> Result<(), String> {
        let path = self.path_for_scope(scope);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建设置目录 {} 失败：{error}", parent.display()))?;
        }
        fs::write(path, content)
            .map_err(|error| format!("写入设置文件 {} 失败：{error}", path.display()))
    }
}
