use agent::harness::{
    InMemorySessionStorage, JsonlSessionStorage, PromptTemplate, SessionStorage, Skill,
};
use agent::AgentMessage;
use ai::{clamp_thinking_level, supported_thinking_levels, MessageRole, Model, ModelThinkingLevel};
use std::path::PathBuf;

use crate::auth_storage::AuthStorageBackend;
use crate::bash_executor::{execute_bash, BashResult};
use crate::export_html::{export_session_to_html, ExportOptions};
use crate::extensions::ResolvedCommand;
use crate::model_registry::ModelRegistry;
use crate::rpc::dispatcher::RpcSessionBackend;
use crate::rpc::types::{QueueMode, RpcSessionState, RpcSlashCommand};
use crate::session_manager::{ForkMessage, SessionManager, SessionStats};
use crate::slash_commands::SlashCommandInfo;

use super::command_registry::RpcCommandRegistry;
use super::prompt_input::PromptInputProcessor;

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
    ) -> Result<(), String>
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
    ) -> Result<(), String> {
        Err("switch_session is not supported by in-memory RPC sessions".to_string())
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
    ) -> Result<(), String> {
        manager.switch_to_session(session_path)
    }
}

pub struct ManagedRpcSessionBackend<S: SessionStorage, B: AuthStorageBackend> {
    session_manager: SessionManager<S>,
    model_registry: ModelRegistry<B>,
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
}

impl<S: SessionStorage, B: AuthStorageBackend> ManagedRpcSessionBackend<S, B> {
    pub fn new(session_manager: SessionManager<S>, model_registry: ModelRegistry<B>) -> Self {
        let model = model_registry.get_available().into_iter().next();
        Self {
            session_manager,
            model_registry,
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

    pub fn with_extension_commands(mut self, commands: Vec<ResolvedCommand>) -> Self {
        self.set_extension_commands(commands);
        self
    }

    pub fn set_extension_commands(&mut self, commands: Vec<ResolvedCommand>) {
        self.prompt_input.set_extension_commands(commands);
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
}

impl<S: RpcSessionLifecycle, B: AuthStorageBackend> RpcSessionBackend
    for ManagedRpcSessionBackend<S, B>
{
    fn prompt(&mut self, message: String) -> Result<(), String> {
        if self.prompt_input.try_execute_extension_command(&message)? {
            return Ok(());
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
        S::replace_with_new_session(&mut self.session_manager, parent_session)?;
        self.steering_messages.clear();
        self.follow_up_messages.clear();
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

    fn export_html(&mut self, output_path: Option<String>) -> Result<String, String> {
        let path = output_path.map(PathBuf::from);
        export_session_to_html(
            &self.session_manager,
            ExportOptions {
                output_path: path,
                ..ExportOptions::default()
            },
        )
        .map(|path| path.to_string_lossy().to_string())
    }

    fn switch_session(&mut self, session_path: String) -> Result<serde_json::Value, String> {
        S::switch_to_session(&mut self.session_manager, session_path)?;
        self.steering_messages.clear();
        self.follow_up_messages.clear();
        Ok(serde_json::json!({
            "cancelled": false,
        }))
    }

    fn fork(&mut self, entry_id: String) -> Result<serde_json::Value, String> {
        let text = self.session_manager.fork_before_user_message(&entry_id)?;
        Ok(serde_json::json!({
            "text": text,
            "cancelled": false,
        }))
    }

    fn clone_session(&mut self) -> Result<serde_json::Value, String> {
        self.session_manager.clone_at_leaf()?;
        Ok(serde_json::json!({
            "cancelled": false,
        }))
    }

    fn fork_messages(&self) -> Result<Vec<ForkMessage>, String> {
        Ok(self.session_manager.fork_messages())
    }

    fn last_assistant_text(&self) -> Result<Option<String>, String> {
        Ok(self.session_manager.last_assistant_text())
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
    use agent::harness::InMemorySessionStorage;

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
        let mut first = SessionManager::create("/tmp/first", Some(dir.clone()))
            .expect("first session should create");
        first
            .append_message(AgentMessage::new(MessageRole::User, "first".to_string()))
            .expect("first message");
        let first_file = first.session_file().expect("first file").to_path_buf();
        let mut second =
            SessionManager::create("/tmp/second", Some(dir)).expect("second session should create");
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
        assert_eq!(
            backend.session_manager().cwd(),
            std::path::Path::new("/tmp/second")
        );
        assert_eq!(backend.messages().expect("messages")[0].content, "second");
        assert_ne!(
            backend.state().expect("state").session_file.as_deref(),
            Some(first_file.to_string_lossy().as_ref())
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

        let result = backend.fork(fork_entry).expect("fork");

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
