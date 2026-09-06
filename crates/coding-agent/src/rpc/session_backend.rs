use agent::harness::{
    InMemorySessionStorage, JsonlSessionStorage, PromptTemplate, SessionStorage, Skill,
};
use agent::AgentMessage;
use ai::{
    clamp_thinking_level, supported_thinking_levels, MessageRole, Model, ModelThinkingLevel,
    ProviderRegistry,
};
use std::fs;
use std::path::{Path, PathBuf};

use crate::auth_storage::AuthStorageBackend;
use crate::bash_executor::{execute_bash, BashResult};
use crate::changelog_command::changelog_summary;
use crate::compaction::{prepare_compaction, DEFAULT_COMPACTION_SETTINGS};
use crate::copy_command::copy_last_assistant_text;
#[cfg(test)]
use crate::copy_command::copy_last_assistant_text_with_runner;
use crate::export_html::{
    export_session_to_html, export_session_to_jsonl, ExportOptions, JsonlExportOptions,
};
use crate::extensions::types::{ExtensionEventKind, STALE_EXTENSION_CONTEXT_MESSAGE};
use crate::extensions::{ExtensionEvent, ExtensionRunner, ResolvedCommand};
use crate::model_registry::ModelRegistry;
use crate::rpc::dispatcher::RpcSessionBackend;
use crate::rpc::types::{ForkPosition, QueueMode, RpcSessionState, RpcSlashCommand};
use crate::session_cwd::assert_session_cwd_exists;
use crate::session_info::session_info_summary;
use crate::session_manager::{ForkMessage, SessionManager, SessionStats};
use crate::share_command::{share_session_html, temp_share_html_path, ShareSessionResult};
#[cfg(test)]
use crate::share_command::{share_session_html_with_runner, ShareCommandRunner};
use crate::slash_commands::SlashCommandInfo;
use crate::utils::AppConfigPaths;
#[cfg(test)]
use crate::utils::{ClipboardEnvironment, ClipboardPlatform, ClipboardRunner};

use super::command_registry::RpcCommandRegistry;
use super::prompt_input::PromptInputProcessor;

fn to_agent_compaction_preparation(
    preparation: crate::compaction::CompactionPreparation,
) -> agent::harness::CompactionPreparation {
    agent::harness::CompactionPreparation {
        first_kept_entry_id: preparation.first_kept_entry_id,
        messages_to_summarize: preparation.messages_to_summarize,
        turn_prefix_messages: preparation.turn_prefix_messages,
        is_split_turn: preparation.is_split_turn,
        tokens_before: preparation.tokens_before,
        previous_summary: preparation.previous_summary,
        file_ops: agent::harness::FileOperations {
            read: preparation.file_ops.read,
            written: preparation.file_ops.written,
            edited: preparation.file_ops.edited,
        },
        settings: agent::harness::CompactionSettings {
            enabled: preparation.settings.enabled,
            reserve_tokens: preparation.settings.reserve_tokens,
            keep_recent_tokens: preparation.settings.keep_recent_tokens,
        },
    }
}

pub trait RpcSessionLifecycle: SessionStorage {
    fn replace_with_new_session(
        manager: &mut SessionManager<Self>,
        parent_session: Option<String>,
    ) -> Result<(), String>
    where
        Self: Sized;

    fn switch_to_session(
        manager: &mut SessionManager<Self>,
        session_path: String,
        fallback_cwd: PathBuf,
    ) -> Result<(), String>
    where
        Self: Sized;

    fn create_branched_session(
        manager: &mut SessionManager<Self>,
        leaf_id: &str,
    ) -> Result<Option<PathBuf>, String>
    where
        Self: Sized;

    fn import_session(
        manager: &mut SessionManager<Self>,
        input_path: &Path,
        cwd_override: Option<PathBuf>,
        fallback_cwd: PathBuf,
    ) -> Result<PathBuf, String>
    where
        Self: Sized;
}

impl RpcSessionLifecycle for InMemorySessionStorage {
    fn replace_with_new_session(
        manager: &mut SessionManager<Self>,
        parent_session: Option<String>,
    ) -> Result<(), String> {
        manager.replace_with_new_session(parent_session)
    }

    fn switch_to_session(
        _manager: &mut SessionManager<Self>,
        _session_path: String,
        _fallback_cwd: PathBuf,
    ) -> Result<(), String> {
        Err("switch_session is not supported by in-memory RPC sessions".to_string())
    }

    fn create_branched_session(
        manager: &mut SessionManager<Self>,
        leaf_id: &str,
    ) -> Result<Option<PathBuf>, String> {
        manager.create_branched_session(leaf_id)
    }

    fn import_session(
        _manager: &mut SessionManager<Self>,
        _input_path: &Path,
        _cwd_override: Option<PathBuf>,
        _fallback_cwd: PathBuf,
    ) -> Result<PathBuf, String> {
        Err("import_session is not supported by in-memory RPC sessions".to_string())
    }
}

impl RpcSessionLifecycle for JsonlSessionStorage {
    fn replace_with_new_session(
        manager: &mut SessionManager<Self>,
        parent_session: Option<String>,
    ) -> Result<(), String> {
        manager.replace_with_new_session(parent_session)
    }

    fn switch_to_session(
        manager: &mut SessionManager<Self>,
        session_path: String,
        fallback_cwd: PathBuf,
    ) -> Result<(), String> {
        let next = SessionManager::open(session_path, None)?;
        assert_session_cwd_exists(&next, fallback_cwd).map_err(|error| error.to_string())?;
        *manager = next;
        Ok(())
    }

    fn create_branched_session(
        manager: &mut SessionManager<Self>,
        leaf_id: &str,
    ) -> Result<Option<PathBuf>, String> {
        manager.create_branched_session(leaf_id)
    }

    fn import_session(
        manager: &mut SessionManager<Self>,
        input_path: &Path,
        cwd_override: Option<PathBuf>,
        fallback_cwd: PathBuf,
    ) -> Result<PathBuf, String> {
        fs::create_dir_all(manager.session_dir()).map_err(|error| error.to_string())?;
        let destination = manager.session_dir().join(
            input_path
                .file_name()
                .ok_or_else(|| format!("Invalid import path: {}", input_path.display()))?,
        );
        let source = crate::utils::paths::resolve_path(&input_path.to_string_lossy(), "", None);
        let destination =
            crate::utils::paths::resolve_path(&destination.to_string_lossy(), "", None);
        if source != destination {
            fs::copy(&source, &destination).map_err(|error| error.to_string())?;
        }
        let imported = if let Some(cwd_override) = cwd_override {
            SessionManager::open_with_cwd(
                &destination,
                Some(manager.session_dir().to_path_buf()),
                cwd_override,
            )?
        } else {
            SessionManager::open(&destination, Some(manager.session_dir().to_path_buf()))?
        };
        assert_session_cwd_exists(&imported, fallback_cwd).map_err(|error| error.to_string())?;
        *manager = imported;
        Ok(destination)
    }
}

pub struct ManagedRpcSessionBackend<S: SessionStorage, B: AuthStorageBackend> {
    session_manager: SessionManager<S>,
    model_registry: ModelRegistry<B>,
    provider_registry: ProviderRegistry,
    extension_runner: Option<ExtensionRunner>,
    pending_session_start: SessionStartEvent,
    model: Option<Model>,
    thinking_level: ModelThinkingLevel,
    steering_messages: Vec<String>,
    follow_up_messages: Vec<String>,
    steering_mode: QueueMode,
    follow_up_mode: QueueMode,
    auto_compaction_enabled: bool,
    auto_retry_enabled: bool,
    command_registry: RpcCommandRegistry,
    prompt_input: PromptInputProcessor,
    app_config: AppConfigPaths,
    #[cfg(test)]
    copy_last_assistant_text_runner: Option<(
        ClipboardPlatform,
        ClipboardEnvironment,
        bool,
        Box<dyn ClipboardRunner>,
    )>,
    #[cfg(test)]
    share_command_runner: Option<Box<dyn ShareCommandRunner>>,
    #[cfg(test)]
    share_html_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SessionStartEvent {
    pub reason: String,
    pub previous_session_file: Option<String>,
}

impl Default for SessionStartEvent {
    fn default() -> Self {
        Self {
            reason: "startup".to_string(),
            previous_session_file: None,
        }
    }
}

impl<S: SessionStorage, B: AuthStorageBackend> ManagedRpcSessionBackend<S, B> {
    pub fn new(session_manager: SessionManager<S>, model_registry: ModelRegistry<B>) -> Self {
        let model = model_registry.get_available().into_iter().next();
        Self {
            session_manager,
            model_registry,
            provider_registry: ProviderRegistry::builtins(),
            extension_runner: None,
            pending_session_start: SessionStartEvent::default(),
            model,
            thinking_level: ModelThinkingLevel::Medium,
            steering_messages: Vec::new(),
            follow_up_messages: Vec::new(),
            steering_mode: QueueMode::OneAtATime,
            follow_up_mode: QueueMode::OneAtATime,
            auto_compaction_enabled: true,
            auto_retry_enabled: true,
            command_registry: RpcCommandRegistry::default(),
            prompt_input: PromptInputProcessor::new(),
            app_config: AppConfigPaths::new(
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(".")),
            ),
            #[cfg(test)]
            copy_last_assistant_text_runner: None,
            #[cfg(test)]
            share_command_runner: None,
            #[cfg(test)]
            share_html_path: None,
        }
    }

    pub fn with_slash_commands(mut self, commands: Vec<SlashCommandInfo>) -> Self {
        self.set_slash_commands(commands);
        self
    }

    pub fn with_rpc_slash_commands(mut self, commands: Vec<RpcSlashCommand>) -> Self {
        self.set_rpc_slash_commands(commands);
        self
    }

