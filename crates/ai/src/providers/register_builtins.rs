use crate::{AssistantStopReason, Model, RichAssistantMessage, Usage};

/// 复刻 pi 的 Node-only provider 动态导入规则：构建后的 JS 运行时使用 .js 后缀。
pub fn node_only_provider_specifier(runtime_url: &str, specifier: &str) -> String {
    if runtime_url.ends_with(".js") && specifier.ends_with(".ts") {
        let prefix = specifier.trim_end_matches(".ts");
        format!("{prefix}.js")
    } else {
        specifier.to_string()
    }
}

/// 复刻 pi 懒加载 provider 失败时生成的 assistant 错误消息结构。
pub fn lazy_load_error_rich_message(
    model: &Model,
    error_message: impl Into<String>,
    timestamp_millis: u128,
) -> RichAssistantMessage {
    RichAssistantMessage {
        content: Vec::new(),
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        response_model: None,
        response_id: None,
        usage: Usage::default(),
        stop_reason: AssistantStopReason::Error,
        error_message: Some(error_message.into()),
        diagnostics: Vec::new(),
        timestamp_millis,
    }
}

#[cfg(test)]
mod tests {
    use super::{lazy_load_error_rich_message, node_only_provider_specifier};
    use crate::{AssistantStopReason, Model};

    #[test]
    fn node_only_provider_specifier_uses_js_for_built_runtime() {
        assert_eq!(
            node_only_provider_specifier(
                "file:///dist/register-builtins.js",
                "./amazon-bedrock.ts"
            ),
            "./amazon-bedrock.js"
        );
        assert_eq!(
            node_only_provider_specifier("file:///src/register-builtins.ts", "./amazon-bedrock.ts"),
            "./amazon-bedrock.ts"
        );
        assert_eq!(
            node_only_provider_specifier(
                "file:///dist/register-builtins.js",
                "./amazon-bedrock.js"
            ),
            "./amazon-bedrock.js"
        );
    }

    #[test]
    fn lazy_load_error_message_preserves_model_identity_and_zero_usage() {
        let model = Model {
            id: "gpt-test".to_string(),
            provider: "openai".to_string(),
            api: "openai-responses".to_string(),
            ..Model::default()
        };

        let message = lazy_load_error_rich_message(&model, "load failed", 123);

        assert!(message.content.is_empty());
        assert_eq!(message.api, "openai-responses");
        assert_eq!(message.provider, "openai");
        assert_eq!(message.model, "gpt-test");
        assert_eq!(message.usage.input, 0);
        assert_eq!(message.usage.output, 0);
        assert_eq!(message.usage.cache_read, 0);
        assert_eq!(message.usage.cache_write, 0);
        assert_eq!(message.usage.total_tokens, 0);
        assert_eq!(message.usage.cost.total, 0.0);
        assert_eq!(message.stop_reason, AssistantStopReason::Error);
        assert_eq!(message.error_message.as_deref(), Some("load failed"));
        assert_eq!(message.timestamp_millis, 123);
    }
}
