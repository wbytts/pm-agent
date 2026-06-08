use std::collections::BTreeMap;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::conversation::{
    AssistantContentBlock, RichMessage, TextContent, UserContentBlock, UserMessageContent,
};
pub use crate::providers::bedrock_messages::{
    bedrock_image_format, build_bedrock_system_prompt, convert_bedrock_messages,
    convert_bedrock_tool_config, normalize_bedrock_tool_call_id,
};
pub use crate::providers::bedrock_stream::{
    bedrock_stream_events_from_process_result, map_bedrock_stop_reason,
    parse_bedrock_converse_event_stream_body, process_bedrock_stream_events,
};
use crate::providers::bedrock_types::{
    BedrockAdditionalModelRequestFields, BedrockCacheRetention, BedrockEndpointDecision,
    BedrockMessage, BedrockMessagesContext, BedrockOptions, BedrockSystemContentBlock,
    BedrockThinkingDisplay, BedrockToolChoice, BedrockToolConfiguration, BedrockToolDefinition,
};
use crate::providers::{
    adjust_max_tokens_for_thinking, clamp_reasoning, SimpleStreamOptions, ThinkingBudgets,
    ThinkingTokenBudget,
};
use crate::types::{
    validate_model, AiError, AiResult, AssistantStopReason, Message, MessageRole, Model,
    ModelThinkingLevel, StreamEvent, StreamRequest, Usage,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BedrockConverseConfig {
    pub region: Option<String>,
    pub profile: Option<String>,
    pub bearer_token: Option<String>,
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BedrockConverseProvider {
    config: BedrockConverseConfig,
}

impl BedrockConverseProvider {
    pub fn new(config: BedrockConverseConfig) -> Self {
        Self { config }
    }

    pub fn from_env() -> Self {
        Self::new(BedrockConverseConfig {
            region: env::var("AWS_REGION")
                .or_else(|_| env::var("AWS_DEFAULT_REGION"))
                .ok(),
            profile: env::var("AWS_PROFILE").ok(),
            bearer_token: env::var("AWS_BEARER_TOKEN_BEDROCK").ok(),
            base_url: env::var("AWS_BEDROCK_BASE_URL")
                .unwrap_or_else(|_| "https://bedrock-runtime.us-east-1.amazonaws.com".to_string()),
        })
    }

    fn has_auth(&self) -> bool {
        env::var("AWS_BEDROCK_SKIP_AUTH").ok().as_deref() == Some("1")
            || self
                .config
                .bearer_token
                .as_deref()
                .is_some_and(|token| !token.trim().is_empty())
            || env::var("AWS_ACCESS_KEY_ID")
                .ok()
                .is_some_and(|value| !value.trim().is_empty())
            || env::var("AWS_PROFILE")
                .ok()
                .is_some_and(|value| !value.trim().is_empty())
            || self
                .config
                .profile
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }

    fn stream_with_aws_credentials(
        &self,
        request: StreamRequest,
        explicit_credentials: Option<AwsCredentials>,
        amz_date_override: Option<&str>,
    ) -> AiResult<Vec<StreamEvent>> {
        validate_model(&request.model)?;
        if !self.has_auth() && explicit_credentials.is_none() {
            return Err(AiError::MissingApiKey(
                "AWS_BEARER_TOKEN_BEDROCK 或 AWS_ACCESS_KEY_ID/AWS_PROFILE".to_string(),
            ));
        }

        let rich_messages = request.rich_messages;
        let (system_prompt, messages) = split_bedrock_system_messages(request.messages);
        let context_messages = if rich_messages.is_empty() {
            messages
                .into_iter()
                .map(simple_message_to_bedrock_rich_message)
                .collect()
        } else {
            rich_messages
        };
        let context = BedrockMessagesContext {
            messages: context_messages,
        };
        let options = bedrock_options_from_metadata(&request.metadata, request.model.max_tokens);
        let cache_retention = resolve_bedrock_cache_retention(
            options.cache_retention,
            env::var("AWS_BEDROCK_FORCE_CACHE").ok().as_deref() == Some("1"),
        );
        let messages = convert_bedrock_messages(
            &request.model,
            &context,
            cache_retention,
            env::var("AWS_BEDROCK_FORCE_CACHE").ok().as_deref() == Some("1"),
        )
        .map_err(AiError::InvalidResponse)?;
        let system = build_bedrock_system_prompt(
            system_prompt.as_deref(),
            &request.model,
            cache_retention,
            env::var("AWS_BEDROCK_FORCE_CACHE").ok().as_deref() == Some("1"),
        );
        let tool_config = convert_bedrock_tool_config(
            &request
                .tools
                .into_iter()
                .map(|tool| BedrockToolDefinition {
                    name: tool.name,
                    description: tool.description,
                    parameters: tool.parameters,
                })
                .collect::<Vec<_>>(),
            options.tool_choice.as_ref(),
        );
        let additional_model_request_fields = build_bedrock_additional_model_request_fields(
            &request.model,
            &options,
            is_gov_cloud_bedrock_target(&request.model, self.config.region.as_deref()),
        );
        let payload = bedrock_converse_stream_payload(
            &request.model.id,
            messages,
            system,
            options.max_tokens,
            options.temperature,
            tool_config,
            additional_model_request_fields,
            &options.request_metadata,
        )
        .map_err(AiError::InvalidResponse)?;

        let bearer_token = self
            .config
            .bearer_token
            .as_deref()
            .filter(|token| !token.trim().is_empty());
        let skip_auth = env::var("AWS_BEDROCK_SKIP_AUTH").ok().as_deref() == Some("1");
        let url = bedrock_converse_stream_url(&self.config.base_url, &request.model.id);
        let client = reqwest::blocking::Client::new();
        let mut request_builder = client
            .post(&url)
            .header("content-type", "application/json")
            .header("accept", "application/vnd.amazon.eventstream");
        if let Some(token) = bearer_token {
            request_builder = request_builder.bearer_auth(token).json(&payload);
        } else if skip_auth {
            request_builder = request_builder.json(&payload);
        } else {
            let credentials = explicit_credentials
                .or_else(resolve_aws_env_credentials)
                .ok_or_else(|| {
                    AiError::Http(
                        "Bedrock SigV4 runtime 需要 AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY"
                            .to_string(),
                    )
                })?;
            let region = self.config.region.as_deref().unwrap_or("us-east-1");
            let body = serde_json::to_string(&payload)
                .map_err(|error| AiError::InvalidResponse(error.to_string()))?;
            let amz_date = amz_date_override
                .map(str::to_string)
                .unwrap_or_else(current_amz_date);
            let signed = sign_bedrock_sigv4_request(&url, region, &credentials, &amz_date, &body)
                .map_err(AiError::InvalidResponse)?;
            request_builder = request_builder
                .header("host", signed.host)
                .header("x-amz-date", signed.amz_date)
                .header("x-amz-content-sha256", signed.payload_hash)
                .header("authorization", signed.authorization)
                .body(body);
            if let Some(token) = signed.security_token {
                request_builder = request_builder.header("x-amz-security-token", token);
            }
        }

        let response = request_builder
            .send()
            .map_err(|error| AiError::Http(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(AiError::Http(format!("status={status}, body={body}")));
        }
        let body = response
            .bytes()
            .map_err(|error| AiError::InvalidResponse(error.to_string()))?;
        let events =
            parse_bedrock_converse_event_stream_body(&body).map_err(AiError::InvalidResponse)?;
        let result = process_bedrock_stream_events(
            &events,
            empty_bedrock_assistant(&request.model),
            None::<fn(&mut Usage)>,
        )
        .map_err(AiError::InvalidResponse)?;
        Ok(bedrock_stream_events_from_process_result(result))
    }

    #[cfg(test)]
    fn stream_with_aws_credentials_for_test(
        &self,
        request: StreamRequest,
        credentials: AwsCredentials,
        amz_date: &str,
    ) -> AiResult<Vec<StreamEvent>> {
        self.stream_with_aws_credentials(request, Some(credentials), Some(amz_date))
    }
}

impl crate::types::LanguageModelProvider for BedrockConverseProvider {
    fn stream(&self, request: StreamRequest) -> AiResult<Vec<StreamEvent>> {
        self.stream_with_aws_credentials(request, None, None)
    }
}

pub fn bedrock_converse_stream_url(base_url: &str, model_id: &str) -> String {
    format!(
        "{}/model/{}/converse-stream",
        base_url.trim_end_matches('/'),
        model_id
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SignedBedrockRequest {
    host: String,
    amz_date: String,
    payload_hash: String,
    authorization: String,
    security_token: Option<String>,
}

fn resolve_aws_env_credentials() -> Option<AwsCredentials> {
    let access_key_id = env::var("AWS_ACCESS_KEY_ID").ok()?;
    let secret_access_key = env::var("AWS_SECRET_ACCESS_KEY").ok()?;
    let access_key_id = access_key_id.trim();
    let secret_access_key = secret_access_key.trim();
    if access_key_id.is_empty() || secret_access_key.is_empty() {
        return None;
    }
    Some(AwsCredentials {
        access_key_id: access_key_id.to_string(),
        secret_access_key: secret_access_key.to_string(),
        session_token: env::var("AWS_SESSION_TOKEN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    })
}

fn sign_bedrock_sigv4_request(
    url: &str,
    region: &str,
    credentials: &AwsCredentials,
    amz_date: &str,
    body: &str,
) -> Result<SignedBedrockRequest, String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| format!("Bedrock URL 无效：{error}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "Bedrock URL 缺少 host".to_string())
        .map(|host| match parsed.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        })?;
    let canonical_uri = if parsed.path().is_empty() {
        "/"
    } else {
        parsed.path()
    };
    let canonical_query = parsed.query().unwrap_or("");
    let payload_hash = bedrock_sha256_hex(body.as_bytes());
    let date = amz_date
        .get(..8)
        .ok_or_else(|| "x-amz-date 必须包含 YYYYMMDD 前缀".to_string())?;
    let mut headers = vec![
        ("accept", "application/vnd.amazon.eventstream".to_string()),
        ("content-type", "application/json".to_string()),
        ("host", host.clone()),
        ("x-amz-content-sha256", payload_hash.clone()),
        ("x-amz-date", amz_date.to_string()),
    ];
    if let Some(token) = credentials.session_token.as_ref() {
        headers.push(("x-amz-security-token", token.clone()));
    }
    headers.sort_by(|left, right| left.0.cmp(right.0));
    let canonical_headers = headers
        .iter()
        .map(|(name, value)| format!("{name}:{}\n", value.trim()))
        .collect::<String>();
    let signed_headers = headers
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(";");
    let canonical_request = format!(
        "POST\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let credential_scope = format!("{date}/{region}/bedrock/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        bedrock_sha256_hex(canonical_request.as_bytes())
    );
    let signing_key = bedrock_sigv4_signing_key(&credentials.secret_access_key, date, region);
    let signature = bedrock_hmac_sha256_hex(&signing_key, string_to_sign.as_bytes());
    Ok(SignedBedrockRequest {
        host,
        amz_date: amz_date.to_string(),
        payload_hash,
        authorization: format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            credentials.access_key_id
        ),
        security_token: credentials.session_token.clone(),
    })
}

fn bedrock_sigv4_signing_key(secret_access_key: &str, date: &str, region: &str) -> [u8; 32] {
    let date_key = bedrock_hmac_sha256(
        format!("AWS4{secret_access_key}").as_bytes(),
        date.as_bytes(),
    );
    let region_key = bedrock_hmac_sha256(&date_key, region.as_bytes());
    let service_key = bedrock_hmac_sha256(&region_key, b"bedrock");
    bedrock_hmac_sha256(&service_key, b"aws4_request")
}

fn current_amz_date() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3600;
    let minute = (second_of_day % 3600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m, d)
}

pub fn bedrock_sha256_hex(data: &[u8]) -> String {
    hex_lower(&bedrock_sha256(data))
}

pub fn bedrock_hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    hex_lower(&bedrock_hmac_sha256(key, data))
}

fn bedrock_hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut key_block = [0_u8; 64];
    if key.len() > 64 {
        key_block[..32].copy_from_slice(&bedrock_sha256(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut outer = [0x5c_u8; 64];
    let mut inner = [0x36_u8; 64];
    for index in 0..64 {
        outer[index] ^= key_block[index];
        inner[index] ^= key_block[index];
    }
    let mut inner_data = Vec::with_capacity(64 + data.len());
    inner_data.extend_from_slice(&inner);
    inner_data.extend_from_slice(data);
    let inner_hash = bedrock_sha256(&inner_data);
    let mut outer_data = Vec::with_capacity(96);
    outer_data.extend_from_slice(&outer);
    outer_data.extend_from_slice(&inner_hash);
    bedrock_sha256(&outer_data)
}

fn bedrock_sha256(data: &[u8]) -> [u8; 32] {
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut message = data.to_vec();
    let bit_len = (message.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    let mut h = H0;
    for chunk in message.chunks_exact(64) {
        let mut w = [0_u32; 64];
        for index in 0..16 {
            w[index] = u32::from_be_bytes([
                chunk[index * 4],
                chunk[index * 4 + 1],
                chunk[index * 4 + 2],
                chunk[index * 4 + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut output = [0_u8; 32];
    for (index, word) in h.iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    output
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub fn bedrock_converse_stream_payload(
    model_id: &str,
    messages: Vec<BedrockMessage>,
    system: Option<Vec<BedrockSystemContentBlock>>,
    max_tokens: Option<usize>,
    temperature: Option<f64>,
    tool_config: Option<BedrockToolConfiguration>,
    additional_model_request_fields: Option<BedrockAdditionalModelRequestFields>,
    request_metadata: &BTreeMap<String, String>,
) -> Result<Value, String> {
    let mut object = serde_json::Map::new();
    object.insert("modelId".to_string(), json!(model_id));
    object.insert(
        "messages".to_string(),
        serde_json::to_value(messages)
            .map_err(|error| format!("Bedrock messages JSON 无效：{error}"))?,
    );

    if let Some(system) = system.filter(|system| !system.is_empty()) {
        object.insert(
            "system".to_string(),
            serde_json::to_value(system)
                .map_err(|error| format!("Bedrock system JSON 无效：{error}"))?,
        );
    }
    let mut inference_config = serde_json::Map::new();
    if let Some(max_tokens) = max_tokens {
        inference_config.insert("maxTokens".to_string(), json!(max_tokens));
    }
    if let Some(temperature) = temperature {
        inference_config.insert("temperature".to_string(), json!(temperature));
    }
    if !inference_config.is_empty() {
        object.insert(
            "inferenceConfig".to_string(),
            Value::Object(inference_config),
        );
    }
    if let Some(tool_config) = tool_config {
        object.insert(
            "toolConfig".to_string(),
            serde_json::to_value(tool_config)
                .map_err(|error| format!("Bedrock toolConfig JSON 无效：{error}"))?,
        );
    }
    if let Some(additional_model_request_fields) = additional_model_request_fields {
        object.insert(
            "additionalModelRequestFields".to_string(),
            serde_json::to_value(additional_model_request_fields).map_err(|error| {
                format!("Bedrock additionalModelRequestFields JSON 无效：{error}")
            })?,
        );
    }
    if !request_metadata.is_empty() {
        object.insert("requestMetadata".to_string(), json!(request_metadata));
    }

    Ok(Value::Object(object))
}

fn split_bedrock_system_messages(messages: Vec<Message>) -> (Option<String>, Vec<Message>) {
    let mut system = Vec::new();
    let mut rest = Vec::new();
    for message in messages {
        if message.role == MessageRole::System {
            system.push(message.content);
        } else {
            rest.push(message);
        }
    }
    let system_prompt = if system.is_empty() {
        None
    } else {
        Some(system.join("\n\n"))
    };
    (system_prompt, rest)
}

fn empty_bedrock_assistant(model: &Model) -> crate::conversation::RichAssistantMessage {
    crate::conversation::RichAssistantMessage {
        content: Vec::new(),
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        response_model: None,
        response_id: None,
        usage: Usage::default(),
        stop_reason: AssistantStopReason::Stop,
        error_message: None,
        diagnostics: Vec::new(),
        timestamp_millis: 0,
    }
}

fn simple_message_to_bedrock_rich_message(message: Message) -> RichMessage {
    match message.role {
        MessageRole::Assistant => {
            RichMessage::Assistant(crate::conversation::RichAssistantMessage {
                content: vec![AssistantContentBlock::Text(TextContent {
                    text: message.content,
                    text_signature: None,
                })],
                api: "bedrock-converse-stream".to_string(),
                provider: "amazon-bedrock".to_string(),
                model: String::new(),
                response_model: None,
                response_id: None,
                usage: Usage::default(),
                stop_reason: AssistantStopReason::Stop,
                error_message: None,
                diagnostics: Vec::new(),
                timestamp_millis: 0,
            })
        }
        MessageRole::System | MessageRole::User => {
            RichMessage::User(crate::conversation::UserMessage {
                content: UserMessageContent::Text(message.content),
                timestamp_millis: 0,
            })
        }
        MessageRole::Tool => RichMessage::ToolResult(crate::conversation::ToolResultMessage {
            tool_call_id: "tool".to_string(),
            tool_name: "tool".to_string(),
            content: vec![UserContentBlock::Text(TextContent {
                text: message.content,
                text_signature: None,
            })],
            details: None,
            is_error: false,
            timestamp_millis: 0,
        }),
    }
}

pub fn bedrock_model_match_candidates(model_id: &str, model_name: Option<&str>) -> Vec<String> {
    let mut values = vec![model_id.to_string()];
    if let Some(model_name) = model_name {
        values.push(model_name.to_string());
    }
    values
        .into_iter()
        .flat_map(|value| {
            let lower = value.to_ascii_lowercase();
            let normalized = lower
                .chars()
                .map(|character| {
                    if matches!(character, ' ' | '_' | '.' | ':') {
                        '-'
                    } else {
                        character
                    }
                })
                .collect::<String>();
            [lower, normalized]
        })
        .collect()
}

pub fn bedrock_supports_adaptive_thinking(model_id: &str, model_name: Option<&str>) -> bool {
    bedrock_model_match_candidates(model_id, model_name)
        .iter()
        .any(|value| {
            value.contains("opus-4-6") || value.contains("opus-4-7") || value.contains("sonnet-4-6")
        })
}

pub fn bedrock_supports_native_xhigh_effort(model: &Model) -> bool {
    bedrock_model_match_candidates(&model.id, Some(&model.display_name))
        .iter()
        .any(|value| value.contains("opus-4-7"))
}

pub fn map_bedrock_thinking_level_to_effort(
    model: &Model,
    level: Option<ModelThinkingLevel>,
) -> &'static str {
    if level == Some(ModelThinkingLevel::XHigh) && bedrock_supports_native_xhigh_effort(model) {
        return "xhigh";
    }
    if let Some(level) = level {
        if let Some(Some(mapped)) = model.thinking_level_map.get(&level) {
            return match mapped.as_str() {
                "low" => "low",
                "medium" => "medium",
                "high" => "high",
                "xhigh" => "xhigh",
                "max" => "max",
                _ => fallback_bedrock_effort(level),
            };
        }
        return fallback_bedrock_effort(level);
    }
    "high"
}

fn fallback_bedrock_effort(level: ModelThinkingLevel) -> &'static str {
    match level {
        ModelThinkingLevel::Minimal | ModelThinkingLevel::Low => "low",
        ModelThinkingLevel::Medium => "medium",
        ModelThinkingLevel::High | ModelThinkingLevel::XHigh => "high",
        ModelThinkingLevel::Off => "high",
    }
}

pub fn resolve_bedrock_cache_retention(
    cache_retention: Option<BedrockCacheRetention>,
    force_long_from_env: bool,
) -> BedrockCacheRetention {
    cache_retention.unwrap_or_else(|| {
        if force_long_from_env {
            BedrockCacheRetention::Long
        } else {
            BedrockCacheRetention::Short
        }
    })
}

pub fn is_anthropic_claude_bedrock_model(model: &Model) -> bool {
    let id = model.id.to_ascii_lowercase();
    let name = model.display_name.to_ascii_lowercase();
    id.contains("anthropic.claude")
        || id.contains("anthropic/claude")
        || name.contains("anthropic.claude")
        || name.contains("anthropic/claude")
        || name.contains("claude")
}

pub fn bedrock_supports_prompt_caching(model: &Model, force_cache: bool) -> bool {
    let candidates = bedrock_model_match_candidates(&model.id, Some(&model.display_name));
    let has_claude_ref = candidates.iter().any(|value| value.contains("claude"));
    if !has_claude_ref {
        return force_cache;
    }
    candidates.iter().any(|value| {
        value.contains("-4-")
            || value.contains("claude-3-7-sonnet")
            || value.contains("claude-3-5-haiku")
    })
}

pub fn bedrock_supports_thinking_signature(model: &Model) -> bool {
    is_anthropic_claude_bedrock_model(model)
}

pub fn standard_bedrock_endpoint_region(base_url: Option<&str>) -> Option<String> {
    let base_url = base_url?;
    let host = base_url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let suffixes = [".amazonaws.com.cn", ".amazonaws.com"];
    for suffix in suffixes {
        let Some(prefix) = host.strip_suffix(suffix) else {
            continue;
        };
        for runtime_prefix in ["bedrock-runtime-fips.", "bedrock-runtime."] {
            if let Some(region) = prefix.strip_prefix(runtime_prefix) {
                return Some(region.to_string());
            }
        }
    }
    None
}

pub fn bedrock_endpoint_decision(
    base_url: &str,
    configured_region: Option<&str>,
    has_configured_profile: bool,
) -> BedrockEndpointDecision {
    let endpoint_region = standard_bedrock_endpoint_region(Some(base_url));
    let use_explicit_endpoint = endpoint_region
        .as_ref()
        .map(|_| configured_region.is_none() && !has_configured_profile)
        .unwrap_or(true);
    BedrockEndpointDecision {
        endpoint_region,
        use_explicit_endpoint,
    }
}

pub fn is_gov_cloud_bedrock_target(model: &Model, region: Option<&str>) -> bool {
    if region
        .map(|region| region.to_ascii_lowercase().starts_with("us-gov-"))
        .unwrap_or(false)
    {
        return true;
    }
    let model_id = model.id.to_ascii_lowercase();
    model_id.starts_with("us-gov.") || model_id.starts_with("arn:aws-us-gov:")
}

pub fn build_bedrock_additional_model_request_fields(
    model: &Model,
    options: &BedrockOptions,
    is_gov_cloud: bool,
) -> Option<BedrockAdditionalModelRequestFields> {
    let reasoning = options.reasoning?;
    model.reasoning.as_ref()?;

    if !is_anthropic_claude_bedrock_model(model) {
        return None;
    }

    let display = if is_gov_cloud {
        None
    } else {
        Some(
            match options
                .thinking_display
                .unwrap_or(BedrockThinkingDisplay::Summarized)
            {
                BedrockThinkingDisplay::Summarized => "summarized",
                BedrockThinkingDisplay::Omitted => "omitted",
            },
        )
    };
    let mut fields = BTreeMap::new();
    if bedrock_supports_adaptive_thinking(&model.id, Some(&model.display_name)) {
        let mut thinking = BTreeMap::from([("type".to_string(), json!("adaptive"))]);
        if let Some(display) = display {
            thinking.insert("display".to_string(), json!(display));
        }
        fields.insert("thinking".to_string(), json!(thinking));
        fields.insert(
            "output_config".to_string(),
            json!({ "effort": map_bedrock_thinking_level_to_effort(model, Some(reasoning)) }),
        );
    } else {
        let clamped = clamp_reasoning(Some(reasoning)).unwrap_or(ModelThinkingLevel::High);
        let default_budget = match reasoning {
            ModelThinkingLevel::Minimal => 1024,
            ModelThinkingLevel::Low => 2048,
            ModelThinkingLevel::Medium => 8192,
            ModelThinkingLevel::High | ModelThinkingLevel::XHigh => 16384,
            ModelThinkingLevel::Off => 16384,
        };
        let budget =
            bedrock_budget_for_level(options.thinking_budgets, clamped).unwrap_or(default_budget);
        let mut thinking = BTreeMap::from([
            ("type".to_string(), json!("enabled")),
            ("budget_tokens".to_string(), json!(budget)),
        ]);
        if let Some(display) = display {
            thinking.insert("display".to_string(), json!(display));
        }
        fields.insert("thinking".to_string(), json!(thinking));
        if options.interleaved_thinking.unwrap_or(true) {
            fields.insert(
                "anthropic_beta".to_string(),
                json!(["interleaved-thinking-2025-05-14"]),
            );
        }
    }

    Some(BedrockAdditionalModelRequestFields { fields })
}

pub fn adjust_bedrock_max_tokens_for_simple_reasoning(
    model: &Model,
    options: &SimpleStreamOptions,
) -> Option<ThinkingTokenBudget> {
    let reasoning = options
        .metadata
        .get("reasoning")
        .and_then(Value::as_str)
        .and_then(parse_bedrock_thinking_level)?;
    if !is_anthropic_claude_bedrock_model(model)
        || bedrock_supports_adaptive_thinking(&model.id, Some(&model.display_name))
    {
        return None;
    }
    let model_max_tokens = model.max_tokens?;
    Some(adjust_max_tokens_for_thinking(
        options.max_tokens,
        model_max_tokens,
        reasoning,
        None,
    ))
}

fn bedrock_budget_for_level(
    budgets: Option<ThinkingBudgets>,
    level: ModelThinkingLevel,
) -> Option<usize> {
    let budgets = budgets?;
    match level {
        ModelThinkingLevel::Minimal => Some(budgets.minimal),
        ModelThinkingLevel::Low => Some(budgets.low),
        ModelThinkingLevel::Medium => Some(budgets.medium),
        ModelThinkingLevel::High | ModelThinkingLevel::XHigh => Some(budgets.high),
        ModelThinkingLevel::Off => None,
    }
}

fn parse_bedrock_thinking_level(value: &str) -> Option<ModelThinkingLevel> {
    match value {
        "minimal" => Some(ModelThinkingLevel::Minimal),
        "low" => Some(ModelThinkingLevel::Low),
        "medium" => Some(ModelThinkingLevel::Medium),
        "high" => Some(ModelThinkingLevel::High),
        "xhigh" => Some(ModelThinkingLevel::XHigh),
        _ => None,
    }
}

pub fn bedrock_options_from_metadata(
    metadata: &BTreeMap<String, Value>,
    default_max_tokens: Option<usize>,
) -> BedrockOptions {
    BedrockOptions {
        region: metadata
            .get("region")
            .and_then(Value::as_str)
            .map(str::to_string),
        profile: metadata
            .get("profile")
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_choice: metadata
            .get("toolChoice")
            .and_then(parse_bedrock_tool_choice),
        reasoning: metadata
            .get("reasoning")
            .and_then(Value::as_str)
            .and_then(parse_bedrock_thinking_level),
        thinking_budgets: metadata
            .get("thinkingBudgets")
            .and_then(parse_thinking_budgets),
        interleaved_thinking: metadata.get("interleavedThinking").and_then(Value::as_bool),
        thinking_display: metadata
            .get("thinkingDisplay")
            .and_then(Value::as_str)
            .and_then(parse_bedrock_thinking_display),
        request_metadata: metadata
            .get("requestMetadata")
            .and_then(Value::as_object)
            .map(|object| {
                object
                    .iter()
                    .filter_map(|(key, value)| {
                        value.as_str().map(|value| (key.clone(), value.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        bearer_token: metadata
            .get("bearerToken")
            .and_then(Value::as_str)
            .map(str::to_string),
        cache_retention: metadata
            .get("cacheRetention")
            .and_then(Value::as_str)
            .and_then(parse_bedrock_cache_retention),
        max_tokens: metadata
            .get("maxTokens")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .or(default_max_tokens),
        temperature: metadata.get("temperature").and_then(Value::as_f64),
    }
}

fn parse_bedrock_tool_choice(value: &Value) -> Option<BedrockToolChoice> {
    if let Some(choice) = value.as_str() {
        return match choice {
            "auto" => Some(BedrockToolChoice::Auto),
            "any" => Some(BedrockToolChoice::Any),
            "none" => Some(BedrockToolChoice::None),
            _ => None,
        };
    }
    let object = value.as_object()?;
    if object.get("type").and_then(Value::as_str) == Some("tool") {
        return object
            .get("name")
            .and_then(Value::as_str)
            .map(|name| BedrockToolChoice::Tool {
                name: name.to_string(),
            });
    }
    None
}

fn parse_bedrock_cache_retention(value: &str) -> Option<BedrockCacheRetention> {
    match value {
        "none" => Some(BedrockCacheRetention::None),
        "short" => Some(BedrockCacheRetention::Short),
        "long" => Some(BedrockCacheRetention::Long),
        _ => None,
    }
}

fn parse_bedrock_thinking_display(value: &str) -> Option<BedrockThinkingDisplay> {
    match value {
        "summarized" => Some(BedrockThinkingDisplay::Summarized),
        "omitted" => Some(BedrockThinkingDisplay::Omitted),
        _ => None,
    }
}

fn parse_thinking_budgets(value: &Value) -> Option<ThinkingBudgets> {
    let object = value.as_object()?;
    Some(ThinkingBudgets {
        minimal: object.get("minimal")?.as_u64()? as usize,
        low: object.get("low")?.as_u64()? as usize,
        medium: object.get("medium")?.as_u64()? as usize,
        high: object.get("high")?.as_u64()? as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{
        ImageContent, RichAssistantMessage, RichMessage, ThinkingContent, ToolCall,
        ToolResultMessage, UserMessage,
    };
    use crate::providers::bedrock_types::{
        BedrockContentBlockDelta, BedrockContentBlockStart, BedrockConversationRole,
        BedrockImageFormat, BedrockMessage, BedrockProcessedEvent, BedrockReasoningDelta,
        BedrockStreamEvent, BedrockToolChoice, BedrockToolDefinition, BedrockToolResultStatus,
        BedrockToolUseDelta, BedrockToolUseStart, BedrockUsage,
    };
    use crate::types::{LanguageModelProvider, ModelInputKind, ModelReasoning, Usage, UsageCost};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn detects_claude_and_adaptive_thinking_models() {
        let model = model("anthropic.claude-sonnet-4-6", "Claude Sonnet 4.6");
        assert!(is_anthropic_claude_bedrock_model(&model));
        assert!(bedrock_supports_adaptive_thinking(
            &model.id,
            Some(&model.display_name)
        ));
        assert!(bedrock_supports_prompt_caching(&model, false));
        assert!(bedrock_supports_thinking_signature(&model));
    }

    #[test]
    fn maps_thinking_effort_and_native_xhigh() {
        let mut model = model("anthropic.claude-opus-4-7", "Claude Opus 4.7");
        assert_eq!(
            map_bedrock_thinking_level_to_effort(&model, Some(ModelThinkingLevel::XHigh)),
            "xhigh"
        );
        model.id = "anthropic.claude-sonnet-4".to_string();
        model.display_name = "Claude Sonnet 4".to_string();
        assert_eq!(
            map_bedrock_thinking_level_to_effort(&model, Some(ModelThinkingLevel::XHigh)),
            "high"
        );
    }

    #[test]
    fn builds_system_prompt_with_long_cache_point() {
        let result = build_bedrock_system_prompt(
            Some("system"),
            &model("anthropic.claude-sonnet-4-20250514", "Claude Sonnet 4"),
            BedrockCacheRetention::Long,
            false,
        )
        .expect("system");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].text.as_deref(), Some("system"));
        assert_eq!(
            result[1]
                .cache_point
                .as_ref()
                .and_then(|point| point.ttl.as_deref()),
            Some("ONE_HOUR")
        );
    }

    #[test]
    fn converts_tool_config_and_tool_choice() {
        let result = convert_bedrock_tool_config(
            &[BedrockToolDefinition {
                name: "read_file".to_string(),
                description: "读取文件".to_string(),
                parameters: json!({"type":"object"}),
            }],
            Some(&BedrockToolChoice::Tool {
                name: "read_file".to_string(),
            }),
        )
        .expect("tool config");

        assert_eq!(result.tools[0].tool_spec.name, "read_file");
        assert_eq!(
            result.tools[0].tool_spec.input_schema["json"]["type"],
            "object"
        );
        assert_eq!(
            result.tool_choice.expect("choice")["tool"]["name"],
            "read_file"
        );
    }

    #[test]
    fn maps_stop_reason_like_bedrock() {
        assert_eq!(
            map_bedrock_stop_reason(Some("end_turn")),
            AssistantStopReason::Stop
        );
        assert_eq!(
            map_bedrock_stop_reason(Some("model_context_window_exceeded")),
            AssistantStopReason::Length
        );
        assert_eq!(
            map_bedrock_stop_reason(Some("tool_use")),
            AssistantStopReason::ToolUse
        );
        assert_eq!(map_bedrock_stop_reason(None), AssistantStopReason::Error);
    }

    #[test]
    fn detects_standard_endpoint_region_and_explicit_endpoint_decision() {
        let decision = bedrock_endpoint_decision(
            "https://bedrock-runtime.us-east-1.amazonaws.com",
            None,
            false,
        );
        assert_eq!(decision.endpoint_region.as_deref(), Some("us-east-1"));
        assert!(decision.use_explicit_endpoint);

        let decision = bedrock_endpoint_decision(
            "https://bedrock-runtime.us-east-1.amazonaws.com",
            Some("us-west-2"),
            false,
        );
        assert!(!decision.use_explicit_endpoint);

        assert_eq!(
            standard_bedrock_endpoint_region(Some(
                "https://bedrock-runtime-fips.us-gov-west-1.amazonaws.com"
            ))
            .as_deref(),
            Some("us-gov-west-1")
        );
    }

    #[test]
    fn builds_additional_model_request_fields_for_adaptive_thinking() {
        let model = reasoning_model("anthropic.claude-sonnet-4-6", "Claude Sonnet 4.6");
        let options = BedrockOptions {
            reasoning: Some(ModelThinkingLevel::High),
            ..bedrock_options()
        };

        let result =
            build_bedrock_additional_model_request_fields(&model, &options, false).expect("fields");

        assert_eq!(result.fields["thinking"]["type"], "adaptive");
        assert_eq!(result.fields["output_config"]["effort"], "high");
    }

    #[test]
    fn builds_additional_model_request_fields_for_budget_thinking() {
        let model = reasoning_model("anthropic.claude-sonnet-4", "Claude Sonnet 4");
        let options = BedrockOptions {
            reasoning: Some(ModelThinkingLevel::Medium),
            interleaved_thinking: Some(true),
            thinking_budgets: Some(ThinkingBudgets {
                minimal: 1,
                low: 2,
                medium: 333,
                high: 4,
            }),
            ..bedrock_options()
        };

        let result =
            build_bedrock_additional_model_request_fields(&model, &options, false).expect("fields");

        assert_eq!(result.fields["thinking"]["type"], "enabled");
        assert_eq!(result.fields["thinking"]["budget_tokens"], 333);
        assert_eq!(
            result.fields["anthropic_beta"][0],
            "interleaved-thinking-2025-05-14"
        );
    }

    #[test]
    fn builds_bedrock_options_from_stream_request_metadata_like_pi() {
        let mut metadata = BTreeMap::new();
        metadata.insert("temperature".to_string(), json!(0.3));
        metadata.insert("maxTokens".to_string(), json!(2048));
        metadata.insert(
            "toolChoice".to_string(),
            json!({"type":"tool","name":"read_file"}),
        );
        metadata.insert("reasoning".to_string(), json!("medium"));
        metadata.insert("interleavedThinking".to_string(), json!(false));
        metadata.insert("thinkingDisplay".to_string(), json!("omitted"));
        metadata.insert("cacheRetention".to_string(), json!("long"));
        metadata.insert(
            "requestMetadata".to_string(),
            json!({"team":"agents","purpose":"migration"}),
        );

        let options = bedrock_options_from_metadata(&metadata, Some(4096));

        assert_eq!(options.temperature, Some(0.3));
        assert_eq!(options.max_tokens, Some(2048));
        assert_eq!(options.reasoning, Some(ModelThinkingLevel::Medium));
        assert_eq!(options.interleaved_thinking, Some(false));
        assert_eq!(
            options.thinking_display,
            Some(BedrockThinkingDisplay::Omitted)
        );
        assert_eq!(options.cache_retention, Some(BedrockCacheRetention::Long));
        assert_eq!(options.request_metadata["team"], "agents");
        assert!(matches!(
            options.tool_choice,
            Some(BedrockToolChoice::Tool { ref name }) if name == "read_file"
        ));
    }

    #[test]
    fn builds_bedrock_converse_stream_payload_omits_absent_optional_fields_like_pi() {
        let payload = bedrock_converse_stream_payload(
            "anthropic.claude-test",
            vec![BedrockMessage {
                role: BedrockConversationRole::User,
                content: vec![crate::providers::BedrockContentBlock {
                    text: Some("hello".to_string()),
                    image: None,
                    tool_use: None,
                    tool_result: None,
                    reasoning_content: None,
                    cache_point: None,
                }],
            }],
            None,
            None,
            None,
            None,
            None,
            &BTreeMap::new(),
        )
        .expect("payload");

        assert_eq!(payload["modelId"], "anthropic.claude-test");
        assert!(payload.get("system").is_none());
        assert!(payload.get("toolConfig").is_none());
        assert!(payload.get("additionalModelRequestFields").is_none());
        assert!(payload.get("requestMetadata").is_none());
        assert!(payload.get("inferenceConfig").is_none());
    }

    #[test]
    fn omits_display_for_gov_cloud() {
        let model = reasoning_model("us-gov.anthropic.claude-sonnet-4-6", "Claude Sonnet 4.6");
        let options = BedrockOptions {
            reasoning: Some(ModelThinkingLevel::High),
            ..bedrock_options()
        };
        let result =
            build_bedrock_additional_model_request_fields(&model, &options, true).expect("fields");

        assert!(result.fields["thinking"].get("display").is_none());
        assert!(is_gov_cloud_bedrock_target(&model, Some("us-gov-west-1")));
    }

    #[test]
    fn maps_image_formats() {
        assert_eq!(
            bedrock_image_format("image/jpeg"),
            Some(BedrockImageFormat::Jpeg)
        );
        assert_eq!(
            bedrock_image_format("image/png"),
            Some(BedrockImageFormat::Png)
        );
        assert_eq!(bedrock_image_format("text/plain"), None);
    }

    #[test]
    fn converts_user_text_and_images_to_bedrock_blocks() {
        let context = BedrockMessagesContext {
            messages: vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Blocks(vec![
                    UserContentBlock::Text(text("hello")),
                    UserContentBlock::Image(ImageContent {
                        data: "abc".to_string(),
                        mime_type: "image/png".to_string(),
                    }),
                ]),
                timestamp_millis: 1,
            })],
        };

        let result = convert_bedrock_messages(
            &vision_model("anthropic.claude-sonnet-4-20250514", "Claude Sonnet 4"),
            &context,
            BedrockCacheRetention::None,
            false,
        )
        .expect("messages");

        assert_eq!(result[0].role, BedrockConversationRole::User);
        assert_eq!(result[0].content[0].text.as_deref(), Some("hello"));
        assert_eq!(
            result[0].content[1]
                .image
                .as_ref()
                .map(|image| &image.format),
            Some(&BedrockImageFormat::Png)
        );
    }

    #[test]
    fn converts_same_model_claude_thinking_with_signature() {
        let model = model("anthropic.claude-sonnet-4-20250514", "Claude Sonnet 4");
        let context = BedrockMessagesContext {
            messages: vec![RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::Thinking(ThinkingContent {
                    thinking: "reasoning".to_string(),
                    thinking_signature: Some("sig".to_string()),
                    redacted: false,
                })],
                ..assistant_defaults(&model.provider, &model.api, &model.id)
            })],
        };

        let result = convert_bedrock_messages(&model, &context, BedrockCacheRetention::None, false)
            .expect("messages");

        assert_eq!(result[0].role, BedrockConversationRole::Assistant);
        let reasoning = result[0].content[0]
            .reasoning_content
            .as_ref()
            .expect("reasoning");
        assert_eq!(reasoning.reasoning_text.text, "reasoning");
        assert_eq!(reasoning.reasoning_text.signature.as_deref(), Some("sig"));
    }

    #[test]
    fn falls_back_to_text_when_claude_thinking_signature_is_missing() {
        let model = model("anthropic.claude-sonnet-4-20250514", "Claude Sonnet 4");
        let context = BedrockMessagesContext {
            messages: vec![RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::Thinking(ThinkingContent {
                    thinking: "reasoning".to_string(),
                    thinking_signature: None,
                    redacted: false,
                })],
                ..assistant_defaults(&model.provider, &model.api, &model.id)
            })],
        };

        let result = convert_bedrock_messages(&model, &context, BedrockCacheRetention::None, false)
            .expect("messages");

        assert_eq!(result[0].content[0].text.as_deref(), Some("reasoning"));
        assert!(result[0].content[0].reasoning_content.is_none());
    }

    #[test]
    fn non_claude_thinking_omits_signature() {
        let model = model("meta.llama-4", "Llama 4");
        let context = BedrockMessagesContext {
            messages: vec![RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::Thinking(ThinkingContent {
                    thinking: "reasoning".to_string(),
                    thinking_signature: Some("sig".to_string()),
                    redacted: false,
                })],
                ..assistant_defaults(&model.provider, &model.api, &model.id)
            })],
        };

        let result = convert_bedrock_messages(&model, &context, BedrockCacheRetention::None, false)
            .expect("messages");

        let reasoning = result[0].content[0]
            .reasoning_content
            .as_ref()
            .expect("reasoning");
        assert_eq!(reasoning.reasoning_text.signature, None);
    }

    #[test]
    fn merges_consecutive_tool_results_into_one_user_message() {
        let context = BedrockMessagesContext {
            messages: vec![
                tool_result("tool-1", "first", false),
                tool_result("tool-2", "second", true),
            ],
        };

        let result = convert_bedrock_messages(
            &model("anthropic.claude-sonnet-4-20250514", "Claude Sonnet 4"),
            &context,
            BedrockCacheRetention::None,
            false,
        )
        .expect("messages");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, BedrockConversationRole::User);
        assert_eq!(result[0].content.len(), 2);
        assert_eq!(
            result[0].content[1]
                .tool_result
                .as_ref()
                .map(|result| result.status),
            Some(BedrockToolResultStatus::Error)
        );
    }

    #[test]
    fn appends_cache_point_to_last_user_message() {
        let context = BedrockMessagesContext {
            messages: vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Text("hello".to_string()),
                timestamp_millis: 1,
            })],
        };

        let result = convert_bedrock_messages(
            &model("anthropic.claude-sonnet-4-20250514", "Claude Sonnet 4"),
            &context,
            BedrockCacheRetention::Long,
            false,
        )
        .expect("messages");

        assert_eq!(
            result[0]
                .content
                .last()
                .and_then(|block| block.cache_point.as_ref())
                .and_then(|point| point.ttl.as_deref()),
            Some("ONE_HOUR")
        );
    }

    #[test]
    fn normalizes_foreign_tool_call_ids() {
        let model = model("anthropic.claude-sonnet-4-20250514", "Claude Sonnet 4");
        let context = BedrockMessagesContext {
            messages: vec![RichMessage::Assistant(RichAssistantMessage {
                content: vec![AssistantContentBlock::ToolCall(ToolCall {
                    id: "tool.call/id/with/very/long/value/that/exceeds/bedrock/tool/id/limit"
                        .to_string(),
                    name: "read_file".to_string(),
                    arguments: BTreeMap::new(),
                    thought_signature: None,
                })],
                provider: "other".to_string(),
                ..assistant_defaults(&model.provider, &model.api, &model.id)
            })],
        };

        let result = convert_bedrock_messages(&model, &context, BedrockCacheRetention::None, false)
            .expect("messages");

        let tool_use = result[0].content[0].tool_use.as_ref().expect("tool use");
        assert_eq!(tool_use.tool_use_id.len(), 64);
        assert!(!tool_use.tool_use_id.contains('.'));
        assert!(!tool_use.tool_use_id.contains('/'));
    }

    #[test]
    fn rejects_unknown_image_type() {
        let context = BedrockMessagesContext {
            messages: vec![RichMessage::User(UserMessage {
                content: UserMessageContent::Blocks(vec![UserContentBlock::Image(ImageContent {
                    data: "abc".to_string(),
                    mime_type: "image/svg+xml".to_string(),
                })]),
                timestamp_millis: 1,
            })],
        };

        let error = convert_bedrock_messages(
            &vision_model("anthropic.claude-sonnet-4-20250514", "Claude Sonnet 4"),
            &context,
            BedrockCacheRetention::None,
            false,
        )
        .expect_err("unsupported image");

        assert_eq!(error, "Unknown image type: image/svg+xml");
    }

    #[test]
    fn processes_text_stream_events() {
        let assistant = assistant_defaults("amazon-bedrock", "bedrock-converse-stream", "model");

        let result = process_bedrock_stream_events(
            &[
                BedrockStreamEvent::MessageStart {
                    role: BedrockConversationRole::Assistant,
                },
                BedrockStreamEvent::ContentBlockDelta {
                    content_block_index: 0,
                    delta: BedrockContentBlockDelta {
                        text: Some("hel".to_string()),
                        tool_use: None,
                        reasoning_content: None,
                    },
                },
                BedrockStreamEvent::ContentBlockDelta {
                    content_block_index: 0,
                    delta: BedrockContentBlockDelta {
                        text: Some("lo".to_string()),
                        tool_use: None,
                        reasoning_content: None,
                    },
                },
                BedrockStreamEvent::ContentBlockStop {
                    content_block_index: 0,
                },
                BedrockStreamEvent::MessageStop {
                    stop_reason: Some("end_turn".to_string()),
                },
            ],
            assistant,
            None::<fn(&mut Usage)>,
        )
        .expect("stream");

        assert!(matches!(
            result.events[1],
            BedrockProcessedEvent::TextStart { content_index: 0 }
        ));
        assert!(matches!(
            result.events.last(),
            Some(BedrockProcessedEvent::Completed {
                stop_reason: AssistantStopReason::Stop
            })
        ));
        assert_eq!(
            result.assistant.content[0],
            AssistantContentBlock::Text(TextContent {
                text: "hello".to_string(),
                text_signature: None,
            })
        );
    }

    #[test]
    fn processes_tool_call_stream_events() {
        let assistant = assistant_defaults("amazon-bedrock", "bedrock-converse-stream", "model");

        let result = process_bedrock_stream_events(
            &[
                BedrockStreamEvent::ContentBlockStart {
                    content_block_index: 2,
                    start: BedrockContentBlockStart {
                        tool_use: Some(BedrockToolUseStart {
                            tool_use_id: Some("tool-1".to_string()),
                            name: Some("read_file".to_string()),
                        }),
                    },
                },
                BedrockStreamEvent::ContentBlockDelta {
                    content_block_index: 2,
                    delta: BedrockContentBlockDelta {
                        text: None,
                        tool_use: Some(BedrockToolUseDelta {
                            input: Some("{\"path\":\"/tmp".to_string()),
                        }),
                        reasoning_content: None,
                    },
                },
                BedrockStreamEvent::ContentBlockDelta {
                    content_block_index: 2,
                    delta: BedrockContentBlockDelta {
                        text: None,
                        tool_use: Some(BedrockToolUseDelta {
                            input: Some("/a\"}".to_string()),
                        }),
                        reasoning_content: None,
                    },
                },
                BedrockStreamEvent::ContentBlockStop {
                    content_block_index: 2,
                },
            ],
            assistant,
            None::<fn(&mut Usage)>,
        )
        .expect("stream");

        let AssistantContentBlock::ToolCall(tool_call) = &result.assistant.content[0] else {
            panic!("expected tool call");
        };
        assert_eq!(tool_call.id, "tool-1");
        assert_eq!(tool_call.name, "read_file");
        assert_eq!(tool_call.arguments["path"], "/tmp/a");
        assert!(result.events.iter().any(|event| matches!(
            event,
            BedrockProcessedEvent::ToolCallEnd { tool_call, .. } if tool_call.id == "tool-1"
        )));
    }

    #[test]
    fn processes_reasoning_signature_and_usage() {
        let assistant = assistant_defaults("amazon-bedrock", "bedrock-converse-stream", "model");

        let result = process_bedrock_stream_events(
            &[
                BedrockStreamEvent::ContentBlockDelta {
                    content_block_index: 1,
                    delta: BedrockContentBlockDelta {
                        text: None,
                        tool_use: None,
                        reasoning_content: Some(BedrockReasoningDelta {
                            text: Some("think".to_string()),
                            signature: Some("sig".to_string()),
                        }),
                    },
                },
                BedrockStreamEvent::ContentBlockStop {
                    content_block_index: 1,
                },
                BedrockStreamEvent::Metadata {
                    usage: Some(BedrockUsage {
                        input_tokens: Some(10),
                        output_tokens: Some(5),
                        cache_read_input_tokens: Some(3),
                        cache_write_input_tokens: Some(2),
                        total_tokens: None,
                    }),
                },
            ],
            assistant,
            Some(|usage: &mut Usage| {
                usage.cost.total = 1.5;
            }),
        )
        .expect("stream");

        let AssistantContentBlock::Thinking(thinking) = &result.assistant.content[0] else {
            panic!("expected thinking");
        };
        assert_eq!(thinking.thinking, "think");
        assert_eq!(thinking.thinking_signature.as_deref(), Some("sig"));
        assert_eq!(result.assistant.usage.input, 10);
        assert_eq!(result.assistant.usage.total_tokens, 15);
        assert_eq!(result.assistant.usage.cost.total, 1.5);
    }

    #[test]
    fn converts_bedrock_processed_events_to_public_stream_events_like_pi() {
        let assistant = assistant_defaults("amazon-bedrock", "bedrock-converse-stream", "model");
        let result = process_bedrock_stream_events(
            &[
                BedrockStreamEvent::ContentBlockDelta {
                    content_block_index: 0,
                    delta: BedrockContentBlockDelta {
                        text: Some("hi".to_string()),
                        tool_use: None,
                        reasoning_content: None,
                    },
                },
                BedrockStreamEvent::ContentBlockStop {
                    content_block_index: 0,
                },
                BedrockStreamEvent::Metadata {
                    usage: Some(BedrockUsage {
                        input_tokens: Some(1),
                        output_tokens: Some(2),
                        cache_read_input_tokens: None,
                        cache_write_input_tokens: None,
                        total_tokens: Some(3),
                    }),
                },
                BedrockStreamEvent::MessageStop {
                    stop_reason: Some("end_turn".to_string()),
                },
            ],
            assistant,
            None::<fn(&mut Usage)>,
        )
        .expect("process");

        let events = crate::providers::bedrock_stream_events_from_process_result(result);

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "hi"
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::Usage { usage } if usage.input == 1 && usage.output == 2
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::Finished { message }) if message.content == "hi"
        ));
    }

    #[test]
    fn parses_bedrock_aws_event_stream_body_like_converse_stream() {
        let body = [
            aws_event_stream_message("messageStart", br#"{"role":"assistant"}"#),
            aws_event_stream_message(
                "contentBlockDelta",
                br#"{"contentBlockIndex":0,"delta":{"text":"hi"}}"#,
            ),
            aws_event_stream_message("contentBlockStop", br#"{"contentBlockIndex":0}"#),
            aws_event_stream_message("messageStop", br#"{"stopReason":"end_turn"}"#),
        ]
        .concat();

        let events = crate::providers::parse_bedrock_converse_event_stream_body(&body)
            .expect("event stream");

        assert!(matches!(
            events[0],
            BedrockStreamEvent::MessageStart {
                role: BedrockConversationRole::Assistant
            }
        ));
        assert!(matches!(
            &events[1],
            BedrockStreamEvent::ContentBlockDelta {
                content_block_index: 0,
                delta,
            } if delta.text.as_deref() == Some("hi")
        ));
        assert!(matches!(
            events[3],
            BedrockStreamEvent::MessageStop {
                stop_reason: Some(ref reason)
            } if reason == "end_turn"
        ));
    }

    #[test]
    fn bedrock_bearer_runtime_posts_converse_stream_and_parses_eventstream_like_pi() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("read");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("headers")
                + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .or_else(|| line.strip_prefix("Content-Length: "))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).expect("read body");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            assert!(request_text
                .starts_with("POST /model/anthropic.claude-test/converse-stream HTTP/1.1"));
            assert!(request_text.contains("authorization: Bearer bedrock-token"));
            assert!(request_text.contains("\"modelId\":\"anthropic.claude-test\""));
            assert!(request_text.contains("\"messages\""));

            let body = [
                aws_event_stream_message("messageStart", br#"{"role":"assistant"}"#),
                aws_event_stream_message(
                    "contentBlockDelta",
                    br#"{"contentBlockIndex":0,"delta":{"text":"hi"}}"#,
                ),
                aws_event_stream_message("contentBlockStop", br#"{"contentBlockIndex":0}"#),
                aws_event_stream_message(
                    "metadata",
                    br#"{"usage":{"inputTokens":4,"outputTokens":2,"totalTokens":6}}"#,
                ),
                aws_event_stream_message("messageStop", br#"{"stopReason":"end_turn"}"#),
            ]
            .concat();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/vnd.amazon.eventstream\r\ncontent-length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).expect("write head");
            stream.write_all(&body).expect("write body");
        });

        let provider = BedrockConverseProvider::new(BedrockConverseConfig {
            region: Some("us-east-1".to_string()),
            profile: None,
            bearer_token: Some("bedrock-token".to_string()),
            base_url: format!("http://{address}"),
        });
        let events = provider
            .stream(StreamRequest {
                model: model("anthropic.claude-test", "Claude Test"),
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "hello".to_string(),
                }],
                rich_messages: Vec::new(),
                tools: Vec::new(),
                metadata: Default::default(),
            })
            .expect("stream");
        handle.join().expect("server");

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "hi"
        ));
        assert!(matches!(
            &events[1],
            StreamEvent::Usage { usage } if usage.input == 4 && usage.output == 2
        ));
        assert!(matches!(
            events.last(),
            Some(StreamEvent::Finished { message }) if message.content == "hi"
        ));
    }

    #[test]
    fn bedrock_runtime_prefers_stream_request_rich_messages_like_pi() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("read");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("headers")
                + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .or_else(|| line.strip_prefix("Content-Length: "))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).expect("read body");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            assert!(request_text.contains("\"reasoningContent\""));
            assert!(request_text.contains("\"signature\":\"sig\""));
            assert!(!request_text.contains("fallback simple"));

            let body = [
                aws_event_stream_message("messageStart", br#"{"role":"assistant"}"#),
                aws_event_stream_message(
                    "contentBlockDelta",
                    br#"{"contentBlockIndex":0,"delta":{"text":"hi"}}"#,
                ),
                aws_event_stream_message("contentBlockStop", br#"{"contentBlockIndex":0}"#),
                aws_event_stream_message("messageStop", br#"{"stopReason":"end_turn"}"#),
            ]
            .concat();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/vnd.amazon.eventstream\r\ncontent-length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).expect("write head");
            stream.write_all(&body).expect("write body");
        });

        let request_model = model("anthropic.claude-test", "Claude Test");
        let provider = BedrockConverseProvider::new(BedrockConverseConfig {
            region: Some("us-east-1".to_string()),
            profile: None,
            bearer_token: Some("bedrock-token".to_string()),
            base_url: format!("http://{address}"),
        });
        provider
            .stream(StreamRequest {
                model: request_model.clone(),
                messages: vec![Message {
                    role: MessageRole::User,
                    content: "fallback simple".to_string(),
                }],
                rich_messages: vec![RichMessage::Assistant(RichAssistantMessage {
                    content: vec![AssistantContentBlock::Thinking(ThinkingContent {
                        thinking: "reasoning".to_string(),
                        thinking_signature: Some("sig".to_string()),
                        redacted: false,
                    })],
                    ..assistant_defaults(
                        &request_model.provider,
                        &request_model.api,
                        &request_model.id,
                    )
                })],
                tools: Vec::new(),
                metadata: Default::default(),
            })
            .expect("stream");
        handle.join().expect("server");
    }

    #[test]
    fn bedrock_sigv4_crypto_matches_sha256_and_hmac_test_vectors() {
        assert_eq!(
            bedrock_sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            bedrock_hmac_sha256_hex(&[0x0b; 20], b"Hi There"),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn bedrock_sigv4_runtime_posts_signed_converse_stream_like_pi() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("read");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .expect("headers")
                + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .or_else(|| line.strip_prefix("Content-Length: "))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).expect("read body");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let request_text = String::from_utf8_lossy(&request).to_string();
            assert!(request_text
                .starts_with("POST /model/anthropic.claude-test/converse-stream HTTP/1.1"));
            assert!(request_text.contains("authorization: AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20240607/us-east-1/bedrock/aws4_request"));
            assert!(request_text.contains("SignedHeaders=accept;content-type;host;x-amz-content-sha256;x-amz-date;x-amz-security-token"));
            assert!(request_text.contains("x-amz-date: 20240607T120000Z"));
            assert!(request_text.contains("x-amz-security-token: session-token"));
            assert!(request_text.contains("\"modelId\":\"anthropic.claude-test\""));

            let body = [
                aws_event_stream_message("messageStart", br#"{"role":"assistant"}"#),
                aws_event_stream_message(
                    "contentBlockDelta",
                    br#"{"contentBlockIndex":0,"delta":{"text":"hi"}}"#,
                ),
                aws_event_stream_message("contentBlockStop", br#"{"contentBlockIndex":0}"#),
                aws_event_stream_message("messageStop", br#"{"stopReason":"end_turn"}"#),
            ]
            .concat();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/vnd.amazon.eventstream\r\ncontent-length: {}\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).expect("write head");
            stream.write_all(&body).expect("write body");
        });

        let provider = BedrockConverseProvider::new(BedrockConverseConfig {
            region: Some("us-east-1".to_string()),
            profile: None,
            bearer_token: None,
            base_url: format!("http://{address}"),
        });
        let events = provider
            .stream_with_aws_credentials_for_test(
                StreamRequest {
                    model: model("anthropic.claude-test", "Claude Test"),
                    messages: vec![Message {
                        role: MessageRole::User,
                        content: "hello".to_string(),
                    }],
                    rich_messages: Vec::new(),
                    tools: Vec::new(),
                    metadata: Default::default(),
                },
                AwsCredentials {
                    access_key_id: "AKIDEXAMPLE".to_string(),
                    secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
                    session_token: Some("session-token".to_string()),
                },
                "20240607T120000Z",
            )
            .expect("stream");
        handle.join().expect("server");

        assert!(matches!(
            &events[0],
            StreamEvent::TextDelta { text } if text == "hi"
        ));
    }

    #[test]
    fn formats_bedrock_stream_errors() {
        let assistant = assistant_defaults("amazon-bedrock", "bedrock-converse-stream", "model");

        let error = process_bedrock_stream_events(
            &[BedrockStreamEvent::Error {
                name: Some("ValidationException".to_string()),
                message: "bad payload".to_string(),
            }],
            assistant,
            None::<fn(&mut Usage)>,
        )
        .expect_err("error");

        assert_eq!(error, "Validation error: bad payload");
    }

    fn text(value: &str) -> TextContent {
        TextContent {
            text: value.to_string(),
            text_signature: None,
        }
    }

    fn assistant_defaults(provider: &str, api: &str, model: &str) -> RichAssistantMessage {
        RichAssistantMessage {
            content: Vec::new(),
            api: api.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            response_model: None,
            response_id: None,
            usage: Usage {
                input: 0,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                total_tokens: 0,
                cost: UsageCost::default(),
            },
            stop_reason: AssistantStopReason::Stop,
            error_message: None,
            diagnostics: Vec::new(),
            timestamp_millis: 1,
        }
    }

    fn aws_event_stream_message(event_type: &str, payload: &[u8]) -> Vec<u8> {
        let mut headers = Vec::new();
        headers.push(11);
        headers.extend_from_slice(b":event-type");
        headers.push(7);
        headers.extend_from_slice(&(event_type.len() as u16).to_be_bytes());
        headers.extend_from_slice(event_type.as_bytes());

        let total_len = 12 + headers.len() + payload.len() + 4;
        let mut message = Vec::new();
        message.extend_from_slice(&(total_len as u32).to_be_bytes());
        message.extend_from_slice(&(headers.len() as u32).to_be_bytes());
        message.extend_from_slice(&0_u32.to_be_bytes());
        message.extend_from_slice(&headers);
        message.extend_from_slice(payload);
        message.extend_from_slice(&0_u32.to_be_bytes());
        message
    }

    fn tool_result(id: &str, output: &str, is_error: bool) -> RichMessage {
        RichMessage::ToolResult(ToolResultMessage {
            tool_call_id: id.to_string(),
            tool_name: "read_file".to_string(),
            content: vec![UserContentBlock::Text(text(output))],
            details: None,
            is_error,
            timestamp_millis: 1,
        })
    }

    fn model(id: &str, display_name: &str) -> Model {
        Model {
            id: id.to_string(),
            provider: "amazon-bedrock".to_string(),
            api: "bedrock-converse-stream".to_string(),
            display_name: display_name.to_string(),
            context_window: 200_000,
            max_tokens: Some(8192),
            ..Model::default()
        }
    }

    fn vision_model(id: &str, display_name: &str) -> Model {
        Model {
            input: vec![ModelInputKind::Text, ModelInputKind::Image],
            ..model(id, display_name)
        }
    }

    fn reasoning_model(id: &str, display_name: &str) -> Model {
        Model {
            reasoning: Some(ModelReasoning { enabled: true }),
            ..model(id, display_name)
        }
    }

    fn bedrock_options() -> BedrockOptions {
        BedrockOptions {
            region: None,
            profile: None,
            tool_choice: None,
            reasoning: None,
            thinking_budgets: None,
            interleaved_thinking: None,
            thinking_display: None,
            request_metadata: BTreeMap::new(),
            bearer_token: None,
            cache_retention: None,
            max_tokens: None,
            temperature: None,
        }
    }
}
