//! Streaming parser for the OpenAI Chat Completions API.
//!
//! Consumes Server-Sent Events where each `data:` line is a
//! `chat.completion.chunk` JSON object (terminated by `data: [DONE]`) and emits
//! the SAME internal [`ResponseEvent`] types produced by the Responses parser,
//! so all downstream machinery (event mapping, tool dispatch, history) is
//! reused unchanged.

use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::error::ApiError;
use crate::rate_limits::parse_all_rate_limits;
use crate::telemetry::SseTelemetry;
use codex_client::ByteStream;
use codex_client::StreamResponse;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;
use tracing::debug;
use tracing::trace;

const REQUEST_ID_HEADER: &str = "x-request-id";

pub fn spawn_chat_stream(
    stream_response: StreamResponse,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
) -> ResponseStream {
    let rate_limit_snapshots = parse_all_rate_limits(&stream_response.headers);
    let upstream_request_id = stream_response
        .headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent, ApiError>>(1600);
    tokio::spawn(async move {
        for snapshot in rate_limit_snapshots {
            let _ = tx_event.send(Ok(ResponseEvent::RateLimits(snapshot))).await;
        }
        process_sse(stream_response.bytes, tx_event, idle_timeout, telemetry).await;
    });

    ResponseStream {
        rx_event,
        upstream_request_id,
    }
}

#[derive(Debug, Deserialize)]
struct ChatChunk {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    #[serde(default)]
    delta: ChatDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct ChatToolCallDelta {
    #[serde(default)]
    index: Option<i64>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<ChatFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct ChatFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: i64,
    #[serde(default)]
    prompt_tokens_details: Option<ChatPromptTokensDetails>,
    #[serde(default)]
    completion_tokens: i64,
    #[serde(default)]
    completion_tokens_details: Option<ChatCompletionTokensDetails>,
    #[serde(default)]
    total_tokens: i64,
}

#[derive(Debug, Deserialize)]
struct ChatPromptTokensDetails {
    #[serde(default)]
    cached_tokens: i64,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: i64,
}

impl From<ChatUsage> for TokenUsage {
    fn from(usage: ChatUsage) -> Self {
        TokenUsage {
            input_tokens: usage.prompt_tokens,
            cached_input_tokens: usage
                .prompt_tokens_details
                .map(|d| d.cached_tokens)
                .unwrap_or(0),
            output_tokens: usage.completion_tokens,
            reasoning_output_tokens: usage
                .completion_tokens_details
                .map(|d| d.reasoning_tokens)
                .unwrap_or(0),
            total_tokens: usage.total_tokens,
        }
    }
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

pub async fn process_sse(
    stream: ByteStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
) {
    let mut stream = stream.eventsource();
    let mut created_emitted = false;
    let mut response_id = String::new();
    let mut text_buffer = String::new();
    let mut tool_calls: Vec<ToolCallAccumulator> = Vec::new();
    let mut token_usage: Option<TokenUsage> = None;
    let mut saw_finish = false;

    loop {
        let start = Instant::now();
        let response = timeout(idle_timeout, stream.next()).await;
        if let Some(t) = telemetry.as_ref() {
            t.on_sse_poll(&response, start.elapsed());
        }
        let sse = match response {
            Ok(Some(Ok(sse))) => sse,
            Ok(Some(Err(e))) => {
                debug!("SSE Error: {e:#}");
                let _ = tx_event.send(Err(ApiError::Stream(e.to_string()))).await;
                return;
            }
            Ok(None) => {
                // Byte stream ended. If we saw a completion signal, finalize;
                // otherwise treat it as a premature close.
                if saw_finish {
                    let _ = emit_completion(
                        &tx_event,
                        std::mem::take(&mut response_id),
                        std::mem::take(&mut text_buffer),
                        std::mem::take(&mut tool_calls),
                        token_usage.take(),
                    )
                    .await;
                } else {
                    let _ = tx_event
                        .send(Err(ApiError::Stream(
                            "stream closed before response.completed".into(),
                        )))
                        .await;
                }
                return;
            }
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream("idle timeout waiting for SSE".into())))
                    .await;
                return;
            }
        };

