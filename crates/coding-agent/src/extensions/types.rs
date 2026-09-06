use crate::source_info::SourceInfo;
use ai::{ModelCost, ModelInputKind, StreamEvent, StreamRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub type ExtensionHandler = Arc<dyn Fn(ExtensionEvent) -> Option<Value> + Send + Sync>;
pub type CommandHandler = Arc<dyn Fn(ExtensionCommandContext) -> Result<(), String> + Send + Sync>;
pub type ToolExecutor = Arc<dyn Fn(Value, ExtensionContext) -> Result<Value, String> + Send + Sync>;
pub type ProviderStreamHandler =
    Arc<dyn Fn(StreamRequest) -> ai::AiResult<Vec<StreamEvent>> + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub label: Option<String>,
    pub description: String,
    pub prompt_snippet: Option<String>,
    pub parameters: Value,
}

#[derive(Clone)]
pub struct ExecutableToolDefinition {
    pub definition: ToolDefinition,
    pub execute: ToolExecutor,
}

#[derive(Clone)]
pub struct RegisteredTool {
    pub definition: ExecutableToolDefinition,
    pub source_info: SourceInfo,
}

#[derive(Clone)]
pub struct RegisteredCommand {
    pub name: String,
    pub description: Option<String>,
    pub handler: CommandHandler,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionFlagType {
    #[default]
    Boolean,
    String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionFlag {
    pub name: String,
    #[serde(default, rename = "type")]
    pub flag_type: ExtensionFlagType,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelConfig {
    pub id: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub api: Option<String>,
    pub base_url: Option<String>,
    pub reasoning: Option<bool>,
    pub thinking_level_map: Option<BTreeMap<String, Option<String>>>,
    pub input: Option<Vec<ModelInputKind>>,
    pub cost: Option<ModelCost>,
    pub context_window: Option<usize>,
    pub max_tokens: Option<usize>,
    pub headers: Option<BTreeMap<String, String>>,
    pub compat: Option<BTreeMap<String, Value>>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub api: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub auth_header: Option<bool>,
    pub compat: Option<BTreeMap<String, Value>>,
    pub models: Option<Vec<ProviderModelConfig>>,
    #[serde(skip)]
    pub stream_simple: Option<ProviderStreamHandler>,
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("name", &self.name)
            .field("display_name", &self.display_name)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key)
            .field("api", &self.api)
            .field("headers", &self.headers)
            .field("auth_header", &self.auth_header)
            .field("compat", &self.compat)
            .field("models", &self.models)
            .field(
                "stream_simple",
                &self.stream_simple.as_ref().map(|_| "<handler>"),
            )
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionEventKind {
    ResourcesDiscover,
    Input,
    Context,
    BeforeProviderRequest,
    BeforeAgentStart,
    SessionStart,
    SessionBeforeSwitch,
    SessionBeforeFork,
    SessionBeforeCompact,
    SessionCompact,
    SessionBeforeTree,
    SessionTree,
    SessionShutdown,
    TurnStart,
    TurnEnd,
    AgentStart,
    AgentEnd,
    MessageStart,
    MessageUpdate,
    MessageEnd,
    ToolCall,
    ToolResult,
    ToolExecutionStart,
    ToolExecutionUpdate,
    ToolExecutionEnd,
    UserBash,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtensionEvent {
    pub kind: ExtensionEventKind,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionError {
    pub extension_path: String,
    pub event: Option<String>,
    pub message: String,
}

#[derive(Clone)]
pub struct Extension {
    pub path: String,
    pub source_info: SourceInfo,
    pub handlers: BTreeMap<String, Vec<ExtensionHandler>>,
    pub tools: BTreeMap<String, RegisteredTool>,
    pub commands: BTreeMap<String, RegisteredCommand>,
    pub flags: BTreeMap<String, ExtensionFlag>,
}

impl Extension {
    pub fn new(path: impl Into<String>, source_info: SourceInfo) -> Self {
        Self {
            path: path.into(),
            source_info,
            handlers: BTreeMap::new(),
            tools: BTreeMap::new(),
            commands: BTreeMap::new(),
            flags: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingProviderRegistration {
    pub name: String,
    pub config: ProviderConfig,
    pub extension_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PendingProviderAction {
    Register(PendingProviderRegistration),
    Unregister {
        name: String,
        extension_path: String,
    },
}

#[derive(Debug, Default)]
struct ExtensionRuntimeState {
    flag_values: BTreeMap<String, Value>,
    pending_provider_actions: Vec<PendingProviderAction>,
    stale_message: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionRuntime {
    state: Arc<Mutex<ExtensionRuntimeState>>,
}

pub const STALE_EXTENSION_CONTEXT_MESSAGE: &str = "This extension ctx is stale after session replacement or reload. Do not use a captured pi or command ctx after ctx.newSession(), ctx.fork(), ctx.switchSession(), or ctx.reload(). For newSession, fork, and switchSession, move post-replacement work into withSession and use the ctx passed to withSession. For reload, do not use the old ctx after await ctx.reload().";

impl ExtensionRuntime {
    pub fn assert_active(&self) -> Result<(), String> {
        let state = self.state.lock().expect("extension runtime lock poisoned");
        if let Some(message) = &state.stale_message {
            return Err(message.clone());
        }
        Ok(())
    }

    pub fn invalidate(&self, message: Option<String>) {
        let mut state = self.state.lock().expect("extension runtime lock poisoned");
        state.stale_message =
            Some(message.unwrap_or_else(|| STALE_EXTENSION_CONTEXT_MESSAGE.to_string()));
    }

    pub fn register_provider(
        &self,
        name: impl Into<String>,
        config: ProviderConfig,
        extension_path: impl Into<String>,
    ) {
        self.state
            .lock()
            .expect("extension runtime lock poisoned")
            .pending_provider_actions
            .push(PendingProviderAction::Register(
                PendingProviderRegistration {
                    name: name.into(),
                    config,
                    extension_path: extension_path.into(),
                },
            ));
    }

    pub fn unregister_provider(&self, name: &str, extension_path: impl Into<String>) {
        let mut state = self.state.lock().expect("extension runtime lock poisoned");
        let original_len = state.pending_provider_actions.len();
        state
            .pending_provider_actions
            .retain(|action| match action {
                PendingProviderAction::Register(registration) => registration.name != name,
                PendingProviderAction::Unregister { .. } => true,
            });
        if state.pending_provider_actions.len() == original_len {
            state
                .pending_provider_actions
                .push(PendingProviderAction::Unregister {
                    name: name.to_string(),
                    extension_path: extension_path.into(),
                });
        }
    }

    pub fn set_flag_value(&self, name: impl Into<String>, value: Value) {
        self.state
            .lock()
            .expect("extension runtime lock poisoned")
            .flag_values
            .insert(name.into(), value);
    }

    pub fn flag_values(&self) -> BTreeMap<String, Value> {
        self.state
            .lock()
            .expect("extension runtime lock poisoned")
            .flag_values
            .clone()
    }

    pub fn pending_provider_registrations_len(&self) -> usize {
        self.state
            .lock()
            .expect("extension runtime lock poisoned")
            .pending_provider_actions
            .iter()
            .filter(|action| matches!(action, PendingProviderAction::Register(_)))
            .count()
    }

    pub fn pending_provider_registrations_is_empty(&self) -> bool {
        self.pending_provider_registrations_len() == 0
    }

    pub fn take_pending_provider_actions(&self) -> Vec<PendingProviderAction> {
        std::mem::take(
            &mut self
                .state
                .lock()
                .expect("extension runtime lock poisoned")
                .pending_provider_actions,
        )
    }
}

#[derive(Clone)]
pub struct ExtensionContext {
    pub extension_path: String,
    pub source_info: SourceInfo,
}

#[derive(Clone)]
pub struct ExtensionCommandContext {
    pub extension_path: String,
    pub command_name: String,
    pub args: Vec<String>,
    pub source_info: SourceInfo,
}

pub struct ExtensionApi<'a> {
    extension: &'a mut Extension,
    runtime: &'a mut ExtensionRuntime,
}

impl<'a> ExtensionApi<'a> {
    pub fn new(extension: &'a mut Extension, runtime: &'a mut ExtensionRuntime) -> Self {
        Self { extension, runtime }
    }

    pub fn runtime(&self) -> ExtensionRuntime {
        self.runtime.clone()
    }

    pub fn on(
        &mut self,
        event: impl Into<String>,
        handler: ExtensionHandler,
    ) -> Result<(), String> {
        self.runtime.assert_active()?;
        self.extension
            .handlers
            .entry(event.into())
            .or_default()
            .push(handler);
        Ok(())
    }

    pub fn register_tool(&mut self, tool: ExecutableToolDefinition) -> Result<(), String> {
        self.runtime.assert_active()?;
        self.extension.tools.insert(
            tool.definition.name.clone(),
            RegisteredTool {
                definition: tool,
                source_info: self.extension.source_info.clone(),
            },
        );
        Ok(())
    }

    pub fn register_command(
        &mut self,
        name: impl Into<String>,
        description: Option<String>,
        handler: CommandHandler,
    ) -> Result<(), String> {
        self.runtime.assert_active()?;
        let name = name.into();
        self.extension.commands.insert(
            name.clone(),
            RegisteredCommand {
                name,
                description,
                handler,
                source_info: self.extension.source_info.clone(),
            },
        );
        Ok(())
    }

    pub fn register_flag(&mut self, flag: ExtensionFlag) -> Result<(), String> {
        self.runtime.assert_active()?;
        self.extension.flags.insert(flag.name.clone(), flag);
        Ok(())
    }

    pub fn register_provider(
        &mut self,
        name: impl Into<String>,
        config: ProviderConfig,
    ) -> Result<(), String> {
        self.runtime.assert_active()?;
        self.runtime
            .register_provider(name, config, self.extension.path.clone());
        Ok(())
    }

    pub fn unregister_provider(&mut self, name: &str) -> Result<(), String> {
        self.runtime.assert_active()?;
        self.runtime
            .unregister_provider(name, self.extension.path.clone());
        Ok(())
    }
}

pub trait ExtensionFactory: Send + Sync {
    fn load(&self, api: &mut ExtensionApi<'_>) -> Result<(), String>;
}

#[derive(Default)]
pub struct LoadExtensionsResult {
    pub extensions: Vec<Extension>,
    pub errors: Vec<ExtensionError>,
    pub runtime: ExtensionRuntime,
}
