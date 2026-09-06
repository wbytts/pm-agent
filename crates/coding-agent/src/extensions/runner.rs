use agent::AgentMessage;
use serde_json::Value;

use super::types::{
    Extension, ExtensionCommandContext, ExtensionContext, ExtensionError, ExtensionEvent,
    ExtensionFlag, ExtensionRuntime, LoadExtensionsResult, PendingProviderAction,
    RegisteredCommand, RegisteredTool, ToolDefinition,
};
use super::{
    emit_before_agent_start, emit_before_provider_request, emit_context, emit_input,
    emit_message_end, emit_resources_discover, emit_tool_call, emit_tool_result, emit_user_bash,
    find_tool_definition, resolve_flags, resolve_registered_commands, resolve_registered_tools,
    to_model_provider_config, BeforeAgentStartEvent, BeforeAgentStartResult,
    DiscoveredExtensionResources, ExtensionToolCallEvent, ExtensionToolResultEvent,
    InputEventResult, InputSource, ResolvedCommand, ResourcesDiscoverReason, ToolCallDecision,
    ToolResultUpdate, UserBashEvent, UserBashResult,
};
use crate::auth_storage::AuthStorageBackend;
use crate::model_registry::ModelRegistry;
use crate::slash_commands::{extension_commands, SlashCommandInfo};
use ai::ProviderRegistry;
use std::collections::BTreeMap;

pub struct ExtensionRunner {
    extensions: Vec<Extension>,
    runtime: ExtensionRuntime,
    error_listeners: Vec<Box<dyn Fn(&ExtensionError) + Send + Sync>>,
}

impl ExtensionRunner {
    pub fn new(extensions: Vec<Extension>, runtime: ExtensionRuntime) -> Self {
        Self {
            extensions,
            runtime,
            error_listeners: Vec::new(),
        }
    }

    pub fn from_loaded_resources(result: &LoadExtensionsResult) -> Self {
        Self::new(result.extensions.clone(), result.runtime.clone())
    }

    pub fn extensions(&self) -> &[Extension] {
        &self.extensions
    }

