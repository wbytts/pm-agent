use std::path::Path;

use crate::source_info::create_synthetic_source_info;

use super::types::{
    Extension, ExtensionApi, ExtensionError, ExtensionFactory, ExtensionRuntime,
    LoadExtensionsResult,
};

pub fn create_extension_runtime() -> ExtensionRuntime {
    ExtensionRuntime::default()
}

pub fn load_extension_from_factory(
    path: impl Into<String>,
    factory: &dyn ExtensionFactory,
    runtime: &mut ExtensionRuntime,
) -> Result<Extension, ExtensionError> {
    let path = path.into();
    let source_info = create_synthetic_source_info(
        path.clone(),
        "local",
        None,
        None,
        Path::new(&path)
            .parent()
            .map(|parent| parent.to_string_lossy().to_string()),
    );
    let mut extension = Extension::new(path.clone(), source_info);
    let mut api = ExtensionApi::new(&mut extension, runtime);
    factory.load(&mut api).map_err(|message| ExtensionError {
        extension_path: path.clone(),
        event: None,
        message,
    })?;
    Ok(extension)
}

pub fn load_extensions(
    factories: Vec<(String, Box<dyn ExtensionFactory>)>,
    runtime: &mut ExtensionRuntime,
) -> LoadExtensionsResult {
    let mut result = LoadExtensionsResult::default();
    for (path, factory) in factories {
        match load_extension_from_factory(path, factory.as_ref(), runtime) {
            Ok(extension) => result.extensions.push(extension),
            Err(error) => result.errors.push(error),
        }
    }
    result
}

