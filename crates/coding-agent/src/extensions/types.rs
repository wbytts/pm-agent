use crate::source_info::SourceInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

pub type ExtensionHandler = Arc<dyn Fn(ExtensionEvent) -> Option<Value> + Send + Sync>;
pub type CommandHandler = Arc<dyn Fn(ExtensionCommandContext) -> Result<(), String> + Send + Sync>;
pub type ToolExecutor = Arc<dyn Fn(Value, ExtensionContext) -> Result<Value, String> + Send + Sync>;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelConfig {
    pub id: String,
    pub display_name: Option<String>,
    pub api: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub display_name: Option<String>,
    pub models: Vec<ProviderModelConfig>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingProviderRegistration {
    pub name: String,
    pub config: ProviderConfig,
    pub extension_path: String,
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionRuntime {
    pub flag_values: BTreeMap<String, Value>,
    pub pending_provider_registrations: Vec<PendingProviderRegistration>,
    stale_message: Option<String>,
}

impl ExtensionRuntime {
    pub fn assert_active(&self) -> Result<(), String> {
        if let Some(message) = &self.stale_message {
            return Err(message.clone());
        }
        Ok(())
    }

    pub fn invalidate(&mut self, message: Option<String>) {
        self.stale_message = Some(message.unwrap_or_else(|| {
            "This extension context is stale after session replacement or reload.".to_string()
        }));
    }

    pub fn register_provider(
        &mut self,
        name: impl Into<String>,
        config: ProviderConfig,
        extension_path: impl Into<String>,
    ) {
        self.pending_provider_registrations
            .push(PendingProviderRegistration {
                name: name.into(),
                config,
                extension_path: extension_path.into(),
            });
    }

    pub fn unregister_provider(&mut self, name: &str) {
        self.pending_provider_registrations
            .retain(|registration| registration.name != name);
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
        self.runtime.unregister_provider(name);
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
}