    pub fn set_slash_commands(&mut self, commands: Vec<SlashCommandInfo>) {
        self.command_registry.set_slash_commands(commands);
    }

    pub fn set_rpc_slash_commands(&mut self, commands: Vec<RpcSlashCommand>) {
        self.command_registry.set_rpc_slash_commands(commands);
    }

    pub fn with_prompt_resources(
        mut self,
        skills: Vec<Skill>,
        prompt_templates: Vec<PromptTemplate>,
    ) -> Self {
        self.set_prompt_resources(skills, prompt_templates);
        self
    }

    pub fn set_prompt_resources(
        &mut self,
        skills: Vec<Skill>,
        prompt_templates: Vec<PromptTemplate>,
    ) {
        self.prompt_input
            .set_prompt_resources(skills, prompt_templates);
    }

    pub fn set_expand_prompt_templates(&mut self, enabled: bool) {
        self.prompt_input.set_expand_prompt_templates(enabled);
    }

    pub fn set_app_config(&mut self, config: AppConfigPaths) {
        self.app_config = config;
    }

    #[cfg(test)]
    fn set_copy_last_assistant_text_runner(
        &mut self,
        platform: ClipboardPlatform,
        environment: ClipboardEnvironment,
        remote: bool,
        runner: Box<dyn ClipboardRunner>,
    ) {
        self.copy_last_assistant_text_runner = Some((platform, environment, remote, runner));
    }

    #[cfg(test)]
    fn set_share_command_runner(&mut self, runner: Box<dyn ShareCommandRunner>) {
        self.share_command_runner = Some(runner);
    }

    #[cfg(test)]
    fn set_share_html_path(&mut self, path: PathBuf) {
        self.share_html_path = Some(path);
    }

    pub fn with_extension_commands(mut self, commands: Vec<ResolvedCommand>) -> Self {
        self.set_extension_commands(commands);
        self
    }

    pub fn set_extension_commands(&mut self, commands: Vec<ResolvedCommand>) {
        self.prompt_input.set_extension_commands(commands);
    }

    pub fn bind_extension_runner(&mut self, runner: &mut ExtensionRunner) {
        runner.flush_pending_provider_registrations_with_api_providers(
            &mut self.model_registry,
            &mut self.provider_registry,
        );
        self.refresh_current_model_from_registry();
        self.extension_runner = Some(ExtensionRunner::new(
            runner.extensions().to_vec(),
            runner.runtime().clone(),
        ));
    }

    pub fn take_pending_session_start(&mut self) -> SessionStartEvent {
        std::mem::take(&mut self.pending_session_start)
    }

    pub fn emit_session_shutdown(&self, reason: &str, target_session_file: Option<String>) {
        let Some(runner) = &self.extension_runner else {
            return;
        };
        let mut payload = serde_json::json!({
            "type": "session_shutdown",
            "reason": reason,
        });
        if let Some(target_session_file) = target_session_file {
            payload["targetSessionFile"] = serde_json::Value::String(target_session_file);
        }
        runner.emit(
            "session_shutdown",
            ExtensionEvent {
                kind: ExtensionEventKind::SessionShutdown,
                payload,
            },
        );
    }

    fn complete_session_replacement(
        &mut self,
        reason: &str,
        target_session_file: Option<String>,
        previous_session_file: Option<String>,
    ) {
        self.emit_session_shutdown(reason, target_session_file);
        if let Some(runner) = &self.extension_runner {
            runner.invalidate(Some(STALE_EXTENSION_CONTEXT_MESSAGE.to_string()));
        }
        self.record_pending_session_start(reason, previous_session_file);
        self.steering_messages.clear();
        self.follow_up_messages.clear();
    }

    fn emit_session_before_switch(
        &self,
        reason: &str,
        target_session_file: Option<String>,
    ) -> bool {
        let Some(runner) = &self.extension_runner else {
            return false;
        };
        let mut payload = serde_json::json!({
            "type": "session_before_switch",
            "reason": reason,
        });
        if let Some(target_session_file) = target_session_file {
            payload["targetSessionFile"] = serde_json::Value::String(target_session_file);
        }
        runner
            .emit(
                "session_before_switch",
                ExtensionEvent {
                    kind: ExtensionEventKind::SessionBeforeSwitch,
                    payload,
                },
            )
            .into_iter()
            .any(|value| value.get("cancel").and_then(serde_json::Value::as_bool) == Some(true))
    }

    fn emit_session_before_fork(&self, entry_id: &str, position: &str) -> bool {
        let Some(runner) = &self.extension_runner else {
            return false;
        };
        runner
            .emit(
                "session_before_fork",
                ExtensionEvent {
                    kind: ExtensionEventKind::SessionBeforeFork,
                    payload: serde_json::json!({
                        "type": "session_before_fork",
                        "entryId": entry_id,
                        "position": position,
                    }),
                },
            )
            .into_iter()
            .any(|value| value.get("cancel").and_then(serde_json::Value::as_bool) == Some(true))
    }

    fn emit_session_before_compact(
        &self,
        preparation: serde_json::Value,
        branch_entries: serde_json::Value,
        custom_instructions: Option<String>,
    ) -> Vec<serde_json::Value> {
        let Some(runner) = &self.extension_runner else {
            return Vec::new();
        };
        let mut payload = serde_json::json!({
            "type": "session_before_compact",
            "preparation": preparation,
            "branchEntries": branch_entries,
        });
        if let Some(custom_instructions) = custom_instructions {
            payload["customInstructions"] = serde_json::Value::String(custom_instructions);
        }
        runner.emit(
            "session_before_compact",
            ExtensionEvent {
                kind: ExtensionEventKind::SessionBeforeCompact,
                payload,
            },
        )
    }

    fn emit_session_compact(&self, compaction_entry: serde_json::Value, from_extension: bool) {
        let Some(runner) = &self.extension_runner else {
            return;
        };
        runner.emit(
            "session_compact",
            ExtensionEvent {
                kind: ExtensionEventKind::SessionCompact,
                payload: serde_json::json!({
                    "type": "session_compact",
                    "compactionEntry": compaction_entry,
                    "fromExtension": from_extension,
                }),
            },
        );
    }

    fn emit_session_before_tree(&self, preparation: serde_json::Value) -> Vec<serde_json::Value> {
        let Some(runner) = &self.extension_runner else {
            return Vec::new();
        };
        runner.emit(
            "session_before_tree",
            ExtensionEvent {
                kind: ExtensionEventKind::SessionBeforeTree,
                payload: serde_json::json!({
                    "type": "session_before_tree",
                    "preparation": preparation,
                }),
            },
        )
    }

    fn emit_session_tree(
        &self,
        new_leaf_id: Option<String>,
        old_leaf_id: Option<String>,
        summary_entry: Option<serde_json::Value>,
        from_extension: Option<bool>,
    ) {
        let Some(runner) = &self.extension_runner else {
            return;
        };
        let mut payload = serde_json::json!({
            "type": "session_tree",
            "newLeafId": new_leaf_id,
            "oldLeafId": old_leaf_id,
        });
        if let Some(summary_entry) = summary_entry {
            payload["summaryEntry"] = summary_entry;
        }
        if let Some(from_extension) = from_extension {
            payload["fromExtension"] = serde_json::Value::Bool(from_extension);
        }
        runner.emit(
            "session_tree",
            ExtensionEvent {
                kind: ExtensionEventKind::SessionTree,
                payload,
            },
        );
    }

    fn record_pending_session_start(
        &mut self,
        reason: impl Into<String>,
        previous_session_file: Option<String>,
    ) {
        self.pending_session_start = SessionStartEvent {
            reason: reason.into(),
            previous_session_file,
        };
    }

    pub fn unregister_extension_provider(&mut self, runner: &ExtensionRunner, provider: &str) {
        runner.unregister_provider_with_api_providers(
            &mut self.model_registry,
            &mut self.provider_registry,
            provider,
        );
        self.refresh_current_model_from_registry();
    }

    pub fn provider_registry(&self) -> &ProviderRegistry {
        &self.provider_registry
    }

    pub fn model_registry(&self) -> &ModelRegistry<B> {
        &self.model_registry
    }

    pub fn session_manager(&self) -> &SessionManager<S> {
        &self.session_manager
    }

    pub fn session_manager_mut(&mut self) -> &mut SessionManager<S> {
        &mut self.session_manager
    }

    pub fn model(&self) -> Option<&Model> {
        self.model.as_ref()
    }

    pub fn auto_retry_enabled(&self) -> bool {
        self.auto_retry_enabled
    }

    fn refresh_current_model_from_registry(&mut self) {
        let Some(current) = &self.model else {
            return;
        };
        if let Some(model) = self.model_registry.find(&current.provider, &current.id) {
            self.model = Some(model);
            self.thinking_level = self.effective_thinking_level(self.thinking_level);
        }
    }

    pub fn flush_bound_extension_provider_registrations(&mut self) {
        if let Some(runner) = &mut self.extension_runner {
            runner.flush_pending_provider_registrations_with_api_providers(
                &mut self.model_registry,
                &mut self.provider_registry,
            );
            self.refresh_current_model_from_registry();
        }
    }

    #[cfg(test)]
    fn queued_steering_messages(&self) -> &[String] {
        &self.steering_messages
    }

    fn available_thinking_levels(&self) -> Vec<ModelThinkingLevel> {
        self.model
            .as_ref()
            .map(supported_thinking_levels)
            .unwrap_or_else(|| {
                vec![
                    ModelThinkingLevel::Off,
                    ModelThinkingLevel::Minimal,
                    ModelThinkingLevel::Low,
                    ModelThinkingLevel::Medium,
                    ModelThinkingLevel::High,
                    ModelThinkingLevel::XHigh,
                ]
            })
    }