    pub fn runtime(&self) -> &ExtensionRuntime {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut ExtensionRuntime {
        &mut self.runtime
    }

    pub fn invalidate(&self, message: Option<String>) {
        self.runtime.invalidate(message);
    }

    pub fn create_context(&self, extension: &Extension) -> ExtensionContext {
        ExtensionContext {
            extension_path: extension.path.clone(),
            source_info: extension.source_info.clone(),
        }
    }

    pub fn on_error(&mut self, listener: impl Fn(&ExtensionError) + Send + Sync + 'static) {
        self.error_listeners.push(Box::new(listener));
    }

    pub fn has_handlers(&self, event: &str) -> bool {
        self.extensions.iter().any(|extension| {
            extension
                .handlers
                .get(event)
                .is_some_and(|handlers| !handlers.is_empty())
        })
    }

    pub fn emit(&self, event_name: &str, event: ExtensionEvent) -> Vec<Value> {
        let mut results = Vec::new();
        for extension in &self.extensions {
            let Some(handlers) = extension.handlers.get(event_name) else {
                continue;
            };
            for handler in handlers {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handler(event.clone())
                })) {
                    Ok(Some(value)) => results.push(value),
                    Ok(None) => {}
                    Err(_) => self.report_error(ExtensionError {
                        extension_path: extension.path.clone(),
                        event: Some(event_name.to_string()),
                        message: "Extension handler panicked".to_string(),
                    }),
                }
            }
        }
        results
    }

    pub fn emit_resources_discover(
        &self,
        cwd: &str,
        reason: ResourcesDiscoverReason,
    ) -> DiscoveredExtensionResources {
        emit_resources_discover(&self.extensions, cwd, reason, |error| {
            self.report_error(error);
        })
    }

    pub fn emit_message_end(&self, message: AgentMessage) -> Option<AgentMessage> {
        emit_message_end(&self.extensions, message, |error| self.report_error(error))
    }

    pub fn emit_context(&self, messages: Vec<AgentMessage>) -> Vec<AgentMessage> {
        emit_context(&self.extensions, messages, |error| self.report_error(error))
    }

    pub fn emit_before_provider_request(&self, payload: Value) -> Value {
        emit_before_provider_request(&self.extensions, payload, |error| {
            self.report_error(error);
        })
    }

    pub fn emit_before_agent_start(
        &self,
        event: BeforeAgentStartEvent,
    ) -> Option<BeforeAgentStartResult> {
        emit_before_agent_start(&self.extensions, event, |error| self.report_error(error))
    }

    pub fn emit_input(
        &self,
        text: impl Into<String>,
        images: Option<Value>,
        source: InputSource,
    ) -> InputEventResult {
        emit_input(&self.extensions, text, images, source, |error| {
            self.report_error(error);
        })
    }

    pub fn emit_user_bash(&self, event: UserBashEvent) -> Option<UserBashResult> {
        emit_user_bash(&self.extensions, event, |error| self.report_error(error))
    }

    pub fn emit_tool_call(&self, event: ExtensionToolCallEvent) -> Option<ToolCallDecision> {
        emit_tool_call(&self.extensions, event, |error| self.report_error(error))
    }

    pub fn emit_tool_result(&self, event: ExtensionToolResultEvent) -> Option<ToolResultUpdate> {
        emit_tool_result(&self.extensions, event, |error| self.report_error(error))
    }

    pub fn registered_tools(&self) -> Vec<RegisteredTool> {
        resolve_registered_tools(&self.extensions)
    }

    pub fn tool_definition(&self, tool_name: &str) -> Option<ToolDefinition> {
        find_tool_definition(&self.extensions, tool_name)
    }

    pub fn flags(&self) -> BTreeMap<String, ExtensionFlag> {
        resolve_flags(&self.extensions)
    }

    pub fn set_flag_value(&mut self, name: impl Into<String>, value: Value) {
        self.runtime.set_flag_value(name, value);
    }

    pub fn flag_values(&self) -> BTreeMap<String, Value> {
        self.runtime.flag_values()
    }

    pub fn registered_commands(&self) -> Vec<RegisteredCommand> {
        self.extensions
            .iter()
            .flat_map(|extension| extension.commands.values().cloned())
            .collect()
    }

    pub fn resolved_commands(&self) -> Vec<ResolvedCommand> {
        resolve_registered_commands(&self.extensions)
    }

    pub fn registered_command(&self, name: &str) -> Option<RegisteredCommand> {
        self.resolved_commands()
            .into_iter()
            .find(|command| command.invocation_name == name)
            .map(|command| command.command)
    }

    pub fn registered_slash_commands(&self) -> Vec<SlashCommandInfo> {
        extension_commands(&self.resolved_commands())
    }

    pub fn run_command(
        &self,
        command: &RegisteredCommand,
        args: Vec<String>,
    ) -> Result<(), String> {
        let ctx = ExtensionCommandContext {
            extension_path: command.source_info.path.clone(),
            command_name: command.name.clone(),
            args,
            source_info: command.source_info.clone(),
        };
        (command.handler)(ctx)
    }

    pub fn run_command_by_name(&self, name: &str, args: Vec<String>) -> Result<bool, String> {
        let Some(command) = self.registered_command(name) else {
            return Ok(false);
        };
        self.run_command(&command, args)?;
        Ok(true)
    }

    pub fn flush_pending_provider_registrations<B: AuthStorageBackend>(
        &mut self,
        model_registry: &mut ModelRegistry<B>,
    ) {
        self.flush_pending_provider_registrations_inner(model_registry, None);
    }

    pub fn flush_pending_provider_registrations_with_api_providers<B: AuthStorageBackend>(
        &mut self,
        model_registry: &mut ModelRegistry<B>,
        provider_registry: &mut ProviderRegistry,
    ) {
        self.flush_pending_provider_registrations_inner(model_registry, Some(provider_registry));
    }

    fn flush_pending_provider_registrations_inner<B: AuthStorageBackend>(
        &mut self,
        model_registry: &mut ModelRegistry<B>,
        mut provider_registry: Option<&mut ProviderRegistry>,
    ) {
        let pending = self.runtime.take_pending_provider_actions();
        for action in pending {
            match action {
                PendingProviderAction::Register(registration) => {
                    let config = to_model_provider_config(registration.config);
                    match model_registry.try_register_provider(registration.name.clone(), config) {
                        Ok(()) => {
                            if let Some(provider_registry) = provider_registry.as_deref_mut() {
                                model_registry.apply_registered_api_providers(provider_registry);
                            }
                        }
                        Err(message) => {
                            self.report_error(ExtensionError {
                                extension_path: registration.extension_path,
                                event: Some("register_provider".to_string()),
                                message,
                            });
                        }
                    }
                }
                PendingProviderAction::Unregister {
                    name,
                    extension_path: _,
                } => {
                    if let Some(provider_registry) = provider_registry.as_deref_mut() {
                        model_registry.unregister_api_provider(provider_registry, &name);
                    }
                    model_registry.unregister_provider(&name);
                }
            }
        }
    }

    pub fn unregister_provider<B: AuthStorageBackend>(
        &self,
        model_registry: &mut ModelRegistry<B>,
        provider: &str,
    ) {
        model_registry.unregister_provider(provider);
    }

    pub fn unregister_provider_with_api_providers<B: AuthStorageBackend>(
        &self,
        model_registry: &mut ModelRegistry<B>,
        provider_registry: &mut ProviderRegistry,
        provider: &str,
    ) {
        model_registry.unregister_api_provider(provider_registry, provider);
        model_registry.unregister_provider(provider);
    }

    fn report_error(&self, error: ExtensionError) {
        for listener in &self.error_listeners {
            listener(&error);
        }
    }
}

pub fn emit_session_shutdown_event(
    extension_runner: &ExtensionRunner,
    event: ExtensionEvent,
) -> bool {
    if extension_runner.has_handlers("session_shutdown") {
        extension_runner.emit("session_shutdown", event);
        return true;
    }
    false
}
