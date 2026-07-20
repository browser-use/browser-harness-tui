use crate::error::ApiError;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;
use codex_protocol::config_types::Verbosity as VerbosityConfig;
use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::ImageDetail;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::protocol::ModelVerification;
use codex_protocol::protocol::RateLimitSnapshot;
use codex_protocol::protocol::TokenUsage;
use codex_protocol::protocol::TurnModerationMetadataEvent;
use codex_protocol::protocol::W3cTraceContext;
use futures::Stream;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use tokio::sync::mpsc;

pub const WS_REQUEST_HEADER_TRACEPARENT_CLIENT_METADATA_KEY: &str = "ws_request_header_traceparent";
pub const WS_REQUEST_HEADER_TRACESTATE_CLIENT_METADATA_KEY: &str = "ws_request_header_tracestate";

/// Canonical input payload for the compaction endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct CompactionInput<'a> {
    pub model: &'a str,
    pub input: &'a [ResponseItem],
    #[serde(skip_serializing_if = "str::is_empty")]
    pub instructions: &'a str,
    pub tools: Vec<Value>,
    pub parallel_tool_calls: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextControls>,
}

/// Canonical input payload for the memory summarize endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct MemorySummarizeInput {
    pub model: String,
    #[serde(rename = "traces")]
    pub raw_memories: Vec<RawMemory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawMemory {
    pub id: String,
    pub metadata: RawMemoryMetadata,
    pub items: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawMemoryMetadata {
    pub source_path: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MemorySummarizeOutput {
    #[serde(rename = "trace_summary", alias = "raw_memory")]
    pub raw_memory: String,
    pub memory_summary: String,
}

#[derive(Debug)]
pub enum ResponseEvent {
    Created,
    OutputItemDone(ResponseItem),
    OutputItemAdded(ResponseItem),
    /// Emitted when the server includes `OpenAI-Model` on the stream response.
    /// This can differ from the requested model when backend safety routing applies.
    ServerModel(String),
    /// Emitted when the server recommends additional account verification.
    ModelVerifications(Vec<ModelVerification>),
    /// Emitted when the server includes moderation metadata for first-party turn presentation.
    TurnModerationMetadata(TurnModerationMetadataEvent),
    /// Emitted when `X-Reasoning-Included: true` is present on the response,
    /// meaning the server already accounted for past reasoning tokens and the
    /// client should not re-estimate them.
    ServerReasoningIncluded(bool),
    Completed {
        response_id: String,
        token_usage: Option<TokenUsage>,
        /// Did the model affirmatively end its turn? Some providers do not set this,
        /// so we rely on fallback logic when this is `None`.
        end_turn: Option<bool>,
    },
    OutputTextDelta(String),
    ToolCallInputDelta {
        item_id: String,
        call_id: Option<String>,
        delta: String,
    },
    ReasoningSummaryDelta {
        delta: String,
        summary_index: i64,
    },
    ReasoningContentDelta {
        delta: String,
        content_index: i64,
    },
    ReasoningSummaryPartAdded {
        summary_index: i64,
    },
    RateLimits(RateLimitSnapshot),
    ModelsEtag(String),
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningContext {
    Auto,
    CurrentTurn,
    AllTurns,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct Reasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffortConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ReasoningSummaryConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ReasoningContext>,
}

#[derive(Debug, Serialize, Default, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TextFormatType {
    #[default]
    JsonSchema,
}

#[derive(Debug, Serialize, Default, Clone, PartialEq)]
pub struct TextFormat {
    /// Format type used by the OpenAI text controls.
    pub r#type: TextFormatType,
    /// When true, the server is expected to strictly validate responses.
    pub strict: bool,
    /// JSON schema for the desired output.
    pub schema: Value,
    /// Friendly name for the format, used in telemetry/debugging.
    pub name: String,
}

/// Controls the `text` field for the Responses API, combining verbosity and
/// optional JSON schema output formatting.
#[derive(Debug, Serialize, Default, Clone, PartialEq)]
pub struct TextControls {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<OpenAiVerbosity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<TextFormat>,
}

#[derive(Debug, Serialize, Default, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OpenAiVerbosity {
    Low,
    #[default]
    Medium,
    High,
}

impl From<VerbosityConfig> for OpenAiVerbosity {
    fn from(v: VerbosityConfig) -> Self {
        match v {
            VerbosityConfig::Low => OpenAiVerbosity::Low,
            VerbosityConfig::Medium => OpenAiVerbosity::Medium,
            VerbosityConfig::High => OpenAiVerbosity::High,
        }
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ResponsesApiRequest {
    pub model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub instructions: String,
    pub input: Vec<ResponseItem>,
    pub tools: Vec<serde_json::Value>,
    pub tool_choice: String,
    pub parallel_tool_calls: bool,
    pub reasoning: Option<Reasoning>,
    pub store: bool,
    pub stream: bool,
    pub include: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextControls>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_metadata: Option<HashMap<String, String>>,
}

/// Canonical request body for the OpenAI Chat Completions API
/// (`POST /v1/chat/completions`). This is the OpenAI-compatible shape spoken by
/// LiteLLM, OpenRouter, Ollama, vLLM, and most third-party gateways.
///
/// Unlike [`ResponsesApiRequest`], `messages`/`tools` are pre-serialized into
/// generic JSON because the Chat wire format differs from the Responses item
/// shape produced by `ResponseItem`'s serde.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Value>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tool_choice: String,
    pub stream: bool,
    pub stream_options: ChatStreamOptions,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct ChatStreamOptions {
    /// Request a trailing usage chunk so token accounting is available even when
    /// streaming.
    pub include_usage: bool,
}

/// Maps internal [`ResponseItem`]s (the Responses-shaped conversation history)
/// plus `base_instructions` into a list of Chat Completions `messages`.
///
/// Items with no Chat representation (reasoning, compaction, web/image/tool
/// search, etc.) are skipped. Images map to `image_url` content parts. The
/// Chat wire only accepts images on `user` messages, so images returned by
/// tools (e.g. screenshots) are re-emitted as a `user` message after the
/// contiguous run of `tool` replies they belong to — inserting a `user`
/// message between the tool replies of one parallel-call batch would fail
/// strict request validation.
pub fn build_chat_messages(input: &[ResponseItem], base_instructions: &str) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();
    let mut pending_tool_images: Vec<Value> = Vec::new();

    if !base_instructions.is_empty() {
        messages.push(serde_json::json!({
            "role": "system",
            "content": base_instructions,
        }));
    }

    for item in input {
        if !matches!(item, ResponseItem::FunctionCallOutput { .. }) {
            messages.append(&mut pending_tool_images);
        }
        match item {
            ResponseItem::Message { role, content, .. } => {
                messages.push(chat_message_from_content(role, content));
            }
            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": Value::Null,
                    "tool_calls": [{
                        "id": call_id,
                        "type": "function",
                        "function": {
                            "name": name,
                            // `arguments` is already a JSON string; pass through.
                            "arguments": arguments,
                        },
                    }],
                }));
            }
            ResponseItem::FunctionCallOutput { call_id, output } => {
                let mut text = output.body.to_text().unwrap_or_default();
                let image_parts = tool_output_image_parts(&output.body);
                if !image_parts.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str("[image output attached in the next user message]");
                    let mut parts = vec![serde_json::json!({
                        "type": "text",
                        "text": format!("Image output from tool call {call_id}:"),
                    })];
                    parts.extend(image_parts);
                    pending_tool_images.push(serde_json::json!({
                        "role": "user",
                        "content": parts,
                    }));
                }
                messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": text,
                }));
            }
            // Reasoning / Compaction / tool-search / web-search / image-gen and
            // other exotic items have no Chat Completions representation.
            _ => {}
        }
    }
    messages.append(&mut pending_tool_images);

    messages
}