        trace!("SSE event: {}", &sse.data);

        if sse.data.trim() == "[DONE]" {
            let _ = emit_completion(
                &tx_event,
                std::mem::take(&mut response_id),
                std::mem::take(&mut text_buffer),
                std::mem::take(&mut tool_calls),
                token_usage.take(),
            )
            .await;
            return;
        }

        let chunk: ChatChunk = match serde_json::from_str(&sse.data) {
            Ok(chunk) => chunk,
            Err(e) => {
                debug!("Failed to parse SSE event: {e}, data: {}", &sse.data);
                continue;
            }
        };

        if let Some(id) = chunk.id
            && response_id.is_empty()
        {
            response_id = id;
        }

        if !created_emitted {
            created_emitted = true;
            if tx_event.send(Ok(ResponseEvent::Created)).await.is_err() {
                return;
            }
        }

        if let Some(usage) = chunk.usage {
            token_usage = Some(usage.into());
        }

        for choice in chunk.choices {
            if let Some(content) = choice.delta.content
                && !content.is_empty()
            {
                text_buffer.push_str(&content);
                if tx_event
                    .send(Ok(ResponseEvent::OutputTextDelta(content)))
                    .await
                    .is_err()
                {
                    return;
                }
            }

            if let Some(deltas) = choice.delta.tool_calls {
                for delta in deltas {
                    let idx = delta.index.unwrap_or(0).max(0) as usize;
                    if tool_calls.len() <= idx {
                        tool_calls.resize_with(idx + 1, ToolCallAccumulator::default);
                    }
                    let acc = &mut tool_calls[idx];
                    if let Some(id) = delta.id
                        && !id.is_empty()
                    {
                        acc.id = id;
                    }
                    if let Some(function) = delta.function {
                        if let Some(name) = function.name
                            && !name.is_empty()
                        {
                            acc.name = name;
                        }
                        if let Some(arguments) = function.arguments {
                            acc.arguments.push_str(&arguments);
                        }
                    }
                }
            }

            if choice.finish_reason.is_some() {
                saw_finish = true;
            }
        }
    }
}