    fn supports_thinking(&self) -> bool {
        self.model
            .as_ref()
            .and_then(|model| model.reasoning.as_ref())
            .is_some_and(|reasoning| reasoning.enabled)
    }

    fn effective_thinking_level(&self, level: ModelThinkingLevel) -> ModelThinkingLevel {
        self.model
            .as_ref()
            .map(|model| clamp_thinking_level(model, level))
            .unwrap_or(level)
    }

    fn share_current_session(&mut self) -> Result<ShareSessionResult, String> {
        let tmp_file = {
            #[cfg(test)]
            {
                self.share_html_path
                    .clone()
                    .unwrap_or_else(temp_share_html_path)
            }

            #[cfg(not(test))]
            {
                temp_share_html_path()
            }
        };
        export_session_to_html(
            &self.session_manager,
            ExportOptions {
                output_path: Some(tmp_file.clone()),
                ..ExportOptions::default()
            },
        )
        .map_err(|error| format!("Failed to export session: {error}"))?;

        let result = {
            #[cfg(test)]
            if let Some(runner) = self.share_command_runner.as_mut() {
                share_session_html_with_runner(&tmp_file, &self.app_config, runner.as_mut())
            } else {
                share_session_html(&tmp_file, &self.app_config)
            }

            #[cfg(not(test))]
            {
                share_session_html(&tmp_file, &self.app_config)
            }
        };
        let _ = fs::remove_file(&tmp_file);
        result
    }
}

