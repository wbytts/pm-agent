use agent::harness::SessionStorage;

use crate::auth_storage::AuthStorageBackend;
use crate::extensions::ExtensionRunner;
use crate::resource_loader::DefaultResourceLoader;
use crate::settings_manager::SettingsStorage;
use crate::slash_commands::compose_slash_commands;

use super::session_backend::ManagedRpcSessionBackend;

pub struct RpcResourceSnapshot {
    pub extension_runner: ExtensionRunner,
}

impl<S: SessionStorage, B: AuthStorageBackend> ManagedRpcSessionBackend<S, B> {
    pub fn apply_loaded_resources<T: SettingsStorage>(
        &mut self,
        loader: &DefaultResourceLoader<T>,
    ) -> RpcResourceSnapshot {
        let extension_runner = ExtensionRunner::from_loaded_resources(loader.extensions());
        let extension_commands = extension_runner.registered_slash_commands();
        let resource_commands = loader.resource_slash_commands();
        self.set_slash_commands(compose_slash_commands(
            extension_commands,
            resource_commands,
        ));
        self.set_prompt_resources(loader.skills().0.to_vec(), loader.prompts().0.to_vec());
        self.set_extension_commands(extension_runner.resolved_commands());
        RpcResourceSnapshot { extension_runner }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_storage::{AuthStorage, AuthStorageData, InMemoryAuthStorageBackend};
    use crate::extensions::{ExtensionApi, ExtensionFactory};
    use crate::model_registry::ModelRegistry;
    use crate::resource_loader::DefaultResourceLoaderOptions;
    use crate::rpc::dispatcher::RpcSessionBackend;
    use crate::session_manager::SessionManager;
    use crate::settings_manager::{InMemorySettingsStorage, SettingsManager};
    use agent::harness::InMemorySessionStorage;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    struct CommandFactory {
        seen_args: Arc<Mutex<Vec<String>>>,
    }

    impl ExtensionFactory for CommandFactory {
        fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String> {
            let seen_args = self.seen_args.clone();
            api.register_command(
                "demo",
                Some("Demo command".to_string()),
                Arc::new(move |ctx| {
                    *seen_args.lock().expect("args lock") = ctx.args;
                    Ok(())
                }),
            )
        }
    }

    struct NamedCommandFactory {
        label: &'static str,
        seen: Arc<Mutex<Vec<String>>>,
    }

    impl ExtensionFactory for NamedCommandFactory {
        fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String> {
            let seen = self.seen.clone();
            let label = self.label.to_string();
            api.register_command(
                "demo",
                Some(format!("Demo {label}")),
                Arc::new(move |ctx| {
                    let mut seen = seen.lock().expect("seen lock");
                    seen.push(label.clone());
                    seen.extend(ctx.args);
                    Ok(())
                }),
            )
        }
    }

    #[test]
    fn applies_loaded_resources_to_rpc_backend() {
        let dir = temp_dir();
        let skill_dir = dir.join("skill");
        std::fs::create_dir_all(&skill_dir).expect("skill dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: Demo skill\n---\nUse skill.",
        )
        .expect("skill");
        std::fs::write(dir.join("prompt.md"), "# Review\nReview $ARGUMENTS").expect("prompt");
        let seen_args = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: vec![display_path(&skill_dir)],
            additional_prompt_paths: vec![display_path(dir.join("prompt.md"))],
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/demo.ts".to_string(),
                Box::new(CommandFactory {
                    seen_args: seen_args.clone(),
                }),
            )],
            no_extensions: false,
            no_skills: false,
            no_prompt_templates: false,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let mut backend = test_backend();

        let snapshot = backend.apply_loaded_resources(&loader);

        assert_eq!(snapshot.extension_runner.registered_commands().len(), 1);
        assert_eq!(
            backend
                .commands()
                .expect("commands")
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            vec!["demo", "prompt", "skill:demo-skill"]
        );
        backend
            .prompt("/prompt src/lib.rs".to_string())
            .expect("prompt");
        assert_eq!(
            backend.messages().expect("messages")[0].content,
            "# Review\nReview src/lib.rs"
        );
        backend
            .prompt("/demo one two".to_string())
            .expect("extension command");
        assert_eq!(
            *seen_args.lock().expect("args lock"),
            vec!["one".to_string(), "two".to_string()]
        );
    }

    #[test]
    fn applies_duplicate_extension_commands_with_pi_invocation_names() {
        let dir = temp_dir();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir,
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![
                (
                    "/extensions/one.ts".to_string(),
                    Box::new(NamedCommandFactory {
                        label: "one",
                        seen: seen.clone(),
                    }),
                ),
                (
                    "/extensions/two.ts".to_string(),
                    Box::new(NamedCommandFactory {
                        label: "two",
                        seen: seen.clone(),
                    }),
                ),
            ],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let mut backend = test_backend();

        let snapshot = backend.apply_loaded_resources(&loader);

        assert_eq!(
            snapshot
                .extension_runner
                .resolved_commands()
                .iter()
                .map(|command| command.invocation_name.as_str())
                .collect::<Vec<_>>(),
            vec!["demo:1", "demo:2"]
        );
        assert_eq!(
            backend
                .commands()
                .expect("commands")
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>(),
            vec!["demo:1", "demo:2"]
        );

        backend
            .prompt("/demo:2 beta".to_string())
            .expect("second command should run");

        assert_eq!(
            *seen.lock().expect("seen lock"),
            vec!["two".to_string(), "beta".to_string()]
        );
        assert!(backend.messages().expect("messages").is_empty());
    }

    fn test_backend() -> ManagedRpcSessionBackend<InMemorySessionStorage, InMemoryAuthStorageBackend>
    {
        let session_manager = SessionManager::in_memory("/tmp/project");
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        ManagedRpcSessionBackend::new(session_manager, model_registry)
    }

    fn temp_dir() -> PathBuf {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-rpc-resources-{millis}"));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn display_path(path: impl AsRef<std::path::Path>) -> String {
        path.as_ref().to_string_lossy().to_string()
    }
}