async fn emit_completion(
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    response_id: String,
    text_buffer: String,
    tool_calls: Vec<ToolCallAccumulator>,
    token_usage: Option<TokenUsage>,
) -> Result<(), ()> {
    if !text_buffer.is_empty() {
        let item = ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText { text: text_buffer }],
            phase: None,
        };
        if tx_event
            .send(Ok(ResponseEvent::OutputItemDone(item)))
            .await
            .is_err()
        {
            return Err(());
        }
    }

    for tool_call in tool_calls {
        // A valid tool call must at least name a function.
        if tool_call.name.is_empty() {
            continue;
        }
        let item = ResponseItem::FunctionCall {
            id: None,
            name: tool_call.name,
            namespace: None,
            arguments: tool_call.arguments,
            call_id: tool_call.id,
        };
        if tx_event
            .send(Ok(ResponseEvent::OutputItemDone(item)))
            .await
            .is_err()
        {
            return Err(());
        }
    }

    let _ = tx_event
        .send(Ok(ResponseEvent::Completed {
            response_id,
            token_usage,
            end_turn: None,
        }))
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use codex_client::TransportError;
    use futures::TryStreamExt;
    use tokio::sync::mpsc;
    use tokio_test::io::Builder as IoBuilder;
    use tokio_util::io::ReaderStream;

    fn idle_timeout() -> Duration {
        Duration::from_millis(1000)
    }

    async fn collect_events(chunks: &[&[u8]]) -> Vec<Result<ResponseEvent, ApiError>> {
        let mut builder = IoBuilder::new();
        for chunk in chunks {
            builder.read(chunk);
        }

        let reader = builder.build();
        let stream =
            ReaderStream::new(reader).map_err(|err| TransportError::Network(err.to_string()));
        let (tx, mut rx) = mpsc::channel::<Result<ResponseEvent, ApiError>>(16);
        tokio::spawn(process_sse(
            Box::pin(stream),
            tx,
            idle_timeout(),
            /*telemetry*/ None,
        ));

        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }
        events
    }

    fn sse(data: &str) -> String {
        format!("data: {data}\n\n")
    }

    #[tokio::test]
    async fn parses_text_tool_call_and_completed_with_usage() {
        let chunk1 = sse(
            r#"{"id":"chatcmpl-1","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"}}]}"#,
        );
        let chunk2 = sse(r#"{"id":"chatcmpl-1","choices":[{"index":0,"delta":{"content":" world"}}]}"#);
        let tool1 = sse(
            r#"{"id":"chatcmpl-1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":"}}]}}]}"#,
        );
        let tool2 = sse(
            r#"{"id":"chatcmpl-1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"sf\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        );
        let usage = sse(
            r#"{"id":"chatcmpl-1","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_tokens_details":{"cached_tokens":4},"completion_tokens_details":{"reasoning_tokens":2}}}"#,
        );
        let done = sse("[DONE]");

        let events = collect_events(&[
            chunk1.as_bytes(),
            chunk2.as_bytes(),
            tool1.as_bytes(),
            tool2.as_bytes(),
            usage.as_bytes(),
            done.as_bytes(),
        ])
        .await;

        // Created, OutputTextDelta("Hello"), OutputTextDelta(" world"),
        // OutputItemDone(Message), OutputItemDone(FunctionCall), Completed
        assert_matches!(events[0], Ok(ResponseEvent::Created));
        assert_matches!(&events[1], Ok(ResponseEvent::OutputTextDelta(d)) if d == "Hello");
        assert_matches!(&events[2], Ok(ResponseEvent::OutputTextDelta(d)) if d == " world");

        assert_matches!(
            &events[3],
            Ok(ResponseEvent::OutputItemDone(ResponseItem::Message { role, content, .. }))
                if role == "assistant"
                    && matches!(content.as_slice(), [ContentItem::OutputText { text }] if text == "Hello world")
        );

        assert_matches!(
            &events[4],
            Ok(ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            })) if name == "get_weather"
                && arguments == r#"{"city":"sf"}"#
                && call_id == "call_1"
        );

        match &events[5] {
            Ok(ResponseEvent::Completed {
                response_id,
                token_usage,
                end_turn,
            }) => {
                assert_eq!(response_id, "chatcmpl-1");
                assert!(end_turn.is_none());
                let usage = token_usage.as_ref().expect("expected usage");
                assert_eq!(usage.input_tokens, 10);
                assert_eq!(usage.cached_input_tokens, 4);
                assert_eq!(usage.output_tokens, 5);
                assert_eq!(usage.reasoning_output_tokens, 2);
                assert_eq!(usage.total_tokens, 15);
            }
            other => panic!("unexpected final event: {other:?}"),
        }

        assert_eq!(events.len(), 6);
    }

    #[tokio::test]
    async fn finalizes_on_finish_reason_without_done() {
        let chunk1 =
            sse(r#"{"id":"c","choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":"stop"}]}"#);

        let events = collect_events(&[chunk1.as_bytes()]).await;

        assert_matches!(events[0], Ok(ResponseEvent::Created));
        assert_matches!(&events[1], Ok(ResponseEvent::OutputTextDelta(d)) if d == "hi");
        assert_matches!(
            &events[2],
            Ok(ResponseEvent::OutputItemDone(ResponseItem::Message { .. }))
        );
        assert_matches!(
            &events[3],
            Ok(ResponseEvent::Completed { response_id, .. }) if response_id == "c"
        );
        assert_eq!(events.len(), 4);
    }

    #[tokio::test]
    async fn error_when_stream_closes_before_completion() {
        let chunk1 = sse(r#"{"id":"c","choices":[{"index":0,"delta":{"content":"hi"}}]}"#);

        let events = collect_events(&[chunk1.as_bytes()]).await;

        assert_matches!(events[0], Ok(ResponseEvent::Created));
        assert_matches!(&events[1], Ok(ResponseEvent::OutputTextDelta(_)));
        match events.last() {
            Some(Err(ApiError::Stream(msg))) => {
                assert_eq!(msg, "stream closed before response.completed");
            }
            other => panic!("expected stream error, got {other:?}"),
        }
    }
}