impl<S: RpcSessionLifecycle, B: AuthStorageBackend> RpcSessionBackend
    for ManagedRpcSessionBackend<S, B>
{
    fn prompt(&mut self, message: String) -> Result<(), String> {
        if let Some((command_name, args)) =
            crate::rpc::prompt_input::parse_extension_command_invocation(&message)
        {
            if let Some(runner) = &self.extension_runner {
                if runner
                    .run_command_by_name(command_name, agent::harness::parse_command_args(args))?
                {
                    self.flush_bound_extension_provider_registrations();
                    return Ok(());
                }
            }
            if self.prompt_input.try_execute_extension_command(&message)? {
                self.flush_bound_extension_provider_registrations();
                return Ok(());
            }
        }
        let message = self.prompt_input.expand_prompt_text(&message)?;
        self.session_manager
            .append_message(AgentMessage::new(MessageRole::User, message))?;
        Ok(())
    }

    fn state(&self) -> Result<RpcSessionState, String> {
        let stats = self.session_manager.session_stats();
        Ok(RpcSessionState {
            model: self.model.clone(),
            thinking_level: self.thinking_level,
            is_streaming: false,
            is_compacting: false,
            steering_mode: self.steering_mode,
            follow_up_mode: self.follow_up_mode,
            session_file: stats.session_file,
            session_id: stats.session_id,
            session_name: self.session_manager.session_name(),
            auto_compaction_enabled: self.auto_compaction_enabled,
            message_count: stats.total_messages,
            pending_message_count: self.steering_messages.len() + self.follow_up_messages.len(),
        })
    }

    fn steer(&mut self, message: String) -> Result<(), String> {
        let message = self.prompt_input.expand_prompt_text(&message)?;
        self.steering_messages.push(message);
        Ok(())
    }

    fn follow_up(&mut self, message: String) -> Result<(), String> {
        let message = self.prompt_input.expand_prompt_text(&message)?;
        self.follow_up_messages.push(message);
        Ok(())
    }

    fn abort(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn new_session(&mut self, parent_session: Option<String>) -> Result<serde_json::Value, String> {
        if self.emit_session_before_switch("new", None) {
            return Ok(serde_json::json!({
                "cancelled": true,
            }));
        }
        let previous_session_file = self.session_manager.session_stats().session_file;
        S::replace_with_new_session(&mut self.session_manager, parent_session)?;
        let target_session_file = self.session_manager.session_stats().session_file;
        self.complete_session_replacement("new", target_session_file, previous_session_file);
        Ok(serde_json::json!({
            "cancelled": false,
        }))
    }

    fn reload(&mut self) -> Result<serde_json::Value, String> {
        self.emit_session_shutdown("reload", None);
        if let Some(runner) = &self.extension_runner {
            runner.invalidate(Some(STALE_EXTENSION_CONTEXT_MESSAGE.to_string()));
        }
        self.record_pending_session_start("reload", None);
        Ok(serde_json::json!({
            "cancelled": false,
        }))
    }

    fn set_model(&mut self, provider: String, model_id: String) -> Result<Model, String> {
        let model = self
            .model_registry
            .find(&provider, &model_id)
            .ok_or_else(|| format!("Model not found: {provider}/{model_id}"))?;
        self.model = Some(model.clone());
        self.thinking_level = self.effective_thinking_level(self.thinking_level);
        Ok(model)
    }

    fn cycle_model(&mut self) -> Result<Option<serde_json::Value>, String> {
        let available_models = self.model_registry.get_available();
        if available_models.len() <= 1 {
            return Ok(None);
        }
        let current_index = available_models
            .iter()
            .position(|model| {
                self.model.as_ref().is_some_and(|current| {
                    current.provider == model.provider && current.id == model.id
                })
            })
            .unwrap_or(0);
        let next_model = available_models[(current_index + 1) % available_models.len()].clone();
        self.model = Some(next_model.clone());
        self.thinking_level = self.effective_thinking_level(self.thinking_level);
        Ok(Some(serde_json::json!({
            "model": next_model,
            "thinkingLevel": self.thinking_level,
            "isScoped": false,
        })))
    }

    fn available_models(&self) -> Result<Vec<Model>, String> {
        Ok(self.model_registry.get_available())
    }

    fn set_thinking_level(&mut self, level: ModelThinkingLevel) -> Result<(), String> {
        self.thinking_level = self.effective_thinking_level(level);
        Ok(())
    }

    fn cycle_thinking_level(&mut self) -> Result<Option<ModelThinkingLevel>, String> {
        if !self.supports_thinking() {
            self.thinking_level = ModelThinkingLevel::Off;
            return Ok(None);
        }
        let levels = self.available_thinking_levels();
        let current_index = levels
            .iter()
            .position(|level| *level == self.thinking_level)
            .unwrap_or(0);
        let next_level = levels[(current_index + 1) % levels.len()];
        self.set_thinking_level(next_level)?;
        Ok(Some(self.thinking_level))
    }

    fn set_steering_mode(&mut self, mode: QueueMode) -> Result<(), String> {
        self.steering_mode = mode;
        Ok(())
    }

    fn set_follow_up_mode(&mut self, mode: QueueMode) -> Result<(), String> {
        self.follow_up_mode = mode;
        Ok(())
    }

    fn compact(
        &mut self,
        custom_instructions: Option<String>,
    ) -> Result<serde_json::Value, String> {
        let branch_entries = self.session_manager.branch(None)?;
        let preparation = prepare_compaction(&branch_entries, DEFAULT_COMPACTION_SETTINGS)?
            .ok_or_else(|| {
                if matches!(
                    branch_entries.last(),
                    Some(agent::harness::SessionTreeEntry::Compaction { .. })
                ) {
                    "Already compacted".to_string()
                } else {
                    "Nothing to compact (session too small)".to_string()
                }
            })?;
        let preparation_value =
            serde_json::to_value(&preparation).map_err(|error| error.to_string())?;
        let branch_entries_value =
            serde_json::to_value(&branch_entries).map_err(|error| error.to_string())?;

        let mut extension_compaction = None;
        for result in self.emit_session_before_compact(
            preparation_value,
            branch_entries_value,
            custom_instructions.clone(),
        ) {
            if result.get("cancel").and_then(serde_json::Value::as_bool) == Some(true) {
                return Err("Compaction cancelled".to_string());
            }
            if extension_compaction.is_none() {
                extension_compaction = result.get("compaction").cloned();
            }
        }

        let (summary, first_kept_entry_id, tokens_before, details, from_extension) =
            if let Some(compaction) = extension_compaction {
                let summary = compaction
                    .get("summary")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "Extension compaction missing summary".to_string())?
                    .to_string();
                let first_kept_entry_id = compaction
                    .get("firstKeptEntryId")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "Extension compaction missing firstKeptEntryId".to_string())?
                    .to_string();
                let tokens_before = compaction
                    .get("tokensBefore")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "Extension compaction missing tokensBefore".to_string())?;
                let details = compaction.get("details").cloned();
                (summary, first_kept_entry_id, tokens_before, details, true)
            } else {
                let model = self
                    .model
                    .clone()
                    .ok_or_else(|| "No model available for compaction".to_string())?;
                let provider = self
                    .provider_registry
                    .provider_for(&model)
                    .map_err(|error| error.to_string())?;
                let result = agent::harness::compact(
                    to_agent_compaction_preparation(preparation),
                    &provider,
                    model,
                    agent::harness::CompactOptions {
                        custom_instructions,
                    },
                )
                .map_err(|error| error.message)?;
                let details =
                    serde_json::to_value(&result.details).map_err(|error| error.to_string())?;
                (
                    result.summary,
                    result.first_kept_entry_id,
                    result.tokens_before,
                    Some(details),
                    false,
                )
            };

        let compaction_entry_id = self.session_manager.append_compaction(
            summary.clone(),
            first_kept_entry_id.clone(),
            tokens_before,
            details.clone(),
            from_extension,
        )?;
        let compaction_entry = self
            .session_manager
            .branch(None)?
            .into_iter()
            .find(|entry| entry.id() == compaction_entry_id)
            .ok_or_else(|| "Saved compaction entry not found".to_string())?;
        let compaction_entry_value =
            serde_json::to_value(&compaction_entry).map_err(|error| error.to_string())?;
        self.emit_session_compact(compaction_entry_value, from_extension);

        Ok(serde_json::json!({
            "summary": summary,
            "firstKeptEntryId": first_kept_entry_id,
            "tokensBefore": tokens_before,
            "details": details,
        }))
    }

    fn set_auto_compaction(&mut self, enabled: bool) -> Result<(), String> {
        self.auto_compaction_enabled = enabled;
        Ok(())
    }

    fn set_auto_retry(&mut self, enabled: bool) -> Result<(), String> {
        self.auto_retry_enabled = enabled;
        Ok(())
    }

    fn abort_retry(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn bash(&mut self, command: String) -> Result<BashResult, String> {
        let result = execute_bash(
            &command,
            &self.session_manager.cwd().to_string_lossy(),
            None,
        )?;
        self.session_manager
            .append_message(AgentMessage::new(MessageRole::Tool, result.output.clone()))?;
        Ok(result)
    }

    fn abort_bash(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn session_stats(&self) -> Result<SessionStats, String> {
        Ok(self.session_manager.session_stats())
    }

    fn session_info(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(session_info_summary(
            self.session_manager.session_stats(),
            self.session_manager.session_name(),
        ))
        .map_err(|error| error.to_string())
    }

    fn changelog(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(changelog_summary(&self.app_config)).map_err(|error| error.to_string())
    }

    fn export_html(&mut self, output_path: Option<String>) -> Result<String, String> {
        let path = output_path.map(PathBuf::from);
        if path
            .as_deref()
            .and_then(std::path::Path::extension)
            .and_then(|extension| extension.to_str())
            == Some("jsonl")
        {
            return export_session_to_jsonl(
                &self.session_manager,
                JsonlExportOptions { output_path: path },
            )
            .map(|path| path.to_string_lossy().to_string());
        }
        export_session_to_html(
            &self.session_manager,
            ExportOptions {
                output_path: path,
                ..ExportOptions::default()
            },
        )
        .map(|path| path.to_string_lossy().to_string())
    }

    fn share_session(&mut self) -> Result<serde_json::Value, String> {
        let result = self.share_current_session()?;
        Ok(serde_json::json!({
            "previewUrl": result.preview_url,
            "gistUrl": result.gist_url,
            "gistId": result.gist_id,
        }))
    }

    fn switch_session(&mut self, session_path: String) -> Result<serde_json::Value, String> {
        if self.emit_session_before_switch("resume", Some(session_path.clone())) {
            return Ok(serde_json::json!({
                "cancelled": true,
            }));
        }
        let previous_session_file = self.session_manager.session_stats().session_file;
        let fallback_cwd = self.session_manager.cwd().to_path_buf();
        S::switch_to_session(&mut self.session_manager, session_path, fallback_cwd)?;
        let target_session_file = self.session_manager.session_stats().session_file;
        self.complete_session_replacement("resume", target_session_file, previous_session_file);
        Ok(serde_json::json!({
            "cancelled": false,
        }))
    }

    fn import_session(
        &mut self,
        input_path: String,
        cwd_override: Option<String>,
    ) -> Result<serde_json::Value, String> {
        let input_path = crate::utils::paths::resolve_path(&input_path, "", None);
        if !input_path.exists() {
            return Err(format!("Import file not found: {}", input_path.display()));
        }
        let destination = self.session_manager.session_dir().join(
            input_path
                .file_name()
                .ok_or_else(|| format!("Invalid import path: {}", input_path.display()))?,
        );
        let destination =
            crate::utils::paths::resolve_path(&destination.to_string_lossy(), "", None);
        let destination_string = destination.to_string_lossy().to_string();
        if self.emit_session_before_switch("resume", Some(destination_string.clone())) {
            return Ok(serde_json::json!({
                "cancelled": true,
            }));
        }

        let previous_session_file = self.session_manager.session_stats().session_file;
        let fallback_cwd = self.session_manager.cwd().to_path_buf();
        S::import_session(
            &mut self.session_manager,
            &input_path,
            cwd_override.map(PathBuf::from),
            fallback_cwd,
        )?;
        let target_session_file = self.session_manager.session_stats().session_file;
        self.complete_session_replacement("resume", target_session_file, previous_session_file);
        Ok(serde_json::json!({
            "cancelled": false,
            "sessionPath": destination_string,
        }))
    }

    fn fork(
        &mut self,
        entry_id: String,
        position: ForkPosition,
    ) -> Result<serde_json::Value, String> {
        let position_name = match position {
            ForkPosition::Before => "before",
            ForkPosition::At => "at",
        };
        if self.emit_session_before_fork(&entry_id, position_name) {
            return Ok(serde_json::json!({
                "cancelled": true,
            }));
        }
        let previous_session_file = self.session_manager.session_stats().session_file;
        let selected_text = match position {
            ForkPosition::Before => Some(self.session_manager.fork_before_user_message(&entry_id)?),
            ForkPosition::At => {
                S::create_branched_session(&mut self.session_manager, &entry_id)?;
                None
            }
        };
        let target_session_file = self.session_manager.session_stats().session_file;
        self.complete_session_replacement("fork", target_session_file, previous_session_file);
        let mut response = serde_json::json!({
            "cancelled": false,
        });
        if let Some(text) = selected_text {
            response["text"] = serde_json::Value::String(text);
        }
        Ok(response)
    }

    fn clone_session(&mut self) -> Result<serde_json::Value, String> {
        self.session_manager.clone_at_leaf()?;
        Ok(serde_json::json!({
            "cancelled": false,
        }))
    }

    fn navigate_tree(
        &mut self,
        target_id: String,
        summarize: bool,
        custom_instructions: Option<String>,
        replace_instructions: bool,
        label: Option<String>,
    ) -> Result<serde_json::Value, String> {
        let old_leaf_id = self.session_manager.leaf_id()?;
        if old_leaf_id.as_deref() == Some(target_id.as_str()) {
            return Ok(serde_json::json!({
                "cancelled": false,
            }));
        }
        let target_entry = self
            .session_manager
            .entry(&target_id)
            .cloned()
            .ok_or_else(|| format!("Entry {target_id} not found"))?;
        let collected = self
            .session_manager
            .collect_branch_summary_entries(old_leaf_id.as_deref(), &target_id)?;
        let preparation = serde_json::json!({
            "targetId": target_id,
            "oldLeafId": old_leaf_id,
            "commonAncestorId": collected.common_ancestor_id,
            "entriesToSummarize": collected.entries,
            "userWantsSummary": summarize,
            "customInstructions": custom_instructions,
            "replaceInstructions": replace_instructions,
            "label": label,
        });

        let mut extension_summary = None;
        let mut from_extension = false;
        let mut label = label;
        for result in self.emit_session_before_tree(preparation) {
            if result.get("cancel").and_then(serde_json::Value::as_bool) == Some(true) {
                return Ok(serde_json::json!({
                    "cancelled": true,
                }));
            }
            if summarize && extension_summary.is_none() {
                extension_summary = result.get("summary").cloned();
                from_extension = extension_summary.is_some();
            }
            if result.get("label").is_some() {
                label = result
                    .get("label")
                    .and_then(serde_json::Value::as_str)
                    .map(ToString::to_string);
            }
        }

        let (new_leaf_id, editor_text) = match &target_entry {
            agent::harness::SessionTreeEntry::Message {
                parent_id, message, ..
            } if message.role == MessageRole::User => {
                (parent_id.clone(), Some(message.content.clone()))
            }
            agent::harness::SessionTreeEntry::CustomMessage {
                parent_id, content, ..
            } => (parent_id.clone(), Some(content.clone())),
            _ => (Some(target_id.clone()), None),
        };

        let mut summary_entry = None;
        if summarize && !collected.entries.is_empty() {
            let (summary, details) = if let Some(summary_value) = extension_summary {
                let summary = summary_value
                    .get("summary")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| "Extension tree summary missing summary".to_string())?
                    .to_string();
                let details = summary_value.get("details").cloned();
                (summary, details)
            } else {
                let model = self
                    .model
                    .clone()
                    .ok_or_else(|| "No model available for summarization".to_string())?;
                let provider = self
                    .provider_registry
                    .provider_for(&model)
                    .map_err(|error| error.to_string())?;
                let result = agent::harness::generate_branch_summary(
                    &collected.entries,
                    &provider,
                    model,
                    agent::harness::compaction::GenerateBranchSummaryOptions {
                        custom_instructions,
                        replace_instructions,
                        reserve_tokens: Some(16_384),
                    },
                )
                .map_err(|error| error.message)?;
                let details = serde_json::json!({
                    "readFiles": result.read_files,
                    "modifiedFiles": result.modified_files,
                });
                (result.summary, Some(details))
            };
            let summary_id = self.session_manager.append_branch_summary(
                new_leaf_id.clone(),
                summary,
                details,
                from_extension,
            )?;
            if let Some(label) = label.clone() {
                self.session_manager
                    .append_label_change(summary_id.clone(), Some(label))?;
                self.session_manager.move_to(Some(summary_id.clone()))?;
            }
            let entry = self
                .session_manager
                .entry(&summary_id)
                .cloned()
                .ok_or_else(|| "Saved branch summary entry not found".to_string())?;
            summary_entry = Some(serde_json::to_value(entry).map_err(|error| error.to_string())?);
        } else {
            self.session_manager.move_to(new_leaf_id.clone())?;
            if let Some(label) = label {
                self.session_manager
                    .append_label_change(target_id.clone(), Some(label))?;
                self.session_manager.move_to(new_leaf_id.clone())?;
            }
        }

        let new_leaf_id = self.session_manager.leaf_id()?;
        self.emit_session_tree(
            new_leaf_id.clone(),
            old_leaf_id,
            summary_entry.clone(),
            summary_entry.as_ref().map(|_| from_extension),
        );

        let mut response = serde_json::json!({
            "cancelled": false,
        });
        if let Some(editor_text) = editor_text {
            response["editorText"] = serde_json::Value::String(editor_text);
        }
        if let Some(summary_entry) = summary_entry {
            response["summaryEntry"] = summary_entry;
        }
        Ok(response)
    }

    fn fork_messages(&self) -> Result<Vec<ForkMessage>, String> {
        Ok(self.session_manager.fork_messages())
    }

    fn last_assistant_text(&self) -> Result<Option<String>, String> {
        Ok(self.session_manager.last_assistant_text())
    }

    fn copy_last_assistant_text(&mut self) -> Result<String, String> {
        #[cfg(test)]
        if let Some((platform, environment, remote, runner)) =
            self.copy_last_assistant_text_runner.as_mut()
        {
            return copy_last_assistant_text_with_runner(
                &self.session_manager,
                *platform,
                *environment,
                *remote,
                runner.as_mut(),
            )
            .map_err(|error| error.to_string());
        }

        copy_last_assistant_text(&self.session_manager).map_err(|error| error.to_string())
    }

    fn set_session_name(&mut self, name: String) -> Result<(), String> {
        self.session_manager.append_session_name(name)?;
        Ok(())
    }

    fn messages(&self) -> Result<Vec<AgentMessage>, String> {
        Ok(self.session_manager.build_context()?.messages)
    }

    fn commands(&self) -> Result<Vec<RpcSlashCommand>, String> {
        Ok(self.command_registry.rpc_slash_commands())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_storage::{AuthStorage, AuthStorageData, InMemoryAuthStorageBackend};
    use crate::rpc::prompt_input;
    use crate::share_command::ShareCommandOutput;
    use crate::utils::ClipboardCommand;
    use agent::harness::InMemorySessionStorage;
    use std::sync::{Arc, Mutex};

    #[test]
    fn managed_backend_handles_prompt_state_and_messages() {
        let mut backend = test_backend();
        backend.prompt("hello".to_string()).expect("prompt");
        let state = backend.state().expect("state");

        assert_eq!(state.message_count, 1);
        assert_eq!(backend.messages().expect("messages")[0].content, "hello");
    }

    #[test]
    fn managed_backend_updates_modes_and_session_name() {
        let mut backend = test_backend();
        backend
            .set_steering_mode(QueueMode::All)
            .expect("steering mode");
        backend
            .set_follow_up_mode(QueueMode::All)
            .expect("follow up mode");
        backend
            .set_session_name("Demo".to_string())
            .expect("session name");

        let state = backend.state().expect("state");
        assert_eq!(state.steering_mode, QueueMode::All);
        assert_eq!(state.follow_up_mode, QueueMode::All);
        assert_eq!(state.session_name.as_deref(), Some("Demo"));
    }

    #[test]
    fn managed_backend_get_session_info_formats_pi_session_command_data() {
        let mut backend = test_backend();
        backend
            .set_session_name("Demo".to_string())
            .expect("session name");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(MessageRole::User, "question".to_string()))
            .expect("user");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "answer".to_string(),
            ))
            .expect("assistant");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::Tool,
                "tool result".to_string(),
            ))
            .expect("tool");

        let info = backend.session_info().expect("session info");

        assert_eq!(info["name"], "Demo");
        assert_eq!(info["stats"]["sessionFile"], serde_json::Value::Null);
        assert_eq!(info["stats"]["userMessages"], 1);
        assert_eq!(info["stats"]["assistantMessages"], 1);
        assert_eq!(info["stats"]["toolResults"], 1);
        assert_eq!(info["stats"]["totalMessages"], 3);
        assert_eq!(info["cost"], 0.0);
        let text = info["text"].as_str().expect("text");
        assert!(text.contains("Session Info\n\nName: Demo\n"));
        assert!(text.contains("File: In-memory\n"));
        assert!(text.contains("Messages\nUser: 1\nAssistant: 1\n"));
        assert!(text.contains("Tokens\nInput: 0\nOutput: 0\nTotal: 0"));
    }

    #[test]
    fn managed_backend_get_changelog_formats_pi_changelog_command_data() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("CHANGELOG.md"),
            "# Changelog\n\n## 0.1.0\n- first\n\n## 0.2.0\n- second\n",
        )
        .expect("changelog");
        let mut backend = test_backend();
        let mut config = AppConfigPaths::new("/home/alice");
        config.package_dir = dir;
        backend.set_app_config(config);

        let changelog = backend.changelog().expect("changelog");

        assert_eq!(changelog["title"], "What's New");
        assert_eq!(
            changelog["markdown"],
            "## 0.2.0\n- second\n\n## 0.1.0\n- first"
        );
        assert_eq!(changelog["entries"].as_array().expect("entries").len(), 2);
    }

    #[test]
    fn managed_backend_get_changelog_reports_pi_empty_message() {
        let mut backend = test_backend();
        let mut config = AppConfigPaths::new("/home/alice");
        config.package_dir = temp_dir();
        backend.set_app_config(config);

        let changelog = backend.changelog().expect("changelog");

        assert_eq!(changelog["title"], "What's New");
        assert_eq!(
            changelog["markdown"],
            crate::changelog_command::NO_CHANGELOG_ENTRIES
        );
        assert!(changelog["entries"].as_array().expect("entries").is_empty());
    }

    #[test]
    fn managed_backend_reports_missing_model_like_rpc_mode() {
        let mut backend = test_backend();
        let error = backend
            .set_model("missing".to_string(), "model".to_string())
            .expect_err("model should be missing");

        assert_eq!(error, "Model not found: missing/model");
    }

    #[test]
    fn managed_backend_cycles_available_models_like_pi_rpc() {
        let mut first = plain_model();
        first.id = "first".to_string();
        let mut second = reasoning_model();
        second.id = "second".to_string();
        let mut backend = test_backend_with_models(vec![first, second]);

        let result = backend
            .cycle_model()
            .expect("cycle model")
            .expect("second model should be selected");

        assert_eq!(result["model"]["id"], "second");
        assert_eq!(result["thinkingLevel"], "medium");
        assert_eq!(result["isScoped"], false);
        assert_eq!(
            backend.model().expect("model should update").id.as_str(),
            "second"
        );
        assert_eq!(
            backend.state().expect("state").thinking_level,
            ModelThinkingLevel::Medium
        );
    }

    #[test]
    fn managed_backend_cycle_model_returns_null_for_single_available_model_like_pi_rpc() {
        let mut backend = test_backend_with_models(vec![plain_model()]);

        let result = backend.cycle_model().expect("cycle model");

        assert!(result.is_none());
    }

    #[test]
    fn bind_extension_runner_refreshes_active_model_after_session_start_registration_like_pi() {
        let session_manager = SessionManager::in_memory("/tmp/project");
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let anthropic_model = model_registry
            .get_all()
            .into_iter()
            .find(|model| model.provider == "anthropic")
            .expect("builtin anthropic model should exist");
        let mut backend = ManagedRpcSessionBackend::new(session_manager, model_registry);
        backend
            .set_model(anthropic_model.provider.clone(), anthropic_model.id.clone())
            .expect("set anthropic model");

        let runtime = crate::extensions::ExtensionRuntime::default();
        runtime.register_provider(
            "anthropic",
            crate::extensions::ProviderConfig {
                base_url: Some("http://localhost:8080/session-start".to_string()),
                ..crate::extensions::ProviderConfig::default()
            },
            "/extensions/session-start.ts",
        );
        let mut runner = ExtensionRunner::new(Vec::new(), runtime);

        backend.bind_extension_runner(&mut runner);

        assert_eq!(
            backend.model().and_then(|model| model.base_url.as_deref()),
            Some("http://localhost:8080/session-start")
        );
    }

    #[test]
    fn extension_command_time_register_provider_refreshes_active_model_like_pi() {
        let session_manager = SessionManager::in_memory("/tmp/project");
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let anthropic_model = model_registry
            .get_all()
            .into_iter()
            .find(|model| model.provider == "anthropic")
            .expect("builtin anthropic model should exist");
        let mut backend = ManagedRpcSessionBackend::new(session_manager, model_registry);
        backend
            .set_model(anthropic_model.provider.clone(), anthropic_model.id.clone())
            .expect("set anthropic model");

        let runtime = crate::extensions::ExtensionRuntime::default();
        let runtime_for_command = runtime.clone();
        let command = ResolvedCommand {
            invocation_name: "use-proxy".to_string(),
            command: crate::extensions::RegisteredCommand {
                name: "use-proxy".to_string(),
                description: Some("Use proxy".to_string()),
                handler: std::sync::Arc::new(move |_ctx| {
                    runtime_for_command.register_provider(
                        "anthropic",
                        crate::extensions::ProviderConfig {
                            base_url: Some("http://localhost:8080/command".to_string()),
                            ..crate::extensions::ProviderConfig::default()
                        },
                        "/extensions/use-proxy.ts",
                    );
                    Ok(())
                }),
                source_info: crate::source_info::create_synthetic_source_info(
                    "/extensions/use-proxy.ts",
                    "local",
                    None,
                    None,
                    None,
                ),
            },
        };
        let mut runner = ExtensionRunner::new(Vec::new(), runtime);
        backend.bind_extension_runner(&mut runner);
        backend.set_extension_commands(vec![command]);

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
    fn managed_backend_sets_and_cycles_thinking_level_like_pi_rpc() {
        let mut backend = test_backend_with_models(vec![reasoning_model()]);

        backend
            .set_thinking_level(ModelThinkingLevel::High)
            .expect("set thinking");
        assert_eq!(
            backend.state().expect("state").thinking_level,
            ModelThinkingLevel::High
        );

        let cycled = backend
            .cycle_thinking_level()
            .expect("cycle thinking")
            .expect("reasoning model should cycle");
        assert_eq!(cycled, ModelThinkingLevel::XHigh);
        assert_eq!(
            backend.state().expect("state").thinking_level,
            ModelThinkingLevel::XHigh
        );
    }

    #[test]
    fn managed_backend_clamps_thinking_level_to_current_model_like_pi_rpc() {
        let mut model = reasoning_model();
        model
            .thinking_level_map
            .insert(ModelThinkingLevel::High, None);
        let mut backend = test_backend_with_models(vec![model]);

        backend
            .set_thinking_level(ModelThinkingLevel::High)
            .expect("set thinking");

        assert_eq!(
            backend.state().expect("state").thinking_level,
            ModelThinkingLevel::XHigh
        );
    }

    #[test]
    fn managed_backend_cycle_thinking_returns_null_for_non_reasoning_model_like_pi_rpc() {
        let mut backend = test_backend_with_models(vec![plain_model()]);

        let cycled = backend.cycle_thinking_level().expect("cycle thinking");

        assert_eq!(cycled, None);
        assert_eq!(
            backend.state().expect("state").thinking_level,
            ModelThinkingLevel::Off
        );
    }

    #[test]
    fn managed_backend_returns_registered_slash_commands() {
        let mut backend = test_backend();
        backend.set_slash_commands(vec![crate::slash_commands::SlashCommandInfo {
            name: "skill:rust".to_string(),
            description: Some("Rust help".to_string()),
            argument_hint: None,
            source: crate::slash_commands::SlashCommandSource::Skill,
            source_info: serde_json::json!({ "path": "/skills/rust/SKILL.md" }),
        }]);

        let commands = backend.commands().expect("commands");

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "skill:rust");
        assert_eq!(commands[0].source_info["path"], "/skills/rust/SKILL.md");
    }

    #[test]
    fn managed_backend_copy_last_assistant_text_like_pi_copy_command() {
        let copied = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut backend = test_backend();
        backend.set_copy_last_assistant_text_runner(
            ClipboardPlatform::Macos,
            ClipboardEnvironment::default(),
            false,
            Box::new(RecordingClipboardRunner {
                copied: copied.clone(),
            }),
        );
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "older answer".to_string(),
            ))
            .expect("older assistant");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(MessageRole::User, "question".to_string()))
            .expect("question");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "latest answer".to_string(),
            ))
            .expect("latest assistant");

        let text = backend.copy_last_assistant_text().expect("copy");

        assert_eq!(text, "latest answer");
        assert_eq!(
            *copied.lock().expect("copied lock"),
            vec!["latest answer".to_string()]
        );
    }

    #[test]
    fn managed_backend_copy_last_assistant_text_reports_pi_empty_error() {
        let copied = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut backend = test_backend();
        backend.set_copy_last_assistant_text_runner(
            ClipboardPlatform::Macos,
            ClipboardEnvironment::default(),
            false,
            Box::new(RecordingClipboardRunner {
                copied: copied.clone(),
            }),
        );
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(MessageRole::User, "question".to_string()))
            .expect("question");

        let error = backend
            .copy_last_assistant_text()
            .expect_err("copy should fail");

        assert_eq!(error, crate::copy_command::NO_AGENT_MESSAGES_TO_COPY);
        assert!(copied.lock().expect("copied lock").is_empty());
    }

    #[test]
    fn persisted_rpc_backend_share_session_creates_secret_gist_like_pi() {
        let dir = temp_dir();
        let share_html_path = dir.join("session.html");
        let calls = Arc::new(Mutex::new(Vec::<(String, Vec<String>)>::new()));
        let mut session = SessionManager::create("/tmp/project", Some(dir.clone()))
            .expect("session should create");
        session
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "shared answer".to_string(),
            ))
            .expect("assistant");
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session, model_registry);
        let mut config = AppConfigPaths::new("/home/alice");
        config.share_viewer_url_value = Some("https://viewer.test/session/".to_string());
        backend.set_app_config(config);
        backend.set_share_html_path(share_html_path.clone());
        backend.set_share_command_runner(Box::new(RecordingShareCommandRunner {
            outputs: vec![
                Ok(share_output("", "", Some(0))),
                Ok(share_output(
                    "https://gist.github.com/alice/abc123\n",
                    "",
                    Some(0),
                )),
            ],
            calls: calls.clone(),
        }));

        let result = backend.share_session().expect("share");

        assert_eq!(result["previewUrl"], "https://viewer.test/session/#abc123");
        assert_eq!(result["gistUrl"], "https://gist.github.com/alice/abc123");
        assert_eq!(result["gistId"], "abc123");
        assert_eq!(
            *calls.lock().expect("calls lock"),
            vec![
                (
                    "gh".to_string(),
                    vec!["auth".to_string(), "status".to_string()],
                ),
                (
                    "gh".to_string(),
                    vec![
                        "gist".to_string(),
                        "create".to_string(),
                        "--public=false".to_string(),
                        share_html_path.to_string_lossy().to_string(),
                    ],
                ),
            ]
        );
        assert!(!share_html_path.exists());
    }

    #[test]
    fn persisted_rpc_backend_share_session_reports_pi_auth_error() {
        let dir = temp_dir();
        let share_html_path = dir.join("session.html");
        let mut session = SessionManager::create("/tmp/project", Some(dir)).expect("session");
        session
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "answer".to_string(),
            ))
            .expect("assistant");
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session, model_registry);
        backend.set_share_html_path(share_html_path);
        backend.set_share_command_runner(Box::new(RecordingShareCommandRunner {
            outputs: vec![Ok(share_output("", "login required", Some(1)))],
            calls: Arc::new(Mutex::new(Vec::new())),
        }));

        let error = backend.share_session().expect_err("share should fail");

        assert_eq!(error, crate::share_command::GH_NOT_LOGGED_IN);
    }

    #[test]
    fn in_memory_rpc_backend_share_session_reports_export_failure() {
        let mut backend = test_backend();

        let error = backend.share_session().expect_err("share should fail");

        assert_eq!(
            error,
            "Failed to export session: Cannot export in-memory session to HTML"
        );
    }

    #[test]
    fn managed_backend_expands_prompt_templates() {
        let mut backend = test_backend();
        backend.set_prompt_resources(
            Vec::new(),
            vec![PromptTemplate {
                name: "review".to_string(),
                description: Some("Review".to_string()),
                argument_hint: None,
                content: "Review $ARGUMENTS".to_string(),
                file_path: "/prompts/review.md".to_string(),
                source_info: None,
            }],
        );

        backend
            .prompt("/review src/lib.rs".to_string())
            .expect("prompt");

        assert_eq!(
            backend.messages().expect("messages")[0].content,
            "Review src/lib.rs"
        );
    }

    #[test]
    fn managed_backend_expands_skill_commands_before_prompt_templates() {
        let dir = temp_dir();
        let skill_path = dir.join("SKILL.md");
        std::fs::write(
            &skill_path,
            "---\nname: demo\ndescription: Demo\n---\nUse demo.",
        )
        .expect("skill file");
        let mut backend = test_backend();
        backend.set_prompt_resources(
            vec![Skill {
                name: "demo".to_string(),
                description: "Demo".to_string(),
                content: String::new(),
                file_path: skill_path.to_string_lossy().to_string(),
                source_info: None,
                disable_model_invocation: false,
            }],
            vec![PromptTemplate {
                name: "skill:demo".to_string(),
                description: Some("Should not override skill command".to_string()),
                argument_hint: None,
                content: "template".to_string(),
                file_path: "/prompts/skill-demo.md".to_string(),
                source_info: None,
            }],
        );

        backend
            .prompt("/skill:demo context".to_string())
            .expect("prompt");

        let content = &backend.messages().expect("messages")[0].content;
        assert!(content.contains("<skill name=\"demo\""));
        assert!(content.contains("Use demo."));
        assert!(content.ends_with("context"));
        assert_ne!(content, "template");
    }

    #[test]
    fn managed_backend_can_disable_prompt_expansion() {
        let mut backend = test_backend();
        backend.set_prompt_resources(
            Vec::new(),
            vec![PromptTemplate {
                name: "review".to_string(),
                description: None,
                argument_hint: None,
                content: "expanded".to_string(),
                file_path: "/prompts/review.md".to_string(),
                source_info: None,
            }],
        );
        backend.set_expand_prompt_templates(false);

        backend.prompt("/review input".to_string()).expect("prompt");

        assert_eq!(
            backend.messages().expect("messages")[0].content,
            "/review input"
        );
    }

    #[test]
    fn managed_backend_updates_auto_retry_and_aborts_retry_like_pi_rpc() {
        let mut backend = test_backend();

        backend.set_auto_retry(false).expect("set auto retry");
        assert!(!backend.auto_retry_enabled());

        backend.abort_retry().expect("abort retry");
    }

    #[test]
    fn managed_backend_aborts_idle_session_and_bash_like_pi_rpc() {
        let mut backend = test_backend();

        backend.abort().expect("abort should be idempotent");
        backend
            .abort_bash()
            .expect("abort bash should be idempotent");

        let state = backend.state().expect("state");
        assert!(!state.is_streaming);
    }

    #[test]
    fn managed_backend_queues_steer_and_follow_up_like_pi_rpc() {
        let mut backend = test_backend();

        backend.steer("adjust".to_string()).expect("steer");
        backend.follow_up("next".to_string()).expect("follow up");

        let state = backend.state().expect("state");
        assert_eq!(state.pending_message_count, 2);
        assert!(backend.messages().expect("messages").is_empty());
    }

    #[test]
    fn managed_backend_new_session_replaces_current_session_like_pi_rpc() {
        let mut backend = test_backend();
        backend.prompt("old".to_string()).expect("prompt");
        backend.steer("pending".to_string()).expect("steer");

        let result = backend.new_session(None).expect("new session");

        assert_eq!(result["cancelled"], false);
        assert!(backend.messages().expect("messages").is_empty());
        assert_eq!(backend.state().expect("state").pending_message_count, 0);
    }

    #[test]
    fn managed_backend_invalidates_old_extension_runtime_after_new_session_like_pi_rpc() {
        let mut backend = test_backend();
        let runtime = crate::extensions::ExtensionRuntime::default();
        let mut runner = ExtensionRunner::new(Vec::new(), runtime.clone());
        backend.bind_extension_runner(&mut runner);

        backend.new_session(None).expect("new session");

        assert_eq!(
            runtime
                .assert_active()
                .expect_err("runtime should be stale"),
            crate::extensions::types::STALE_EXTENSION_CONTEXT_MESSAGE
        );
    }

    #[test]
    fn persisted_rpc_backend_new_session_records_parent_session_like_pi_rpc() {
        let dir = temp_dir();
        let session_manager = SessionManager::create("/tmp/project", Some(dir.clone()))
            .expect("session should create");
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session_manager, model_registry);
        backend.prompt("old".to_string()).expect("prompt");
        let old_file = backend
            .state()
            .expect("state")
            .session_file
            .expect("session file");

        let result = backend
            .new_session(Some(old_file.clone()))
            .expect("new session");

        let state = backend.state().expect("state");
        assert_eq!(result["cancelled"], false);
        assert!(backend.messages().expect("messages").is_empty());
        assert_ne!(state.session_file.as_deref(), Some(old_file.as_str()));
        assert_eq!(
            backend
                .session_manager()
                .storage_metadata()
                .parent_session_path
                .as_deref(),
            Some(old_file.as_str())
        );
        assert_eq!(backend.session_manager().session_dir(), dir.as_path());
    }

    #[test]
    fn persisted_rpc_backend_switches_session_like_pi_rpc() {
        let dir = temp_dir();
        let first_cwd = temp_dir();
        let second_cwd = temp_dir();
        let mut first = SessionManager::create(first_cwd, Some(dir.clone()))
            .expect("first session should create");
        first
            .append_message(AgentMessage::new(MessageRole::User, "first".to_string()))
            .expect("first message");
        let first_file = first.session_file().expect("first file").to_path_buf();
        let mut second = SessionManager::create(second_cwd.clone(), Some(dir))
            .expect("second session should create");
        second
            .append_message(AgentMessage::new(MessageRole::User, "second".to_string()))
            .expect("second message");
        let second_file = second.session_file().expect("second file").to_path_buf();
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(first, model_registry);

        let result = backend
            .switch_session(second_file.to_string_lossy().to_string())
            .expect("switch session");

        assert_eq!(result["cancelled"], false);
        assert_eq!(
            backend.state().expect("state").session_file.as_deref(),
            Some(second_file.to_string_lossy().as_ref())
        );
        assert_eq!(backend.session_manager().cwd(), second_cwd.as_path());
        assert_eq!(backend.messages().expect("messages")[0].content, "second");
        assert_ne!(
            backend.state().expect("state").session_file.as_deref(),
            Some(first_file.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn persisted_rpc_backend_rejects_switch_to_missing_cwd_like_pi() {
        let dir = temp_dir();
        let existing_cwd = temp_dir();
        let missing_cwd = dir.join("missing-cwd");
        let mut first =
            SessionManager::create(existing_cwd.clone(), Some(dir.clone())).expect("first session");
        first
            .append_message(AgentMessage::new(MessageRole::User, "first".to_string()))
            .expect("first message");
        let first_file = first.session_file().expect("first file").to_path_buf();
        let second =
            SessionManager::create(missing_cwd.clone(), Some(dir)).expect("second session");
        let second_file = second.session_file().expect("second file").to_path_buf();
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(first, model_registry);

        let error = backend
            .switch_session(second_file.to_string_lossy().to_string())
            .expect_err("missing cwd should fail");

        assert!(error.contains("Stored session working directory does not exist"));
        assert!(error.contains(&missing_cwd.to_string_lossy().to_string()));
        assert!(error.contains(&existing_cwd.to_string_lossy().to_string()));
        assert_eq!(
            backend.session_manager().session_file(),
            Some(first_file.as_path())
        );
        assert_eq!(backend.messages().expect("messages")[0].content, "first");
    }

    #[test]
    fn persisted_rpc_backend_imports_jsonl_into_current_session_dir_like_pi() {
        let source_dir = temp_dir();
        let target_dir = temp_dir();
        let cwd_override = temp_dir();
        let mut source =
            SessionManager::create("/tmp/source-project", Some(source_dir)).expect("source");
        source
            .append_message(AgentMessage::new(
                MessageRole::User,
                "imported request".to_string(),
            ))
            .expect("source message");
        let source_file = source.session_file().expect("source file").to_path_buf();
        let target = SessionManager::create("/tmp/target-project", Some(target_dir.clone()))
            .expect("target");
        let original_file = target.session_file().map(Path::to_path_buf);
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(target, model_registry);

        let result = backend
            .import_session(
                source_file.to_string_lossy().to_string(),
                Some(cwd_override.to_string_lossy().to_string()),
            )
            .expect("import");

        assert_eq!(result["cancelled"], false);
        let imported_path = target_dir.join(source_file.file_name().expect("source basename"));
        assert_eq!(
            result["sessionPath"].as_str(),
            Some(imported_path.to_string_lossy().as_ref())
        );
        assert_eq!(
            backend.session_manager().session_file(),
            Some(imported_path.as_path())
        );
        assert_eq!(backend.session_manager().cwd(), cwd_override.as_path());
        assert_ne!(
            backend.session_manager().session_file(),
            original_file.as_deref()
        );
        assert_eq!(
            backend.messages().expect("messages")[0].content,
            "imported request"
        );
    }

    #[test]
    fn persisted_rpc_backend_rejects_import_with_missing_cwd_without_override_like_pi() {
        let source_dir = temp_dir();
        let target_dir = temp_dir();
        let existing_cwd = temp_dir();
        let missing_cwd = source_dir.join("missing-cwd");
        let mut source =
            SessionManager::create(missing_cwd.clone(), Some(source_dir)).expect("source");
        source
            .append_message(AgentMessage::new(
                MessageRole::User,
                "imported request".to_string(),
            ))
            .expect("source message");
        let source_file = source.session_file().expect("source file").to_path_buf();
        let target =
            SessionManager::create(existing_cwd.clone(), Some(target_dir.clone())).expect("target");
        let original_file = target.session_file().map(Path::to_path_buf);
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(target, model_registry);

        let error = backend
            .import_session(source_file.to_string_lossy().to_string(), None)
            .expect_err("missing cwd should fail");

        assert!(error.contains("Stored session working directory does not exist"));
        assert!(error.contains(&missing_cwd.to_string_lossy().to_string()));
        assert!(error.contains(&existing_cwd.to_string_lossy().to_string()));
        assert_eq!(
            backend.session_manager().session_file(),
            original_file.as_deref()
        );
        assert!(target_dir
            .join(source_file.file_name().expect("source basename"))
            .exists());
    }

    #[test]
    fn persisted_rpc_backend_import_reports_missing_file_like_pi() {
        let dir = temp_dir();
        let session = SessionManager::create("/tmp/project", Some(dir.clone())).expect("session");
        let original_file = session.session_file().map(Path::to_path_buf);
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session, model_registry);
        let missing = dir.join("missing.jsonl");

        let error = backend
            .import_session(missing.to_string_lossy().to_string(), None)
            .expect_err("missing import should fail");

        assert!(error.contains("Import file not found"));
        assert_eq!(
            backend.session_manager().session_file(),
            original_file.as_deref()
        );
    }

    #[test]
    fn managed_backend_expands_queued_prompt_templates_like_pi_rpc() {
        let mut backend = test_backend();
        backend.set_prompt_resources(
            Vec::new(),
            vec![PromptTemplate {
                name: "review".to_string(),
                description: None,
                argument_hint: None,
                content: "Review $ARGUMENTS".to_string(),
                file_path: "/prompts/review.md".to_string(),
                source_info: None,
            }],
        );

        backend
            .steer("/review src/lib.rs".to_string())
            .expect("steer");

        assert_eq!(
            backend.queued_steering_messages(),
            &["Review src/lib.rs".to_string()]
        );
        assert_eq!(backend.state().expect("state").pending_message_count, 1);
        assert!(backend.messages().expect("messages").is_empty());
    }

    #[test]
    fn managed_backend_executes_extension_command_without_appending_prompt() {
        use crate::source_info::create_synthetic_source_info;
        use std::sync::{Arc, Mutex};

        let seen_args = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen_args_for_handler = seen_args.clone();
        let command = ResolvedCommand {
            invocation_name: "demo".to_string(),
            command: crate::extensions::RegisteredCommand {
                name: "demo".to_string(),
                description: Some("Demo command".to_string()),
                handler: Arc::new(move |ctx| {
                    *seen_args_for_handler.lock().expect("args lock") = ctx.args;
                    Ok(())
                }),
                source_info: create_synthetic_source_info(
                    "/extensions/demo.ts",
                    "local",
                    None,
                    None,
                    None,
                ),
            },
        };
        let mut backend = test_backend();
        backend.set_extension_commands(vec![command]);

        backend
            .prompt(r#"/demo "two words" now"#.to_string())
            .expect("command should run");

        assert!(backend.messages().expect("messages").is_empty());
        assert_eq!(
            *seen_args.lock().expect("args lock"),
            vec!["two words".to_string(), "now".to_string()]
        );
    }

    #[test]
    fn managed_backend_appends_unknown_extension_command_as_prompt() {
        let mut backend = test_backend();

        backend.prompt("/unknown arg".to_string()).expect("prompt");

        assert_eq!(
            backend.messages().expect("messages")[0].content,
            "/unknown arg"
        );
    }

    #[test]
    fn managed_backend_forks_before_user_message_like_pi_rpc() {
        let mut backend = test_backend();
        backend.prompt("first".to_string()).expect("first prompt");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "answer".to_string(),
            ))
            .expect("assistant message");
        let fork_entry = backend
            .session_manager_mut()
            .append_message(AgentMessage::new(MessageRole::User, "second".to_string()))
            .expect("second message");

        let result = backend
            .fork(fork_entry, ForkPosition::Before)
            .expect("fork");

        assert_eq!(result["cancelled"], false);
        assert_eq!(result["text"], "second");
        assert_eq!(
            backend.messages().expect("messages"),
            vec![
                AgentMessage::new(MessageRole::User, "first".to_string()),
                AgentMessage::new(MessageRole::Assistant, "answer".to_string()),
            ]
        );
    }

    #[test]
    fn managed_backend_clones_current_leaf_like_pi_rpc() {
        let mut backend = test_backend();
        backend.prompt("first".to_string()).expect("first prompt");

        let result = backend.clone_session().expect("clone");

        assert_eq!(result["cancelled"], false);
        assert_eq!(backend.messages().expect("messages").len(), 1);
    }

    #[test]
    fn managed_backend_rejects_clone_without_current_leaf_like_pi_rpc() {
        let mut backend = test_backend();

        let error = backend
            .clone_session()
            .expect_err("clone without leaf should fail");

        assert_eq!(error, "Cannot clone session: no current entry selected");
    }

    #[test]
    fn managed_backend_compact_uses_model_provider_instead_of_placeholder_like_pi() {
        let mut backend = test_backend_with_models(vec![echo_model()]);
        backend
            .set_model("local".to_string(), "echo".to_string())
            .expect("set echo model");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::User,
                "first request".to_string(),
            ))
            .expect("first prompt");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "first answer".to_string(),
            ))
            .expect("assistant");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::User,
                "second request".to_string(),
            ))
            .expect("second prompt");

        let result = backend
            .compact(Some("focus on requests".to_string()))
            .expect("compact");

        assert_ne!(
            result["summary"],
            "Compaction placeholder summary for 2 messages."
        );
        assert!(result["summary"]
            .as_str()
            .expect("summary")
            .contains("Additional focus: focus on requests"));
        assert!(result["details"]["readFiles"].is_array());
        let entries = backend.session_manager().branch(None).expect("branch");
        let compaction = entries
            .iter()
            .find(|entry| matches!(entry, agent::harness::SessionTreeEntry::Compaction { .. }))
            .expect("compaction entry");
        match compaction {
            agent::harness::SessionTreeEntry::Compaction {
                summary, from_hook, ..
            } => {
                assert!(summary.contains("Additional focus: focus on requests"));
                assert!(!from_hook);
            }
            _ => unreachable!("matched compaction"),
        }
    }

    #[test]
    fn managed_backend_navigate_tree_uses_model_provider_for_branch_summary_like_pi() {
        let mut backend = test_backend_with_models(vec![echo_model()]);
        backend
            .set_model("local".to_string(), "echo".to_string())
            .expect("set echo model");
        let root_user = backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::User,
                "root request".to_string(),
            ))
            .expect("root user");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "root answer".to_string(),
            ))
            .expect("root assistant");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::User,
                "branch request".to_string(),
            ))
            .expect("branch user");

        let result = backend
            .navigate_tree(
                root_user,
                true,
                Some("focus on branch request".to_string()),
                false,
                None,
            )
            .expect("navigate tree");

        assert_eq!(result["cancelled"], false);
        let summary = result["summaryEntry"]["summary"].as_str().expect("summary");
        assert_ne!(summary, "Branch summary placeholder for 1 entries.");
        assert!(summary.contains(agent::harness::BRANCH_SUMMARY_PREAMBLE));
        assert!(summary.contains("Additional focus: focus on branch request"));
        let summary_id = result["summaryEntry"]["id"].as_str().expect("summary id");
        match backend
            .session_manager()
            .entry(summary_id)
            .expect("summary entry")
        {
            agent::harness::SessionTreeEntry::BranchSummary {
                summary,
                details,
                from_hook,
                ..
            } => {
                assert!(summary.contains("Additional focus: focus on branch request"));
                assert_eq!(
                    details.as_ref().and_then(|value| value.get("readFiles")),
                    Some(&serde_json::json!([]))
                );
                assert!(!from_hook);
            }
            _ => panic!("expected branch summary"),
        }
    }

    #[test]
    fn managed_backend_export_html_routes_jsonl_paths_to_branch_export_like_pi() {
        let dir = temp_dir();
        let out = dir.join("branch.jsonl");
        let session_manager = SessionManager::in_memory(dir.clone());
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        let mut backend = ManagedRpcSessionBackend::new(session_manager, model_registry);
        let first = backend
            .session_manager_mut()
            .append_message(AgentMessage::new(MessageRole::User, "first".to_string()))
            .expect("first");
        let branch = backend
            .session_manager_mut()
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "branch".to_string(),
            ))
            .expect("branch");
        backend
            .session_manager_mut()
            .move_to(Some(first))
            .expect("move");
        backend
            .session_manager_mut()
            .append_message(AgentMessage::new(MessageRole::User, "second".to_string()))
            .expect("second");

        let path = backend
            .export_html(Some(out.to_string_lossy().to_string()))
            .expect("export");

        assert_eq!(path, out.to_string_lossy());
        let records = std::fs::read_to_string(&out)
            .expect("jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("record"))
            .collect::<Vec<_>>();
        assert_eq!(records[0]["type"], "session");
        assert_eq!(records[1]["parentId"], serde_json::Value::Null);
        assert_eq!(records[2]["parentId"], records[1]["id"]);
        assert!(records.iter().all(|record| record["id"] != branch));
    }

    #[test]
    fn parses_extension_command_invocation_like_pi() {
        assert_eq!(
            prompt_input::parse_extension_command_invocation("/demo one two"),
            Some(("demo", "one two"))
        );
        assert_eq!(
            prompt_input::parse_extension_command_invocation("/demo"),
            Some(("demo", ""))
        );
        assert_eq!(
            prompt_input::parse_extension_command_invocation("demo"),
            None
        );
    }

    fn test_backend() -> ManagedRpcSessionBackend<InMemorySessionStorage, InMemoryAuthStorageBackend>
    {
        let session_manager = SessionManager::in_memory("/tmp/project");
        let auth_storage = AuthStorage::in_memory(AuthStorageData::new());
        let model_registry = ModelRegistry::in_memory(auth_storage);
        ManagedRpcSessionBackend::new(session_manager, model_registry)
    }

    fn test_backend_with_models(
        models: Vec<Model>,
    ) -> ManagedRpcSessionBackend<InMemorySessionStorage, InMemoryAuthStorageBackend> {
        let session_manager = SessionManager::in_memory("/tmp/project");
        let mut auth_data = AuthStorageData::new();
        auth_data.insert(
            "test".to_string(),
            crate::auth_storage::AuthCredential::ApiKey {
                key: "test-key".to_string(),
            },
        );
        let auth_storage = AuthStorage::in_memory(auth_data);
        let model_registry = ModelRegistry::in_memory(auth_storage).with_models(models);
        ManagedRpcSessionBackend::new(session_manager, model_registry)
    }

    fn plain_model() -> Model {
        Model {
            provider: "test".to_string(),
            id: "plain".to_string(),
            display_name: "Plain".to_string(),
            ..Model::default()
        }
    }

    fn echo_model() -> Model {
        Model {
            provider: "local".to_string(),
            id: "echo".to_string(),
            api: "local-echo".to_string(),
            display_name: "Echo".to_string(),
            context_window: 100_000,
            max_tokens: Some(8_000),
            ..Model::default()
        }
    }

    fn reasoning_model() -> Model {
        let mut model = plain_model();
        model.id = "reasoning".to_string();
        model.display_name = "Reasoning".to_string();
        model.reasoning = Some(ai::ModelReasoning { enabled: true });
        model
            .thinking_level_map
            .insert(ModelThinkingLevel::XHigh, Some("xhigh".to_string()));
        model
    }

    struct RecordingClipboardRunner {
        copied: Arc<Mutex<Vec<String>>>,
    }

    impl ClipboardRunner for RecordingClipboardRunner {
        fn run_command(&mut self, _command: &ClipboardCommand, text: &str) -> bool {
            self.copied
                .lock()
                .expect("copied lock")
                .push(text.to_string());
            true
        }

        fn emit_osc52(&mut self, _sequence: &str) -> bool {
            false
        }
    }

    struct RecordingShareCommandRunner {
        outputs: Vec<Result<ShareCommandOutput, String>>,
        calls: Arc<Mutex<Vec<(String, Vec<String>)>>>,
    }

    impl ShareCommandRunner for RecordingShareCommandRunner {
        fn run(&mut self, program: &str, args: &[String]) -> Result<ShareCommandOutput, String> {
            self.calls
                .lock()
                .expect("calls lock")
                .push((program.to_string(), args.to_vec()));
            self.outputs.remove(0)
        }
    }

    fn share_output(stdout: &str, stderr: &str, status: Option<i32>) -> ShareCommandOutput {
        ShareCommandOutput {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            status,
        }
    }

    fn temp_dir() -> PathBuf {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-rpc-session-backend-{millis}"));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