pub fn discover_and_load_extensions(
    paths: &[String],
    runtime: &mut ExtensionRuntime,
) -> LoadExtensionsResult {
    let mut result = LoadExtensionsResult::default();
    for path in paths {
        if !Path::new(path).exists() {
            result.errors.push(ExtensionError {
                extension_path: path.clone(),
                event: None,
                message: "Extension file does not exist".to_string(),
            });
            continue;
        }
        // Rust 侧不能直接执行 TS 扩展；这里先登记一个空扩展，后续接 JS/TS runtime。
        let source_info = create_synthetic_source_info(
            path.clone(),
            "local",
            None,
            None,
            Path::new(path)
                .parent()
                .map(|parent| parent.to_string_lossy().to_string()),
        );
        result
            .extensions
            .push(Extension::new(path.clone(), source_info));
    }
    let _ = runtime;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_storage::{AuthStorage, AuthStorageData, InMemoryAuthStorageBackend};
    use crate::extensions::types::{
        CommandHandler, ExecutableToolDefinition, ExtensionEvent, ExtensionEventKind,
        ExtensionFactory, ProviderConfig, ProviderModelConfig, ToolDefinition, ToolExecutor,
    };
    use crate::model_registry::ModelRegistry;
    use serde_json::{json, Value};
    use std::sync::Arc;

    struct DemoFactory;

    impl ExtensionFactory for DemoFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.on(
                "turn_start",
                Arc::new(|event: ExtensionEvent| Some(json!({"seen": event.payload}))),
            )?;
            let command: CommandHandler = Arc::new(|ctx| {
                if ctx.command_name == "demo" {
                    Ok(())
                } else {
                    Err("wrong command".to_string())
                }
            });
            api.register_command("demo", Some("Demo command".to_string()), command)?;
            let execute: ToolExecutor = Arc::new(|input: Value, _ctx| Ok(json!({"echo": input})));
            api.register_tool(ExecutableToolDefinition {
                definition: ToolDefinition {
                    name: "demo_tool".to_string(),
                    label: Some("demo".to_string()),
                    description: "Demo tool".to_string(),
                    prompt_snippet: None,
                    parameters: json!({"type":"object"}),
                },
                execute,
            })?;
            Ok(())
        }
    }

    struct DuplicateToolFactory {
        description: &'static str,
    }

    impl ExtensionFactory for DuplicateToolFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            let execute: ToolExecutor = Arc::new(|input: Value, _ctx| Ok(input));
            api.register_tool(ExecutableToolDefinition {
                definition: ToolDefinition {
                    name: "demo_tool".to_string(),
                    label: None,
                    description: self.description.to_string(),
                    prompt_snippet: None,
                    parameters: json!({"type":"object"}),
                },
                execute,
            })?;
            Ok(())
        }
    }

    struct ProviderFactory;

    impl ExtensionFactory for ProviderFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.register_provider(
                "demo-provider",
                ProviderConfig {
                    display_name: Some("Demo Provider".to_string()),
                    models: vec![ProviderModelConfig {
                        id: "demo-model".to_string(),
                        display_name: Some("Demo Model".to_string()),
                        api: Some("openai-chat-completions".to_string()),
                    }],
                },
            )
        }
    }

    struct FlagFactory {
        description: &'static str,
    }

    impl ExtensionFactory for FlagFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.register_flag(crate::extensions::types::ExtensionFlag {
                name: "demo-flag".to_string(),
                description: Some(self.description.to_string()),
            })
        }
    }

    struct ResourcesFactory;

    impl ExtensionFactory for ResourcesFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.on(
                "resources_discover",
                Arc::new(|event: ExtensionEvent| {
                    assert_eq!(event.payload["reason"], "reload");
                    Some(json!({
                        "skillPaths": ["/skills/from-extension"],
                        "promptPaths": ["/prompts/from-extension.md"],
                        "themePaths": ["/themes/from-extension.json"],
                    }))
                }),
            )
        }
    }

    struct MessageEndFactory;

    impl ExtensionFactory for MessageEndFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.on(
                "message_end",
                Arc::new(|event: ExtensionEvent| {
                    let mut message = event.payload["message"].clone();
                    message["content"] = json!("rewritten by extension");
                    Some(json!({ "message": message }))
                }),
            )
        }
    }

    struct ToolResultFactory;

    impl ExtensionFactory for ToolResultFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.on(
                "tool_result",
                Arc::new(|event: ExtensionEvent| {
                    assert_eq!(event.payload["type"], "tool_result");
                    Some(json!({
                        "content": [{ "type": "text", "text": "rewritten by extension" }],
                        "details": { "source": "extension" },
                        "isError": false
                    }))
                }),
            )
        }
    }

    struct ToolCallFactory;

    impl ExtensionFactory for ToolCallFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.on(
                "tool_call",
                Arc::new(|event: ExtensionEvent| {
                    assert_eq!(event.payload["type"], "tool_call");
                    assert_eq!(event.payload["toolName"], "bash");
                    Some(json!({
                        "block": true,
                        "reason": "blocked by extension"
                    }))
                }),
            )
        }
    }

    struct InputFactory;

    impl ExtensionFactory for InputFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.on(
                "input",
                Arc::new(|event: ExtensionEvent| {
                    assert_eq!(event.payload["type"], "input");
                    Some(json!({
                        "action": "transform",
                        "text": format!("{} + extension", event.payload["text"].as_str().unwrap_or_default())
                    }))
                }),
            )
        }
    }

    struct UserBashFactory;

    impl ExtensionFactory for UserBashFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.on(
                "user_bash",
                Arc::new(|event: ExtensionEvent| {
                    assert_eq!(event.payload["type"], "user_bash");
                    Some(json!({ "result": { "stdout": event.payload["command"] } }))
                }),
            )
        }
    }

    struct ContextFactory;

    impl ExtensionFactory for ContextFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.on(
                "context",
                Arc::new(|event: ExtensionEvent| {
                    assert_eq!(event.payload["type"], "context");
                    Some(json!({
                        "messages": [{ "role": "User", "content": "context from extension" }]
                    }))
                }),
            )
        }
    }

    struct BeforeProviderRequestFactory;

    impl ExtensionFactory for BeforeProviderRequestFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.on(
                "before_provider_request",
                Arc::new(|event: ExtensionEvent| {
                    assert_eq!(event.payload["type"], "before_provider_request");
                    Some(json!({ "payload": "rewritten by extension" }))
                }),
            )
        }
    }

    struct BeforeAgentStartFactory;

    impl ExtensionFactory for BeforeAgentStartFactory {
        fn load(&self, api: &mut crate::extensions::types::ExtensionApi<'_>) -> Result<(), String> {
            api.on(
                "before_agent_start",
                Arc::new(|event: ExtensionEvent| {
                    assert_eq!(event.payload["type"], "before_agent_start");
                    Some(json!({
                        "message": { "customType": "notice", "content": event.payload["prompt"] },
                        "systemPrompt": "rewritten system"
                    }))
                }),
            )
        }
    }

    #[test]
    fn loads_extension_from_factory() {
        let mut runtime = create_extension_runtime();
        let extension = load_extension_from_factory("/tmp/demo.ts", &DemoFactory, &mut runtime)
            .expect("extension should load");
        assert!(extension.handlers.contains_key("turn_start"));
        assert!(extension.commands.contains_key("demo"));
        assert!(extension.tools.contains_key("demo_tool"));
    }

    #[test]
    fn runner_emits_events_and_runs_commands() {
        let mut runtime = create_extension_runtime();
        let extension = load_extension_from_factory("/tmp/demo.ts", &DemoFactory, &mut runtime)
            .expect("extension should load");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);
        let results = runner.emit(
            "turn_start",
            ExtensionEvent {
                kind: ExtensionEventKind::TurnStart,
                payload: json!({"id":1}),
            },
        );
        assert_eq!(results[0]["seen"]["id"], 1);
        let mut commands = runner.registered_commands();
        let command = commands.remove(0);
        let slash_commands = runner.registered_slash_commands();
        assert_eq!(slash_commands[0].name, "demo");
        assert_eq!(
            slash_commands[0].source,
            crate::slash_commands::SlashCommandSource::Extension
        );
        assert!(runner
            .registered_command("demo")
            .is_some_and(|command| command.name == "demo"));
        runner
            .run_command(&command, Vec::new())
            .expect("command should run");
        assert!(runner
            .run_command_by_name("demo", Vec::new())
            .expect("named command should run"));
        assert!(!runner
            .run_command_by_name("missing", Vec::new())
            .expect("missing command should not run"));
        let mut tools = runner.registered_tools();
        let tool = tools.remove(0);
        let wrapped = crate::extensions::wrapper::wrap_registered_tool(&tool, &runner);
        assert_eq!(wrapped.definition.name, "demo_tool");
        let output =
            crate::extensions::wrapper::execute_registered_tool(&tool, &runner, json!({"a":1}))
                .expect("tool should execute");
        assert_eq!(output["echo"]["a"], 1);
    }

    #[test]
    fn runner_registered_tools_use_first_registration_like_pi() {
        let mut runtime = create_extension_runtime();
        let first = load_extension_from_factory(
            "/tmp/first.ts",
            &DuplicateToolFactory {
                description: "first",
            },
            &mut runtime,
        )
        .expect("first extension");
        let second = load_extension_from_factory(
            "/tmp/second.ts",
            &DuplicateToolFactory {
                description: "second",
            },
            &mut runtime,
        )
        .expect("second extension");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![first, second], runtime);

        let tools = runner.registered_tools();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].definition.definition.description, "first");
        assert_eq!(
            runner
                .tool_definition("demo_tool")
                .expect("tool definition")
                .description,
            "first"
        );
    }

    #[test]
    fn runner_flushes_pending_provider_registrations_like_pi() {
        let mut runtime = create_extension_runtime();
        let extension =
            load_extension_from_factory("/tmp/provider.ts", &ProviderFactory, &mut runtime)
                .expect("provider extension should load");
        assert_eq!(runtime.pending_provider_registrations.len(), 1);
        let mut runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);
        let storage = AuthStorage::<InMemoryAuthStorageBackend>::in_memory(AuthStorageData::new());
        let mut registry = ModelRegistry::in_memory(storage);
        assert!(registry.find("demo-provider", "demo-model").is_none());

        runner.flush_pending_provider_registrations(&mut registry);

        assert!(runner.runtime().pending_provider_registrations.is_empty());
        let model = registry
            .find("demo-provider", "demo-model")
            .expect("registered model should be available");
        assert_eq!(model.display_name, "Demo Model");

        runner.unregister_provider(&mut registry, "demo-provider");

        assert!(registry.find("demo-provider", "demo-model").is_none());
    }

    #[test]
    fn runner_flags_use_first_registration_and_store_values_like_pi() {
        let mut runtime = create_extension_runtime();
        let first = load_extension_from_factory(
            "/tmp/first-flag.ts",
            &FlagFactory {
                description: "first",
            },
            &mut runtime,
        )
        .expect("first flag extension");
        let second = load_extension_from_factory(
            "/tmp/second-flag.ts",
            &FlagFactory {
                description: "second",
            },
            &mut runtime,
        )
        .expect("second flag extension");
        let mut runner =
            crate::extensions::runner::ExtensionRunner::new(vec![first, second], runtime);

        let flags = runner.flags();

        assert_eq!(flags.len(), 1);
        assert_eq!(
            flags
                .get("demo-flag")
                .and_then(|flag| flag.description.as_deref()),
            Some("first")
        );

        runner.set_flag_value("demo-flag", json!(true));

        assert_eq!(runner.flag_values()["demo-flag"], json!(true));
    }

    #[test]
    fn runner_emits_resources_discover_like_pi() {
        let mut runtime = create_extension_runtime();
        let extension =
            load_extension_from_factory("/tmp/resources.ts", &ResourcesFactory, &mut runtime)
                .expect("resources extension");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);

        let resources = runner.emit_resources_discover(
            "/tmp/project",
            crate::extensions::ResourcesDiscoverReason::Reload,
        );

        assert_eq!(resources.skill_paths[0].path, "/skills/from-extension");
        assert_eq!(resources.skill_paths[0].extension_path, "/tmp/resources.ts");
        assert_eq!(resources.prompt_paths[0].path, "/prompts/from-extension.md");
        assert_eq!(resources.theme_paths[0].path, "/themes/from-extension.json");
    }

    #[test]
    fn runner_emits_message_end_like_pi() {
        let mut runtime = create_extension_runtime();
        let extension =
            load_extension_from_factory("/tmp/message.ts", &MessageEndFactory, &mut runtime)
                .expect("message extension");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);

        let message = runner
            .emit_message_end(agent::AgentMessage::new(
                ai::MessageRole::Assistant,
                "original".to_string(),
            ))
            .expect("message should be modified");

        assert_eq!(message.role, ai::MessageRole::Assistant);
        assert_eq!(message.content, "rewritten by extension");
    }

    #[test]
    fn runner_emits_tool_result_like_pi() {
        let mut runtime = create_extension_runtime();
        let extension =
            load_extension_from_factory("/tmp/tool-result.ts", &ToolResultFactory, &mut runtime)
                .expect("tool result extension");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);

        let update = runner
            .emit_tool_result(crate::extensions::ExtensionToolResultEvent::new(
                "call-1",
                "bash",
                json!({ "command": "pwd" }),
                json!([{ "type": "text", "text": "original" }]),
                None,
                true,
            ))
            .expect("tool result should be modified");

        assert_eq!(update.content[0]["text"], "rewritten by extension");
        assert_eq!(update.details.expect("details")["source"], "extension");
        assert!(!update.is_error);
    }

    #[test]
    fn runner_emits_tool_call_like_pi() {
        let mut runtime = create_extension_runtime();
        let extension =
            load_extension_from_factory("/tmp/tool-call.ts", &ToolCallFactory, &mut runtime)
                .expect("tool call extension");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);

        let decision = runner
            .emit_tool_call(crate::extensions::ExtensionToolCallEvent::new(
                "call-1",
                "bash",
                json!({ "command": "rm -rf target" }),
            ))
            .expect("tool call should return decision");

        assert!(decision.block);
        assert_eq!(decision.reason.as_deref(), Some("blocked by extension"));
    }

    #[test]
    fn runner_emits_input_like_pi() {
        let mut runtime = create_extension_runtime();
        let extension = load_extension_from_factory("/tmp/input.ts", &InputFactory, &mut runtime)
            .expect("input extension");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);

        let result = runner.emit_input("hello", None, crate::extensions::InputSource::Rpc);

        assert_eq!(
            result,
            crate::extensions::InputEventResult::Transform {
                text: "hello + extension".to_string(),
                images: None,
            }
        );
    }

    #[test]
    fn runner_emits_user_bash_like_pi() {
        let mut runtime = create_extension_runtime();
        let extension =
            load_extension_from_factory("/tmp/user-bash.ts", &UserBashFactory, &mut runtime)
                .expect("user bash extension");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);

        let result = runner
            .emit_user_bash(crate::extensions::UserBashEvent::new(
                "pwd",
                false,
                "/tmp/project",
            ))
            .expect("user bash should be handled");

        assert_eq!(result.value["result"]["stdout"], "pwd");
    }

    #[test]
    fn runner_emits_context_like_pi() {
        let mut runtime = create_extension_runtime();
        let extension =
            load_extension_from_factory("/tmp/context.ts", &ContextFactory, &mut runtime)
                .expect("context extension");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);

        let messages = runner.emit_context(vec![agent::AgentMessage::new(
            ai::MessageRole::User,
            "original".to_string(),
        )]);

        assert_eq!(messages[0].content, "context from extension");
    }

    #[test]
    fn runner_emits_before_provider_request_like_pi() {
        let mut runtime = create_extension_runtime();
        let extension = load_extension_from_factory(
            "/tmp/before-provider-request.ts",
            &BeforeProviderRequestFactory,
            &mut runtime,
        )
        .expect("before provider request extension");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);

        let payload = runner.emit_before_provider_request(json!({ "payload": "original" }));

        assert_eq!(payload["payload"], "rewritten by extension");
    }

    #[test]
    fn runner_emits_before_agent_start_like_pi() {
        let mut runtime = create_extension_runtime();
        let extension = load_extension_from_factory(
            "/tmp/before-agent-start.ts",
            &BeforeAgentStartFactory,
            &mut runtime,
        )
        .expect("before agent start extension");
        let runner = crate::extensions::runner::ExtensionRunner::new(vec![extension], runtime);

        let result = runner
            .emit_before_agent_start(crate::extensions::BeforeAgentStartEvent::new(
                "hello",
                None,
                "original system",
                json!({ "cwd": "/tmp/project" }),
            ))
            .expect("before agent start should be modified");

        assert_eq!(result.messages.expect("messages")[0]["content"], "hello");
        assert_eq!(result.system_prompt.as_deref(), Some("rewritten system"));
    }
}