fn chat_message_from_content(role: &str, content: &[ContentItem]) -> Value {
    let mut plain_text = String::new();
    let mut parts: Vec<Value> = Vec::new();
    let mut has_image = false;
    for item in content {
        match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                plain_text.push_str(text);
                parts.push(serde_json::json!({ "type": "text", "text": text }));
            }
            // The Chat wire only accepts image parts on `user` messages;
            // images on other roles have no representation and are dropped.
            ContentItem::InputImage { image_url, detail } if role == "user" => {
                has_image = true;
                parts.push(chat_image_part(image_url, *detail));
            }
            ContentItem::InputImage { .. } => {}
        }
    }
    if has_image {
        serde_json::json!({ "role": role, "content": parts })
    } else {
        serde_json::json!({ "role": role, "content": plain_text })
    }
}

fn tool_output_image_parts(body: &FunctionCallOutputBody) -> Vec<Value> {
    match body {
        FunctionCallOutputBody::Text(_) => Vec::new(),
        FunctionCallOutputBody::ContentItems(items) => items
            .iter()
            .filter_map(|item| match item {
                FunctionCallOutputContentItem::InputImage { image_url, detail } => {
                    Some(chat_image_part(image_url, *detail))
                }
                FunctionCallOutputContentItem::InputText { .. }
                | FunctionCallOutputContentItem::EncryptedContent { .. } => None,
            })
            .collect(),
    }
}

