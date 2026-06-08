use serde::{Deserialize, Serialize};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::{
    validate_images_model, AiError, AiResult, AssistantImages, ContentBlock, ImagesContext,
    ImagesModel, ImagesProvider, ImagesStopReason, ModelInputKind, Usage, UsageCost,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRouterImagesConfig {
    pub api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OpenRouterImagesProvider {
    config: OpenRouterImagesConfig,
}

impl OpenRouterImagesProvider {
    pub fn new(config: OpenRouterImagesConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(OpenRouterImagesConfig {
            api_key: env::var("OPENROUTER_API_KEY").ok(),
        })
    }

    fn api_key(&self) -> AiResult<String> {
        let key = self.config.api_key.as_deref().unwrap_or_default().trim();
        if key.is_empty() {
            return Err(AiError::MissingApiKey("OPENROUTER_API_KEY".to_string()));
        }
        Ok(key.to_string())
    }
}

impl ImagesProvider for OpenRouterImagesProvider {
    fn generate_images(
        &self,
        model: ImagesModel,
        context: ImagesContext,
    ) -> AiResult<AssistantImages> {
        validate_images_model(&model)?;
        let mut output = assistant_images_response(&model);
        let api_key = match self.api_key() {
            Ok(api_key) => api_key,
            Err(error) => {
                output.stop_reason = ImagesStopReason::Error;
                output.error_message = Some(error.to_string());
                return Ok(output);
            }
        };
        let url = format!("{}/chat/completions", model.base_url.trim_end_matches('/'));
        let payload = OpenRouterImagesRequest {
            model: model.id.clone(),
            messages: vec![OpenRouterImagesMessage {
                role: "user".to_string(),
                content: context
                    .input
                    .into_iter()
                    .map(openrouter_content_part)
                    .collect(),
            }],
            stream: false,
            modalities: if model.output.contains(&ModelInputKind::Text) {
                vec!["image".to_string(), "text".to_string()]
            } else {
                vec!["image".to_string()]
            },
        };

        let client = reqwest::blocking::Client::new();
        let mut request = client.post(url).bearer_auth(api_key).json(&payload);
        for (key, value) in &model.headers {
            request = request.header(key, value);
        }
        let response = match request.send() {
            Ok(response) => response,
            Err(error) => {
                output.stop_reason = ImagesStopReason::Error;
                output.error_message = Some(error.to_string());
                return Ok(output);
            }
        };
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            output.stop_reason = ImagesStopReason::Error;
            output.error_message = Some(format!("status={status}, body={body}"));
            return Ok(output);
        }
        let response = match response.json::<OpenRouterImagesResponse>() {
            Ok(response) => response,
            Err(error) => {
                output.stop_reason = ImagesStopReason::Error;
                output.error_message = Some(error.to_string());
                return Ok(output);
            }
        };

        output.output = extract_output(&response);
        output.response_id = response.id;
        output.usage = response.usage.map(|usage| parse_usage(usage, &model));
        Ok(output)
    }
}

fn assistant_images_response(model: &ImagesModel) -> AssistantImages {
    AssistantImages {
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        output: Vec::new(),
        response_id: None,
        usage: None,
        stop_reason: ImagesStopReason::Stop,
        error_message: None,
        timestamp_millis: now_millis(),
    }
}

#[derive(Debug, Serialize)]
struct OpenRouterImagesRequest {
    model: String,
    messages: Vec<OpenRouterImagesMessage>,
    stream: bool,
    modalities: Vec<String>,
}

#[derive(Debug, Serialize)]
struct OpenRouterImagesMessage {
    role: String,
    content: Vec<OpenRouterContentPart>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum OpenRouterContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: OpenRouterImageUrl },
}

