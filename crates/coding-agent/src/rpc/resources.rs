use agent::harness::SessionStorage;
use serde_json::json;

use crate::auth_storage::AuthStorageBackend;
use crate::extensions::types::ExtensionEventKind;
use crate::extensions::{ExtensionEvent, ExtensionRunner};
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
        let mut extension_runner = ExtensionRunner::from_loaded_resources(loader.extensions());
        self.bind_extension_runner(&mut extension_runner);
        let session_start = self.take_pending_session_start();
        let mut session_start_payload = json!({
            "type": "session_start",
            "reason": session_start.reason,
        });
        if let Some(previous_session_file) = session_start.previous_session_file {
            session_start_payload["previousSessionFile"] =
                serde_json::Value::String(previous_session_file);
        }
        extension_runner.emit(
            "session_start",
            ExtensionEvent {
                kind: ExtensionEventKind::SessionStart,
                payload: session_start_payload,
            },
        );
        self.flush_bound_extension_provider_registrations();
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
    use ai::{LanguageModelProvider, StreamEvent};
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

    struct StreamProviderFactory;

    impl ExtensionFactory for StreamProviderFactory {
        fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String> {
            api.register_provider(
                "stream-provider",
                crate::extensions::ProviderConfig {
                    api: Some("local-echo".to_string()),
                    stream_simple: Some(Arc::new(|_request| {
                        Ok(vec![StreamEvent::Finished {
                            message: ai::Message {
                                role: ai::MessageRole::Assistant,
                                content: "extension backend stream".to_string(),
                            },
                        }])
                    })),
                    ..crate::extensions::ProviderConfig::default()
                },
            )
        }
    }

    struct CommandProviderFactory;

    impl ExtensionFactory for CommandProviderFactory {
        fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String> {
            let runtime = api.runtime();
            let unregister_runtime = runtime.clone();
            api.register_command(
                "use-proxy",
                Some("Use proxy".to_string()),
                Arc::new(move |_ctx| {
                    runtime.register_provider(
                        "anthropic",
                        crate::extensions::ProviderConfig {
                            base_url: Some("http://localhost:8080/command".to_string()),
                            ..crate::extensions::ProviderConfig::default()
                        },
                        "/extensions/command-provider.ts",
                    );
                    Ok(())
                }),
            )?;
            api.register_command(
                "clear-proxy",
                Some("Clear proxy".to_string()),
                Arc::new(move |_ctx| {
                    unregister_runtime
                        .unregister_provider("anthropic", "/extensions/command-provider.ts");
                    Ok(())
                }),
            )
        }
    }

    struct SessionStartProviderFactory {
        seen_events: Arc<Mutex<Vec<String>>>,
    }

    impl ExtensionFactory for SessionStartProviderFactory {
        fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String> {
            let runtime = api.runtime();
            let seen_events = self.seen_events.clone();
            api.on(
                "session_start",
                Arc::new(move |event| {
                    seen_events
                        .lock()
                        .expect("events lock")
                        .push(event.payload["reason"].as_str().unwrap_or("").to_string());
                    runtime.register_provider(
                        "anthropic",
                        crate::extensions::ProviderConfig {
                            base_url: Some("http://localhost:8080/session-start".to_string()),
                            ..crate::extensions::ProviderConfig::default()
                        },
                        "/extensions/session-start-provider.ts",
                    );
                    None
                }),
            )
        }
    }

    struct LifecycleFactory {
        events: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl ExtensionFactory for LifecycleFactory {
        fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String> {
            let start_events = self.events.clone();
            api.on(
                "session_start",
                Arc::new(move |event| {
                    start_events
                        .lock()
                        .expect("events lock")
                        .push(event.payload);
                    None
                }),
            )?;
            let shutdown_events = self.events.clone();
            api.on(
                "session_shutdown",
                Arc::new(move |event| {
                    shutdown_events
                        .lock()
                        .expect("events lock")
                        .push(event.payload);
                    None
                }),
            )?;
            let fork_events = self.events.clone();
            api.on(
                "session_before_fork",
                Arc::new(move |event| {
                    fork_events.lock().expect("events lock").push(event.payload);
                    None
                }),
            )?;
            let before_compact_events = self.events.clone();
            api.on(
                "session_before_compact",
                Arc::new(move |event| {
                    let payload = event.payload;
                    before_compact_events
                        .lock()
                        .expect("events lock")
                        .push(payload.clone());
                    Some(json!({
                        "compaction": {
                            "summary": "Custom summary from extension",
                            "firstKeptEntryId": payload["preparation"]["firstKeptEntryId"].clone(),
                            "tokensBefore": payload["preparation"]["tokensBefore"].clone(),
                            "details": {
                                "source": "extension"
                            }
                        }
                    }))
                }),
            )?;
            let compact_events = self.events.clone();
            api.on(
                "session_compact",
                Arc::new(move |event| {
                    compact_events
                        .lock()
                        .expect("events lock")
                        .push(event.payload);
                    None
                }),
            )?;
            let before_tree_events = self.events.clone();
            api.on(
                "session_before_tree",
                Arc::new(move |event| {
                    let payload = event.payload;
                    before_tree_events
                        .lock()
                        .expect("events lock")
                        .push(payload.clone());
                    Some(json!({
                        "summary": {
                            "summary": "Tree summary from extension",
                            "details": {
                                "source": "tree-extension"
                            }
                        },
                        "label": "tree-label"
                    }))
                }),
            )?;
            let tree_events = self.events.clone();
            api.on(
                "session_tree",
                Arc::new(move |event| {
                    tree_events.lock().expect("events lock").push(event.payload);
                    None
                }),
            )
        }
    }

    struct CancelForkFactory {
        events: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl ExtensionFactory for CancelForkFactory {
        fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String> {
            let events = self.events.clone();
            api.on(
                "session_before_fork",
                Arc::new(move |event| {
                    events.lock().expect("events lock").push(event.payload);
                    Some(json!({ "cancel": true }))
                }),
            )
        }
    }

    struct CancelCompactFactory {
        events: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl ExtensionFactory for CancelCompactFactory {
        fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String> {
            let events = self.events.clone();
            api.on(
                "session_before_compact",
                Arc::new(move |event| {
                    events.lock().expect("events lock").push(event.payload);
                    Some(json!({ "cancel": true }))
                }),
            )
        }
    }

    struct CancelTreeFactory {
        events: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl ExtensionFactory for CancelTreeFactory {
        fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String> {
            let events = self.events.clone();
            api.on(
                "session_before_tree",
                Arc::new(move |event| {
                    events.lock().expect("events lock").push(event.payload);
                    Some(json!({ "cancel": true }))
                }),
            )
        }
    }

    struct CancelSwitchFactory {
        events: Arc<Mutex<Vec<serde_json::Value>>>,
        cancel_reason: &'static str,
    }

    impl ExtensionFactory for CancelSwitchFactory {
        fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String> {
            let events = self.events.clone();
            let cancel_reason = self.cancel_reason.to_string();
            api.on(
                "session_before_switch",
                Arc::new(move |event| {
                    let should_cancel =
                        event.payload["reason"].as_str() == Some(cancel_reason.as_str());
                    events.lock().expect("events lock").push(event.payload);
                    should_cancel.then(|| json!({ "cancel": true }))
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

    #[test]
    fn applies_extension_stream_provider_to_backend_provider_registry_like_pi() {
        let dir = temp_dir();
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
            extension_factories: vec![(
                "/extensions/stream.ts".to_string(),
                Box::new(StreamProviderFactory),
            )],
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

        let model = ai::Model {
            id: "echo".to_string(),
            provider: "local".to_string(),
            api: "local-echo".to_string(),
            display_name: "Echo".to_string(),
            context_window: 32_000,
            ..ai::Model::default()
        };
        let overridden = backend
            .provider_registry()
            .provider_for(&model)
            .expect("provider")
            .stream(ai::StreamRequest {
                model: model.clone(),
                messages: Vec::new(),
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: Default::default(),
            })
            .expect("stream");
        assert_eq!(
            overridden,
            vec![StreamEvent::Finished {
                message: ai::Message {
                    role: ai::MessageRole::Assistant,
                    content: "extension backend stream".to_string(),
                }
            }]
        );

        backend.unregister_extension_provider(&snapshot.extension_runner, "stream-provider");
        let restored = backend
            .provider_registry()
            .provider_for(&model)
            .expect("provider")
            .stream(ai::StreamRequest {
                model,
                messages: vec![ai::Message {
                    role: ai::MessageRole::User,
                    content: "hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: Default::default(),
            })
            .expect("stream");
        assert_ne!(
            restored,
            vec![StreamEvent::Finished {
                message: ai::Message {
                    role: ai::MessageRole::Assistant,
                    content: "extension backend stream".to_string(),
                }
            }]
        );
    }

    #[test]
    fn extension_command_provider_registration_updates_backend_model_like_pi() {
        let dir = temp_dir();
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
            extension_factories: vec![(
                "/extensions/command-provider.ts".to_string(),
                Box::new(CommandProviderFactory),
            )],
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
        let anthropic_model = backend
            .model_registry()
            .get_all()
            .into_iter()
            .find(|model| model.provider == "anthropic")
            .expect("builtin anthropic model should exist");
        backend
            .set_model(anthropic_model.provider, anthropic_model.id)
            .expect("set anthropic model");
        backend.apply_loaded_resources(&loader);

        backend
            .prompt("/use-proxy".to_string())
            .expect("command should run");

        assert_eq!(
            backend.model().and_then(|model| model.base_url.as_deref()),
            Some("http://localhost:8080/command")
        );
        assert!(backend.messages().expect("messages").is_empty());
    }

    #[test]
    fn extension_command_provider_unregistration_restores_backend_model_like_pi() {
        let dir = temp_dir();
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
            extension_factories: vec![(
                "/extensions/command-provider.ts".to_string(),
                Box::new(CommandProviderFactory),
            )],
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
        let anthropic_model = backend
            .model_registry()
            .get_all()
            .into_iter()
            .find(|model| model.provider == "anthropic")
            .expect("builtin anthropic model should exist");
        backend
            .set_model(anthropic_model.provider, anthropic_model.id)
            .expect("set anthropic model");
        let original_base_url = backend
            .model()
            .and_then(|model| model.base_url.clone())
            .expect("builtin model should have baseUrl");
        backend.apply_loaded_resources(&loader);

        backend
            .prompt("/use-proxy".to_string())
            .expect("command should run");
        assert_eq!(
            backend.model().and_then(|model| model.base_url.as_deref()),
            Some("http://localhost:8080/command")
        );

        backend
            .prompt("/clear-proxy".to_string())
            .expect("command should run");

        assert_eq!(
            backend.model().and_then(|model| model.base_url.as_deref()),
            Some(original_base_url.as_str())
        );
        assert!(backend.messages().expect("messages").is_empty());
    }

    #[test]
    fn apply_loaded_resources_emits_session_start_and_flushes_provider_like_pi() {
        let dir = temp_dir();
        let seen_events = Arc::new(Mutex::new(Vec::new()));
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
            extension_factories: vec![(
                "/extensions/session-start-provider.ts".to_string(),
                Box::new(SessionStartProviderFactory {
                    seen_events: seen_events.clone(),
                }),
            )],
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
        let anthropic_model = backend
            .model_registry()
            .get_all()
            .into_iter()
            .find(|model| model.provider == "anthropic")
            .expect("builtin anthropic model should exist");
        backend
            .set_model(anthropic_model.provider, anthropic_model.id)
            .expect("set anthropic model");

        backend.apply_loaded_resources(&loader);

        assert_eq!(
            *seen_events.lock().expect("events lock"),
            vec!["startup".to_string()]
        );
        assert_eq!(
            backend.model().and_then(|model| model.base_url.as_deref()),
            Some("http://localhost:8080/session-start")
        );
    }

    #[test]
    fn reload_emits_shutdown_invalidates_old_runtime_and_next_session_start_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/lifecycle.ts".to_string(),
                Box::new(LifecycleFactory {
                    events: events.clone(),
                }),
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let old_runtime = loader.extensions().runtime.clone();
        let session_manager =
            SessionManager::create("/tmp/project", Some(dir)).expect("session should create");
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session_manager, model_registry);
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();

        let result = backend.reload().expect("reload");
        loader.reload().expect("reload should rebuild extensions");
        backend.apply_loaded_resources(&loader);

        assert_eq!(result["cancelled"], false);
        assert_eq!(
            old_runtime
                .assert_active()
                .expect_err("old runtime should be stale"),
            crate::extensions::types::STALE_EXTENSION_CONTEXT_MESSAGE
        );
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![
                json!({
                    "type": "session_shutdown",
                    "reason": "reload",
                }),
                json!({
                    "type": "session_start",
                    "reason": "reload",
                }),
            ]
        );
    }

    #[test]
    fn compact_uses_extension_compaction_and_emits_events_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/lifecycle.ts".to_string(),
                Box::new(LifecycleFactory {
                    events: events.clone(),
                }),
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let session_manager =
            SessionManager::create("/tmp/project", Some(dir)).expect("session should create");
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session_manager, model_registry);
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();
        backend.prompt("first request".to_string()).expect("prompt");
        backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::Assistant,
                "first answer".to_string(),
            ))
            .expect("assistant");
        backend
            .prompt("second request with enough text to compact".to_string())
            .expect("prompt");

        let result = backend.compact(None).expect("compact");

        assert_eq!(result["summary"], "Custom summary from extension");
        assert_eq!(result["details"]["source"], "extension");
        let entries = backend
            .session_manager()
            .branch(None)
            .expect("branch entries");
        let compaction = entries
            .iter()
            .find(|entry| matches!(entry, agent::harness::SessionTreeEntry::Compaction { .. }))
            .expect("compaction entry");
        match compaction {
            agent::harness::SessionTreeEntry::Compaction {
                summary,
                details,
                from_hook,
                ..
            } => {
                assert_eq!(summary, "Custom summary from extension");
                assert_eq!(
                    details.as_ref().and_then(|value| value["source"].as_str()),
                    Some("extension")
                );
                assert!(from_hook);
            }
            _ => unreachable!("matched compaction"),
        }
        let events = events.lock().expect("events lock");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["type"], "session_before_compact");
        assert!(events[0]["preparation"]["messagesToSummarize"].is_array());
        assert!(events[0]["branchEntries"].is_array());
        assert_eq!(events[1]["type"], "session_compact");
        assert_eq!(events[1]["fromExtension"], true);
        assert_eq!(
            events[1]["compactionEntry"]["summary"],
            "Custom summary from extension"
        );
    }

    #[test]
    fn compact_honors_session_before_compact_cancellation_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/cancel-compact.ts".to_string(),
                Box::new(CancelCompactFactory {
                    events: events.clone(),
                }),
            )],
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
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();
        backend.prompt("first request".to_string()).expect("prompt");
        backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::Assistant,
                "first answer".to_string(),
            ))
            .expect("assistant");
        backend
            .prompt("second request".to_string())
            .expect("prompt");

        let error = backend.compact(None).expect_err("compact should cancel");

        assert_eq!(error, "Compaction cancelled");
        assert!(backend
            .session_manager()
            .branch(None)
            .expect("branch entries")
            .iter()
            .all(|entry| !matches!(entry, agent::harness::SessionTreeEntry::Compaction { .. })));
        assert_eq!(events.lock().expect("events lock").len(), 1);
        assert_eq!(
            events.lock().expect("events lock")[0]["type"],
            "session_before_compact"
        );
    }

    #[test]
    fn navigate_tree_moves_to_user_parent_and_returns_editor_text_like_pi() {
        let mut backend = test_backend();
        backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "first request".to_string(),
            ))
            .expect("first prompt");
        let first_assistant = backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::Assistant,
                "first answer".to_string(),
            ))
            .expect("assistant");
        let second = backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "second request".to_string(),
            ))
            .expect("second prompt");
        backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::Assistant,
                "second answer".to_string(),
            ))
            .expect("second assistant");

        let result = backend
            .navigate_tree(second.clone(), false, None, false, None)
            .expect("navigate tree");

        assert_eq!(result["cancelled"], false);
        assert_eq!(result["editorText"], "second request");
        assert_eq!(
            backend.session_manager().leaf_id().expect("leaf"),
            Some(first_assistant)
        );
        assert!(backend
            .session_manager()
            .branch(None)
            .expect("branch entries")
            .iter()
            .all(|entry| !matches!(
                entry,
                agent::harness::SessionTreeEntry::BranchSummary { .. }
            )));
    }

    #[test]
    fn navigate_tree_uses_extension_summary_and_emits_events_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/lifecycle.ts".to_string(),
                Box::new(LifecycleFactory {
                    events: events.clone(),
                }),
            )],
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
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();
        let root_user = backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "root request".to_string(),
            ))
            .expect("prompt");
        let root_assistant = backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::Assistant,
                "root answer".to_string(),
            ))
            .expect("assistant");
        backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "branch request".to_string(),
            ))
            .expect("branch prompt");

        let result = backend
            .navigate_tree(root_user.clone(), true, None, false, None)
            .expect("navigate tree");

        assert_eq!(result["cancelled"], false);
        assert_eq!(result["editorText"], "root request");
        assert_eq!(
            result["summaryEntry"]["summary"],
            "Tree summary from extension"
        );
        assert_eq!(
            backend.session_manager().leaf_id().expect("leaf"),
            result["summaryEntry"]["id"]
                .as_str()
                .map(ToString::to_string)
        );
        let summary_id = result["summaryEntry"]["id"].as_str().expect("summary id");
        let summary_entry = backend
            .session_manager()
            .entry(summary_id)
            .expect("summary entry");
        match summary_entry {
            agent::harness::SessionTreeEntry::BranchSummary {
                parent_id,
                summary,
                details,
                from_hook,
                ..
            } => {
                assert_eq!(parent_id, &None);
                assert_eq!(summary, "Tree summary from extension");
                assert_eq!(
                    details.as_ref().and_then(|value| value["source"].as_str()),
                    Some("tree-extension")
                );
                assert!(from_hook);
            }
            _ => panic!("expected branch summary entry"),
        }
        assert!(matches!(
            backend.session_manager().entry(&root_assistant),
            Some(agent::harness::SessionTreeEntry::Message { .. })
        ));
        let events = events.lock().expect("events lock");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["type"], "session_before_tree");
        assert_eq!(events[0]["preparation"]["targetId"], root_user);
        assert_eq!(events[0]["preparation"]["userWantsSummary"], true);
        assert!(events[0]["preparation"]["entriesToSummarize"].is_array());
        assert_eq!(events[1]["type"], "session_tree");
        assert_eq!(events[1]["fromExtension"], true);
        assert_eq!(
            events[1]["summaryEntry"]["summary"],
            "Tree summary from extension"
        );
    }

    #[test]
    fn navigate_tree_honors_session_before_tree_cancellation_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/cancel-tree.ts".to_string(),
                Box::new(CancelTreeFactory {
                    events: events.clone(),
                }),
            )],
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
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();
        let first = backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "first".to_string(),
            ))
            .expect("prompt");
        let assistant = backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::Assistant,
                "answer".to_string(),
            ))
            .expect("assistant");
        let old_leaf = backend.session_manager().leaf_id().expect("leaf");

        let result = backend
            .navigate_tree(first.clone(), true, None, false, None)
            .expect("navigate tree");

        assert_eq!(result["cancelled"], true);
        assert_eq!(backend.session_manager().leaf_id().expect("leaf"), old_leaf);
        assert!(matches!(
            backend.session_manager().entry(&assistant),
            Some(agent::harness::SessionTreeEntry::Message { .. })
        ));
        assert_eq!(events.lock().expect("events lock").len(), 1);
        assert_eq!(
            events.lock().expect("events lock")[0]["type"],
            "session_before_tree"
        );
    }

    #[test]
    fn new_session_emits_shutdown_and_next_session_start_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/lifecycle.ts".to_string(),
                Box::new(LifecycleFactory {
                    events: events.clone(),
                }),
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let session_manager =
            SessionManager::create("/tmp/project", Some(dir)).expect("session should create");
        let old_file = session_manager
            .session_file()
            .expect("session file")
            .to_string_lossy()
            .to_string();
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session_manager, model_registry);
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();

        backend.new_session(None).expect("new session");
        let new_file = backend
            .state()
            .expect("state")
            .session_file
            .expect("new session file");
        backend.apply_loaded_resources(&loader);

        assert_eq!(
            *events.lock().expect("events lock"),
            vec![
                json!({
                    "type": "session_shutdown",
                    "reason": "new",
                    "targetSessionFile": new_file,
                }),
                json!({
                    "type": "session_start",
                    "reason": "new",
                    "previousSessionFile": old_file,
                }),
            ]
        );
    }

    #[test]
    fn switch_session_emits_shutdown_and_resume_start_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/lifecycle.ts".to_string(),
                Box::new(LifecycleFactory {
                    events: events.clone(),
                }),
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let first_cwd = temp_dir();
        let second_cwd = temp_dir();
        let mut first =
            SessionManager::create(first_cwd, Some(dir.clone())).expect("first session");
        first
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "first".to_string(),
            ))
            .expect("first message");
        let first_file = first
            .session_file()
            .expect("first file")
            .to_string_lossy()
            .to_string();
        let mut second = SessionManager::create(second_cwd, Some(dir)).expect("second session");
        second
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "second".to_string(),
            ))
            .expect("second message");
        let second_file = second
            .session_file()
            .expect("second file")
            .to_string_lossy()
            .to_string();
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(first, model_registry);
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();

        backend
            .switch_session(second_file.clone())
            .expect("switch session");
        backend.apply_loaded_resources(&loader);

        assert_eq!(
            *events.lock().expect("events lock"),
            vec![
                json!({
                    "type": "session_shutdown",
                    "reason": "resume",
                    "targetSessionFile": second_file,
                }),
                json!({
                    "type": "session_start",
                    "reason": "resume",
                    "previousSessionFile": first_file,
                }),
            ]
        );
    }

    #[test]
    fn import_session_emits_shutdown_and_resume_start_like_pi() {
        let dir = temp_dir();
        let source_dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/lifecycle.ts".to_string(),
                Box::new(LifecycleFactory {
                    events: events.clone(),
                }),
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let current_cwd = temp_dir();
        let source_cwd = temp_dir();
        let current =
            SessionManager::create(current_cwd, Some(dir.clone())).expect("current session");
        let current_file = current
            .session_file()
            .expect("current file")
            .to_string_lossy()
            .to_string();
        let mut source =
            SessionManager::create(source_cwd, Some(source_dir)).expect("source session");
        source
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "imported".to_string(),
            ))
            .expect("source message");
        let source_file = source.session_file().expect("source file").to_path_buf();
        let expected_destination = dir
            .join(source_file.file_name().expect("source basename"))
            .to_string_lossy()
            .to_string();
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(current, model_registry);
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();

        let result = backend
            .import_session(source_file.to_string_lossy().to_string(), None)
            .expect("import session");
        backend.apply_loaded_resources(&loader);

        assert_eq!(result["cancelled"], false);
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![
                json!({
                    "type": "session_shutdown",
                    "reason": "resume",
                    "targetSessionFile": expected_destination,
                }),
                json!({
                    "type": "session_start",
                    "reason": "resume",
                    "previousSessionFile": current_file,
                }),
            ]
        );
        assert_eq!(backend.messages().expect("messages")[0].content, "imported");
    }

    #[test]
    fn new_session_honors_session_before_switch_cancellation_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/cancel.ts".to_string(),
                Box::new(CancelSwitchFactory {
                    events: events.clone(),
                    cancel_reason: "new",
                }),
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let session_manager =
            SessionManager::create("/tmp/project", Some(dir)).expect("session should create");
        let original_file = session_manager
            .session_file()
            .expect("session file")
            .to_string_lossy()
            .to_string();
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session_manager, model_registry);
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();

        let result = backend.new_session(None).expect("new session");

        assert_eq!(result["cancelled"], true);
        assert!(
            loader.extensions().runtime.assert_active().is_ok(),
            "cancelled session switch should keep extension runtime active"
        );
        assert_eq!(
            backend.state().expect("state").session_file.as_deref(),
            Some(original_file.as_str())
        );
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![json!({
                "type": "session_before_switch",
                "reason": "new",
            })]
        );
    }

    #[test]
    fn switch_session_honors_session_before_switch_cancellation_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/cancel.ts".to_string(),
                Box::new(CancelSwitchFactory {
                    events: events.clone(),
                    cancel_reason: "resume",
                }),
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let mut first =
            SessionManager::create("/tmp/first", Some(dir.clone())).expect("first session");
        first
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "first".to_string(),
            ))
            .expect("first message");
        let first_file = first
            .session_file()
            .expect("first file")
            .to_string_lossy()
            .to_string();
        let second = SessionManager::create("/tmp/second", Some(dir)).expect("second session");
        let second_file = second
            .session_file()
            .expect("second file")
            .to_string_lossy()
            .to_string();
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(first, model_registry);
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();

        let result = backend
            .switch_session(second_file.clone())
            .expect("switch session");

        assert_eq!(result["cancelled"], true);
        assert_eq!(
            backend.state().expect("state").session_file.as_deref(),
            Some(first_file.as_str())
        );
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![json!({
                "type": "session_before_switch",
                "reason": "resume",
                "targetSessionFile": second_file,
            })]
        );
    }

    #[test]
    fn import_session_honors_session_before_switch_cancellation_like_pi() {
        let dir = temp_dir();
        let source_dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/cancel.ts".to_string(),
                Box::new(CancelSwitchFactory {
                    events: events.clone(),
                    cancel_reason: "resume",
                }),
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let current =
            SessionManager::create("/tmp/current", Some(dir.clone())).expect("current session");
        let current_file = current
            .session_file()
            .expect("current file")
            .to_string_lossy()
            .to_string();
        let source =
            SessionManager::create("/tmp/source", Some(source_dir)).expect("source session");
        let source_file = source.session_file().expect("source file").to_path_buf();
        let expected_destination = dir
            .join(source_file.file_name().expect("source basename"))
            .to_string_lossy()
            .to_string();
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(current, model_registry);
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();

        let result = backend
            .import_session(source_file.to_string_lossy().to_string(), None)
            .expect("import session");

        assert_eq!(result["cancelled"], true);
        assert_eq!(
            backend.state().expect("state").session_file.as_deref(),
            Some(current_file.as_str())
        );
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![json!({
                "type": "session_before_switch",
                "reason": "resume",
                "targetSessionFile": expected_destination,
            })]
        );
    }

    #[test]
    fn fork_emits_before_fork_shutdown_and_start_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/lifecycle.ts".to_string(),
                Box::new(LifecycleFactory {
                    events: events.clone(),
                }),
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let session_manager =
            SessionManager::create("/tmp/project", Some(dir)).expect("session should create");
        let original_file = session_manager
            .session_file()
            .expect("session file")
            .to_string_lossy()
            .to_string();
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session_manager, model_registry);
        backend.prompt("first".to_string()).expect("first prompt");
        backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::Assistant,
                "answer".to_string(),
            ))
            .expect("assistant message");
        let fork_entry = backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "second".to_string(),
            ))
            .expect("second message");
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();

        let result = backend
            .fork(fork_entry.clone(), crate::rpc::types::ForkPosition::Before)
            .expect("fork");
        backend.apply_loaded_resources(&loader);

        assert_eq!(result["cancelled"], false);
        assert_eq!(result["text"], "second");
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![
                json!({
                    "type": "session_before_fork",
                    "entryId": fork_entry,
                    "position": "before",
                }),
                json!({
                    "type": "session_shutdown",
                    "reason": "fork",
                    "targetSessionFile": original_file,
                }),
                json!({
                    "type": "session_start",
                    "reason": "fork",
                    "previousSessionFile": original_file,
                }),
            ]
        );
    }

    #[test]
    fn fork_at_entry_emits_position_at_and_keeps_selected_entry_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/lifecycle.ts".to_string(),
                Box::new(LifecycleFactory {
                    events: events.clone(),
                }),
            )],
            no_extensions: false,
            no_skills: true,
            no_prompt_templates: true,
            no_themes: true,
            no_context_files: true,
            system_prompt: None,
            append_system_prompt: Vec::new(),
        });
        loader.reload().expect("reload should succeed");
        let session_manager =
            SessionManager::create("/tmp/project", Some(dir)).expect("session should create");
        let original_file = session_manager
            .session_file()
            .expect("session file")
            .to_string_lossy()
            .to_string();
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session_manager, model_registry);
        backend.prompt("first".to_string()).expect("first prompt");
        let assistant_entry = backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::Assistant,
                "answer".to_string(),
            ))
            .expect("assistant message");
        backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "second".to_string(),
            ))
            .expect("second message");
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();

        let result = backend
            .fork(assistant_entry.clone(), crate::rpc::types::ForkPosition::At)
            .expect("fork");
        let forked_file = backend
            .state()
            .expect("state")
            .session_file
            .expect("forked session file");
        backend.apply_loaded_resources(&loader);

        assert_eq!(result["cancelled"], false);
        assert!(result.get("text").is_none());
        assert_eq!(
            backend.messages().expect("messages"),
            vec![
                agent::AgentMessage::new(ai::MessageRole::User, "first".to_string()),
                agent::AgentMessage::new(ai::MessageRole::Assistant, "answer".to_string()),
            ]
        );
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![
                json!({
                    "type": "session_before_fork",
                    "entryId": assistant_entry,
                    "position": "at",
                }),
                json!({
                    "type": "session_shutdown",
                    "reason": "fork",
                    "targetSessionFile": forked_file,
                }),
                json!({
                    "type": "session_start",
                    "reason": "fork",
                    "previousSessionFile": original_file,
                }),
            ]
        );
    }

    #[test]
    fn fork_honors_session_before_fork_cancellation_like_pi() {
        let dir = temp_dir();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut loader = DefaultResourceLoader::new(DefaultResourceLoaderOptions {
            cwd: dir.clone(),
            agent_dir: dir.clone(),
            user_agents_dir: Some(temp_dir().join(".agents")),
            settings_manager: SettingsManager::<InMemorySettingsStorage>::in_memory(json!({})),
            package_runner: None,
            additional_extension_paths: Vec::new(),
            additional_skill_paths: Vec::new(),
            additional_prompt_paths: Vec::new(),
            additional_theme_paths: Vec::new(),
            extension_factories: vec![(
                "/extensions/cancel-fork.ts".to_string(),
                Box::new(CancelForkFactory {
                    events: events.clone(),
                }),
            )],
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
        backend.prompt("first".to_string()).expect("first prompt");
        backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::Assistant,
                "answer".to_string(),
            ))
            .expect("assistant message");
        let fork_entry = backend
            .session_manager_mut()
            .append_message(agent::AgentMessage::new(
                ai::MessageRole::User,
                "second".to_string(),
            ))
            .expect("second message");
        backend.apply_loaded_resources(&loader);
        events.lock().expect("events lock").clear();

        let result = backend
            .fork(fork_entry.clone(), crate::rpc::types::ForkPosition::Before)
            .expect("fork");

        assert_eq!(result["cancelled"], true);
        assert_eq!(backend.messages().expect("messages").len(), 3);
        assert_eq!(
            *events.lock().expect("events lock"),
            vec![json!({
                "type": "session_before_fork",
                "entryId": fork_entry,
                "position": "before",
            })]
        );
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