fn chat_image_part(image_url: &str, detail: Option<ImageDetail>) -> Value {
    let detail = match detail {
        Some(ImageDetail::Auto) => Some("auto"),
        Some(ImageDetail::Low) => Some("low"),
        Some(ImageDetail::High) => Some("high"),
        // `original` is Responses-only; let the provider apply its default.
        Some(ImageDetail::Original) | None => None,
    };
    match detail {
        Some(detail) => serde_json::json!({
            "type": "image_url",
            "image_url": { "url": image_url, "detail": detail },
        }),
        None => serde_json::json!({
            "type": "image_url",
            "image_url": { "url": image_url },
        }),
    }
}

impl From<&ResponsesApiRequest> for ResponseCreateWsRequest {
    fn from(request: &ResponsesApiRequest) -> Self {
        Self {
            model: request.model.clone(),
            instructions: request.instructions.clone(),
            previous_response_id: None,
            input: request.input.clone(),
            tools: request.tools.clone(),
            tool_choice: request.tool_choice.clone(),
            parallel_tool_calls: request.parallel_tool_calls,
            reasoning: request.reasoning.clone(),
            store: request.store,
            stream: request.stream,
            include: request.include.clone(),
            service_tier: request.service_tier.clone(),
            prompt_cache_key: request.prompt_cache_key.clone(),
            text: request.text.clone(),
            generate: None,
            client_metadata: request.client_metadata.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ResponseCreateWsRequest {
    pub model: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    pub input: Vec<ResponseItem>,
    pub tools: Vec<Value>,
    pub tool_choice: String,
    pub parallel_tool_calls: bool,
    pub reasoning: Option<Reasoning>,
    pub store: bool,
    pub stream: bool,
    pub include: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextControls>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_metadata: Option<HashMap<String, String>>,
}

pub fn response_create_client_metadata(
    client_metadata: Option<HashMap<String, String>>,
    trace: Option<&W3cTraceContext>,
) -> Option<HashMap<String, String>> {
    let mut client_metadata = client_metadata.unwrap_or_default();

    if let Some(traceparent) = trace.and_then(|trace| trace.traceparent.as_deref()) {
        client_metadata.insert(
            WS_REQUEST_HEADER_TRACEPARENT_CLIENT_METADATA_KEY.to_string(),
            traceparent.to_string(),
        );
    }
    if let Some(tracestate) = trace.and_then(|trace| trace.tracestate.as_deref()) {
        client_metadata.insert(
            WS_REQUEST_HEADER_TRACESTATE_CLIENT_METADATA_KEY.to_string(),
            tracestate.to_string(),
        );
    }

    (!client_metadata.is_empty()).then_some(client_metadata)
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[allow(clippy::large_enum_variant)]
pub enum ResponsesWsRequest {
    #[serde(rename = "response.create")]
    ResponseCreate(ResponseCreateWsRequest),
}

pub fn create_text_param_for_request(
    verbosity: Option<VerbosityConfig>,
    output_schema: &Option<Value>,
    output_schema_strict: bool,
) -> Option<TextControls> {
    if verbosity.is_none() && output_schema.is_none() {
        return None;
    }

    Some(TextControls {
        verbosity: verbosity.map(std::convert::Into::into),
        format: output_schema.as_ref().map(|schema| TextFormat {
            r#type: TextFormatType::JsonSchema,
            strict: output_schema_strict,
            schema: schema.clone(),
            name: "codex_output_schema".to_string(),
        }),
    })
}

pub struct ResponseStream {
    pub rx_event: mpsc::Receiver<Result<ResponseEvent, ApiError>>,
    /// Server-assigned `x-request-id` response header, when present.
    pub upstream_request_id: Option<String>,
}

impl Stream for ResponseStream {
    type Item = Result<ResponseEvent, ApiError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx_event.poll_recv(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::models::FunctionCallOutputPayload;
    use serde_json::json;

    fn user_message(content: Vec<ContentItem>) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content,
            phase: None,
        }
    }

    fn tool_output(call_id: &str, items: Vec<FunctionCallOutputContentItem>) -> ResponseItem {
        ResponseItem::FunctionCallOutput {
            call_id: call_id.to_string(),
            output: FunctionCallOutputPayload::from_content_items(items),
        }
    }

    #[test]
    fn text_only_message_stays_plain_string() {
        let input = [user_message(vec![ContentItem::InputText {
            text: "hello".to_string(),
        }])];
        let messages = build_chat_messages(&input, "");
        assert_eq!(messages, vec![json!({"role": "user", "content": "hello"})]);
    }

    #[test]
    fn user_message_image_becomes_image_url_part() {
        let input = [user_message(vec![
            ContentItem::InputText {
                text: "look at this".to_string(),
            },
            ContentItem::InputImage {
                image_url: "data:image/png;base64,AAA".to_string(),
                detail: Some(ImageDetail::High),
            },
        ])];
        let messages = build_chat_messages(&input, "");
        assert_eq!(
            messages,
            vec![json!({
                "role": "user",
                "content": [
                    {"type": "text", "text": "look at this"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAA", "detail": "high"}},
                ],
            })]
        );
    }

    #[test]
    fn original_detail_is_omitted_from_image_part() {
        let input = [user_message(vec![ContentItem::InputImage {
            image_url: "data:image/png;base64,AAA".to_string(),
            detail: Some(ImageDetail::Original),
        }])];
        let messages = build_chat_messages(&input, "");
        assert_eq!(
            messages,
            vec![json!({
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAA"}},
                ],
            })]
        );
    }

    #[test]
    fn tool_output_image_reemitted_as_user_message() {
        let input = [tool_output(
            "call_1",
            vec![
                FunctionCallOutputContentItem::InputText {
                    text: "screenshot captured".to_string(),
                },
                FunctionCallOutputContentItem::InputImage {
                    image_url: "data:image/png;base64,BBB".to_string(),
                    detail: None,
                },
            ],
        )];
        let messages = build_chat_messages(&input, "");
        assert_eq!(
            messages,
            vec![
                json!({
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "screenshot captured\n[image output attached in the next user message]",
                }),
                json!({
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Image output from tool call call_1:"},
                        {"type": "image_url", "image_url": {"url": "data:image/png;base64,BBB"}},
                    ],
                }),
            ]
        );
    }

    #[test]
    fn parallel_tool_outputs_stay_contiguous_before_image_messages() {
        let input = [
            tool_output(
                "call_1",
                vec![FunctionCallOutputContentItem::InputImage {
                    image_url: "data:image/png;base64,AAA".to_string(),
                    detail: None,
                }],
            ),
            tool_output(
                "call_2",
                vec![FunctionCallOutputContentItem::InputImage {
                    image_url: "data:image/png;base64,BBB".to_string(),
                    detail: None,
                }],
            ),
        ];
        let messages = build_chat_messages(&input, "");
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| m.get("role").and_then(Value::as_str).unwrap())
            .collect();
        assert_eq!(roles, vec!["tool", "tool", "user", "user"]);
    }
}
