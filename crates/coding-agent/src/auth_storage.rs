use crate::resolve_config_value::resolve_config_value;
use ai::{find_env_keys, get_env_api_key};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub type AuthStorageData = BTreeMap<String, AuthCredential>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthCredential {
    ApiKey {
        key: String,
    },
    OAuth {
        #[serde(flatten)]
        data: BTreeMap<String, Value>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthSource {
    Stored,
    Runtime,
    Environment,
    Fallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthStatus {
    pub configured: bool,
    pub source: Option<AuthSource>,
    pub label: Option<String>,
}

pub trait AuthStorageBackend {
    fn read(&self) -> Result<Option<String>, String>;
    fn write(&mut self, content: String) -> Result<(), String>;
}

#[derive(Debug, Clone)]
pub struct InMemoryAuthStorageBackend {
    value: Option<String>,
}

impl InMemoryAuthStorageBackend {
    pub fn new(value: Option<String>) -> Self {
        Self { value }
    }
}

impl AuthStorageBackend for InMemoryAuthStorageBackend {
    fn read(&self) -> Result<Option<String>, String> {
        Ok(self.value.clone())
    }

    fn write(&mut self, content: String) -> Result<(), String> {
        self.value = Some(content);
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct FileAuthStorageBackend {
    auth_path: PathBuf,
}

impl FileAuthStorageBackend {
    pub fn new(auth_path: impl Into<PathBuf>) -> Self {
        Self {
            auth_path: auth_path.into(),
        }
    }

    fn ensure_file(&self) -> Result<(), String> {
        if let Some(parent) = self.auth_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建认证目录 {} 失败：{error}", parent.display()))?;
        }
        if !self.auth_path.exists() {
            fs::write(&self.auth_path, "{}").map_err(|error| {
                format!("创建认证文件 {} 失败：{error}", self.auth_path.display())
            })?;
            set_owner_only_permissions(&self.auth_path)?;
        }
        Ok(())
    }
}

impl AuthStorageBackend for FileAuthStorageBackend {
    fn read(&self) -> Result<Option<String>, String> {
        self.ensure_file()?;
        fs::read_to_string(&self.auth_path)
            .map(Some)
            .map_err(|error| format!("读取认证文件 {} 失败：{error}", self.auth_path.display()))
    }

    fn write(&mut self, content: String) -> Result<(), String> {
        self.ensure_file()?;
        fs::write(&self.auth_path, content)
            .map_err(|error| format!("写入认证文件 {} 失败：{error}", self.auth_path.display()))?;
        set_owner_only_permissions(&self.auth_path)
    }
}

pub struct AuthStorage<B: AuthStorageBackend> {
    backend: B,
    data: AuthStorageData,
    runtime_overrides: BTreeMap<String, String>,
    fallback_resolver: Option<Box<dyn Fn(&str) -> Option<String> + Send + Sync>>,
    errors: Vec<String>,
    load_error: bool,
}

impl AuthStorage<InMemoryAuthStorageBackend> {
    pub fn in_memory(data: AuthStorageData) -> Self {
        let content = serde_json::to_string_pretty(&data).expect("auth data should encode");
        Self::from_backend(InMemoryAuthStorageBackend::new(Some(content)))
    }
}

impl<B: AuthStorageBackend> AuthStorage<B> {
    pub fn from_backend(backend: B) -> Self {
        let mut storage = Self {
            backend,
            data: BTreeMap::new(),
            runtime_overrides: BTreeMap::new(),
            fallback_resolver: None,
            errors: Vec::new(),
            load_error: false,
        };
        storage.reload();
        storage
    }

    pub fn reload(&mut self) {
        match self
            .backend
            .read()
            .and_then(|content| parse_auth_data(content.as_deref()))
        {
            Ok(data) => {
                self.data = data;
                self.load_error = false;
            }
            Err(error) => {
                self.errors.push(error);
                self.data.clear();
                self.load_error = true;
            }
        }
    }

    pub fn set_runtime_api_key(&mut self, provider: impl Into<String>, api_key: impl Into<String>) {
        self.runtime_overrides
            .insert(provider.into(), api_key.into());
    }

    pub fn remove_runtime_api_key(&mut self, provider: &str) {
        self.runtime_overrides.remove(provider);
    }

    pub fn set_fallback_resolver(
        &mut self,
        resolver: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) {
        self.fallback_resolver = Some(Box::new(resolver));
    }

    pub fn get(&self, provider: &str) -> Option<&AuthCredential> {
        self.data.get(provider)
    }

    pub fn set(&mut self, provider: impl Into<String>, credential: AuthCredential) {
        let provider = provider.into();
        self.data.insert(provider.clone(), credential);
        self.persist_provider_change(provider, true);
    }

    pub fn remove(&mut self, provider: &str) {
        self.data.remove(provider);
        self.persist_provider_change(provider.to_string(), false);
    }

    pub fn logout(&mut self, provider: &str) {
        self.remove(provider);
    }

    pub fn list(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }

    pub fn has(&self, provider: &str) -> bool {
        self.data.contains_key(provider)
    }

    pub fn has_auth(&self, provider: &str) -> bool {
        self.runtime_overrides.contains_key(provider)
            || self.data.contains_key(provider)
            || get_env_api_key(provider).is_some()
            || self
                .fallback_resolver
                .as_ref()
                .and_then(|resolver| resolver(provider))
                .is_some()
    }

    pub fn auth_status(&self, provider: &str) -> AuthStatus {
        if self.data.contains_key(provider) {
            return AuthStatus {
                configured: true,
                source: Some(AuthSource::Stored),
                label: None,
            };
        }
        if self.runtime_overrides.contains_key(provider) {
            return AuthStatus {
                configured: false,
                source: Some(AuthSource::Runtime),
                label: Some("--api-key".to_string()),
            };
        }
        if let Some(env_key) = find_env_keys(provider).and_then(|keys| keys.first().cloned()) {
            return AuthStatus {
                configured: false,
                source: Some(AuthSource::Environment),
                label: Some(env_key),
            };
        }
        if self
            .fallback_resolver
            .as_ref()
            .and_then(|resolver| resolver(provider))
            .is_some()
        {
            return AuthStatus {
                configured: false,
                source: Some(AuthSource::Fallback),
                label: Some("custom provider config".to_string()),
            };
        }
        AuthStatus {
            configured: false,
            source: None,
            label: None,
        }
    }

    pub fn all(&self) -> AuthStorageData {
        self.data.clone()
    }

    pub fn drain_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    pub fn api_key(&self, provider: &str, include_fallback: bool) -> Option<String> {
        if let Some(runtime_key) = self.runtime_overrides.get(provider) {
            return Some(runtime_key.clone());
        }
        if let Some(AuthCredential::ApiKey { key }) = self.data.get(provider) {
            return resolve_config_value(key);
        }
        if let Some(env_key) = get_env_api_key(provider) {
            return Some(env_key);
        }
        if include_fallback {
            return self
                .fallback_resolver
                .as_ref()
                .and_then(|resolver| resolver(provider));
        }
        None
    }

    fn persist_provider_change(&mut self, provider: String, exists: bool) {
        if self.load_error {
            return;
        }
        let data = self.data.clone();
        match serde_json::to_string_pretty(&data).map_err(|error| error.to_string()) {
            Ok(content) => {
                if let Err(error) = self.backend.write(content) {
                    self.errors.push(error);
                }
            }
            Err(error) => self.errors.push(error),
        }
        if !exists {
            self.runtime_overrides.remove(&provider);
        }
    }
}

fn parse_auth_data(content: Option<&str>) -> Result<AuthStorageData, String> {
    let Some(content) = content.filter(|value| !value.trim().is_empty()) else {
        return Ok(BTreeMap::new());
    };
    serde_json::from_str(content).map_err(|error| error.to_string())
}

fn set_owner_only_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, permissions)
            .map_err(|error| format!("设置认证文件权限 {} 失败：{error}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_priority_uses_runtime_then_stored_then_fallback() {
        let mut data = AuthStorageData::new();
        data.insert(
            "openai".to_string(),
            AuthCredential::ApiKey {
                key: "stored".to_string(),
            },
        );
        let mut storage = AuthStorage::in_memory(data);
        storage.set_fallback_resolver(|provider| {
            (provider == "openai").then(|| "fallback".to_string())
        });
        assert_eq!(storage.api_key("openai", true).as_deref(), Some("stored"));

        storage.set_runtime_api_key("openai", "runtime");
        assert_eq!(storage.api_key("openai", true).as_deref(), Some("runtime"));
    }

    #[test]
    fn auth_status_reports_sources_without_exposing_secret() {
        let storage = AuthStorage::in_memory(AuthStorageData::new());
        assert_eq!(
            storage.auth_status("missing"),
            AuthStatus {
                configured: false,
                source: None,
                label: None
            }
        );
    }
}