#[derive(Debug, Serialize)]
struct OpenRouterImageUrl {
    url: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterImagesResponse {
    id: Option<String>,
    choices: Vec<OpenRouterImagesChoice>,
    usage: Option<OpenRouterImagesUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterImagesChoice {
    message: OpenRouterImagesResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OpenRouterImagesResponseMessage {
    content: Option<String>,
    images: Option<Vec<OpenRouterGeneratedImage>>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterGeneratedImage {
    image_url: Option<OpenRouterGeneratedImageUrl>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OpenRouterGeneratedImageUrl {
    String(String),
    Object { url: Option<String> },
}

#[derive(Debug, Deserialize)]
struct OpenRouterImagesUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    prompt_tokens_details: Option<OpenRouterPromptTokenDetails>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterPromptTokenDetails {
    cached_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
}

fn openrouter_content_part(block: ContentBlock) -> OpenRouterContentPart {
    match block {
        ContentBlock::Text { text } => OpenRouterContentPart::Text { text },
        ContentBlock::Image { data, mime_type } => OpenRouterContentPart::ImageUrl {
            image_url: OpenRouterImageUrl {
                url: format!("data:{mime_type};base64,{data}"),
            },
        },
    }
}

fn extract_output(response: &OpenRouterImagesResponse) -> Vec<ContentBlock> {
    let Some(choice) = response.choices.first() else {
        return Vec::new();
    };
    let mut output = Vec::new();
    if let Some(text) = choice
        .message
        .content
        .as_ref()
        .filter(|content| !content.is_empty())
    {
        output.push(ContentBlock::Text { text: text.clone() });
    }
    for image in choice.message.images.as_deref().unwrap_or_default() {
        let Some(url) = image.image_url.as_ref().and_then(image_url_value) else {
            continue;
        };
        if let Some((mime_type, data)) = parse_data_url(url) {
            output.push(ContentBlock::Image { data, mime_type });
        }
    }
    output
}

fn image_url_value(value: &OpenRouterGeneratedImageUrl) -> Option<&str> {
    match value {
        OpenRouterGeneratedImageUrl::String(value) => Some(value.as_str()),
        OpenRouterGeneratedImageUrl::Object { url } => url.as_deref(),
    }
}

fn parse_data_url(value: &str) -> Option<(String, String)> {
    let rest = value.strip_prefix("data:")?;
    let (mime_type, data) = rest.split_once(";base64,")?;
    Some((mime_type.to_string(), data.to_string()))
}

fn parse_usage(raw: OpenRouterImagesUsage, model: &ImagesModel) -> Usage {
    let prompt_tokens = raw.prompt_tokens.unwrap_or_default();
    let output = raw.completion_tokens.unwrap_or_default();
    let cached_tokens = raw
        .prompt_tokens_details
        .as_ref()
        .and_then(|details| details.cached_tokens)
        .unwrap_or_default();
    let cache_write = raw
        .prompt_tokens_details
        .and_then(|details| details.cache_write_tokens)
        .unwrap_or_default();
    let cache_read = if cache_write > 0 {
        cached_tokens.saturating_sub(cache_write)
    } else {
        cached_tokens
    };
    let input = prompt_tokens.saturating_sub(cache_read + cache_write);
    let mut cost = UsageCost {
        input: model.cost.input / 1_000_000.0 * input as f64,
        output: model.cost.output / 1_000_000.0 * output as f64,
        cache_read: model.cost.cache_read / 1_000_000.0 * cache_read as f64,
        cache_write: model.cost.cache_write / 1_000_000.0 * cache_write as f64,
        total: 0.0,
    };
    cost.total = cost.input + cost.output + cost.cache_read + cost.cache_write;
    Usage {
        input,
        output,
        cache_read,
        cache_write,
        total_tokens: input + output + cache_read + cache_write,
        cost,
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ModelCost;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn parses_data_url_images_from_openrouter_response() {
        let response: OpenRouterImagesResponse = serde_json::from_value(serde_json::json!({
            "id": "resp_1",
            "choices": [{
                "message": {
                    "content": "caption",
                    "images": [{"image_url": {"url": "data:image/png;base64,abc"}}]
                }
            }]
        }))
        .expect("response should parse");
        let output = extract_output(&response);
        assert_eq!(output.len(), 2);
        assert!(matches!(output[1], ContentBlock::Image { .. }));
    }

    #[test]
    fn parses_openrouter_image_usage_cost() {
        let model = ImagesModel {
            id: "openrouter/auto".to_string(),
            provider: "openrouter".to_string(),
            api: "openrouter-images".to_string(),
            display_name: "Auto Router".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            input: vec![ModelInputKind::Text],
            output: vec![ModelInputKind::Image],
            headers: Default::default(),
            cost: ModelCost {
                input: 10.0,
                output: 20.0,
                cache_read: 1.0,
                cache_write: 2.0,
            },
        };
        let usage = parse_usage(
            OpenRouterImagesUsage {
                prompt_tokens: Some(100),
                completion_tokens: Some(50),
                prompt_tokens_details: Some(OpenRouterPromptTokenDetails {
                    cached_tokens: Some(20),
                    cache_write_tokens: Some(5),
                }),
            },
            &model,
        );
        assert_eq!(usage.input, 80);
        assert_eq!(usage.cache_read, 15);
        assert_eq!(usage.cache_write, 5);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn returns_error_response_for_openrouter_http_failures_like_pi() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer).expect("read");
            stream
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 4\r\n\r\nboom")
                .expect("write");
        });

        let provider = OpenRouterImagesProvider::new(OpenRouterImagesConfig {
            api_key: Some("key".to_string()),
        });
        let response = provider
            .generate_images(
                ImagesModel {
                    id: "openrouter/auto".to_string(),
                    provider: "openrouter".to_string(),
                    api: "openrouter-images".to_string(),
                    display_name: "Auto Router".to_string(),
                    base_url: format!("http://{address}"),
                    input: vec![ModelInputKind::Text],
                    output: vec![ModelInputKind::Image],
                    headers: Default::default(),
                    cost: ModelCost::default(),
                },
                ImagesContext {
                    input: vec![ContentBlock::Text {
                        text: "generate".to_string(),
                    }],
                },
            )
            .expect("provider errors should be represented in AssistantImages");

        handle.join().expect("server thread");
        assert!(matches!(response.stop_reason, ImagesStopReason::Error));
        assert!(response.output.is_empty());
        assert!(response
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("status=500")));
    }
}
