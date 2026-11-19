//! Chat Completions streaming client.
//!
//! Only forward-compatible JSON is read: unknown fields are ignored so a
//! provider that adds response fields keeps working. The wire shapes here are
//! deliberately small — they cover deltas, tool-call fragments, and errors.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::contract::{ChatMessage, Content, ToolCall};
use crate::sse::SseDecoder;

#[derive(Debug, Clone, Serialize)]
pub struct WireFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: WireFunction,
}

#[derive(Debug, Clone, Serialize)]
pub struct WireMessage {
    pub role: String,
    pub content: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WireToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl From<&ChatMessage> for WireMessage {
    fn from(message: &ChatMessage) -> Self {
        Self {
            role: message.role.clone(),
            content: message.content.clone(),
            tool_calls: message.tool_calls.as_ref().map(|calls| {
                calls
                    .iter()
                    .map(|call| WireToolCall {
                        id: call.id.clone(),
                        kind: "function",
                        function: WireFunction {
                            name: call.function.name.clone(),
                            arguments: call.function.arguments.clone(),
                        },
                    })
                    .collect()
            }),
            tool_call_id: message.tool_call_id.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FunctionSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ToolSpec {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: FunctionSpec,
}

pub struct ChatRequest<'a> {
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub model: &'a str,
    pub messages: Vec<WireMessage>,
    pub tools: Vec<ToolSpec>,
}

/// A streamed chunk of assistant output, already flattened.
#[derive(Debug, Default)]
pub struct Chunk {
    pub content: Option<String>,
    pub tool_call_deltas: Vec<ToolCallDelta>,
}

#[derive(Debug, Clone)]
pub struct ToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamResponse {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Option<StreamDelta>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFunction>,
}

#[derive(Debug, Deserialize)]
struct StreamFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(default)]
    message: Option<String>,
}

fn parse_chunk(payload: &str) -> Result<Option<Chunk>, String> {
    let parsed: StreamResponse =
        serde_json::from_str(payload).map_err(|error| format!("Malformed stream event: {error}"))?;

    if let Some(error) = parsed.error {
        return Err(error
            .message
            .unwrap_or_else(|| "The provider returned an error".to_string()));
    }

    let Some(delta) = parsed.choices.into_iter().next().and_then(|choice| choice.delta) else {
        return Ok(None);
    };

    let tool_call_deltas = delta
        .tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(|call| ToolCallDelta {
            index: call.index,
            id: call.id,
            name: call.function.as_ref().and_then(|function| function.name.clone()),
            arguments: call
                .function
                .as_ref()
                .and_then(|function| function.arguments.clone()),
        })
        .collect::<Vec<_>>();

    if delta.content.is_none() && tool_call_deltas.is_empty() {
        return Ok(None);
    }

    Ok(Some(Chunk {
        content: delta.content,
        tool_call_deltas,
    }))
}

/// Accumulates fragments into complete tool calls, keyed by wire index.
#[derive(Default)]
pub struct ToolCallAccumulator {
    calls: Vec<(usize, ToolCall)>,
}

impl ToolCallAccumulator {
    pub fn apply(&mut self, delta: ToolCallDelta) {
        if let Some(existing) = self.calls.iter_mut().find(|(index, _)| *index == delta.index) {
            let call = &mut existing.1;
            if let Some(id) = delta.id {
                if !id.is_empty() {
                    call.id = id;
                }
            }
            if let Some(name) = delta.name {
                if !name.is_empty() {
                    call.function.name = name;
                }
            }
            if let Some(arguments) = delta.arguments {
                call.function.arguments.push_str(&arguments);
            }
            return;
        }

        let mut call = ToolCall {
            id: delta.id.unwrap_or_default(),
            kind: "function".to_string(),
            function: crate::contract::FunctionCall {
                name: delta.name.unwrap_or_default(),
                arguments: String::new(),
            },
        };
        if let Some(arguments) = delta.arguments {
            call.function.arguments.push_str(&arguments);
        }
        self.calls.push((delta.index, call));
    }

    /// Returns complete calls, filling in ids a provider may have omitted.
    pub fn finish(self) -> Vec<ToolCall> {
        let mut calls = self.calls;
        calls.sort_by_key(|(index, _)| *index);
        calls
            .into_iter()
            .map(|(_, mut call)| {
                if call.id.is_empty() {
                    call.id = format!("call_{}", uuid::Uuid::new_v4());
                }
                if call.kind.is_empty() {
                    call.kind = "function".to_string();
                }
                call
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }
}

fn build_body(request: &ChatRequest<'_>) -> serde_json::Value {
    serde_json::json!({
        "model": request.model,
        "messages": request.messages,
        "tools": request.tools,
        "stream": true,
    })
}

/// Streams the completion, invoking `on_chunk` for each parsed delta.
///
/// Returning `Err` aborts the run. A cancelled run surfaces as `Err` with the
/// `CANCELLED` sentinel so the caller can tell it apart from a real failure.
pub const CANCELLED: &str = "cancelled";

pub async fn stream_chat<F, C>(
    client: &reqwest::Client,
    request: ChatRequest<'_>,
    cancelled: C,
    mut on_chunk: F,
) -> Result<(), String>
where
    F: FnMut(Chunk) -> Result<(), String>,
    C: Fn() -> bool,
{
    if cancelled() {
        return Err(CANCELLED.to_string());
    }

    let url = format!("{}/chat/completions", request.base_url);
    let body = build_body(&request);

    let response = client
        .post(&url)
        .bearer_auth(request.api_key)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Could not reach the model provider: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<StreamResponse>(&detail)
            .ok()
            .and_then(|parsed| parsed.error)
            .and_then(|error| error.message)
            .unwrap_or_else(|| {
                let trimmed = detail.trim();
                if trimmed.is_empty() {
                    status.to_string()
                } else {
                    trimmed.chars().take(400).collect()
                }
            });
        return Err(format!("The model provider rejected the request ({status}): {message}"));
    }

    let mut stream = response.bytes_stream();
    let mut decoder = SseDecoder::new();

    while let Some(next) = stream.next().await {
        if cancelled() {
            return Err(CANCELLED.to_string());
        }

        let bytes = next.map_err(|error| format!("The stream failed mid-response: {error}"))?;
        let text = String::from_utf8_lossy(&bytes);

        for payload in decoder.push(&text) {
            if payload == "[DONE]" {
                return Ok(());
            }
            if let Some(chunk) = parse_chunk(&payload)? {
                on_chunk(chunk)?;
            }
        }
    }

    for payload in decoder.finish() {
        if payload == "[DONE]" {
            break;
        }
        if let Some(chunk) = parse_chunk(&payload)? {
            on_chunk(chunk)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_deltas() {
        let chunk = parse_chunk(r#"{"choices":[{"delta":{"content":"Hi"}}]}"#)
            .expect("parse")
            .expect("chunk");
        assert_eq!(chunk.content.as_deref(), Some("Hi"));
        assert!(chunk.tool_call_deltas.is_empty());
    }

    #[test]
    fn ignores_role_only_and_empty_deltas() {
        assert!(parse_chunk(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#)
            .expect("parse")
            .is_none());
        assert!(parse_chunk(r#"{"choices":[]}"#).expect("parse").is_none());
    }

    #[test]
    fn surfaces_provider_errors() {
        let error = parse_chunk(r#"{"error":{"message":"bad key"}}"#).expect_err("must fail");
        assert!(error.contains("bad key"));
    }

    #[test]
    fn ignores_unknown_fields() {
        let chunk = parse_chunk(
            r#"{"id":"x","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":null}],"usage":null}"#,
        )
        .expect("parse")
        .expect("chunk");
        assert_eq!(chunk.content.as_deref(), Some("ok"));
    }

    #[test]
    fn accumulates_fragmented_tool_calls_by_index() {
        let mut accumulator = ToolCallAccumulator::default();
        accumulator.apply(ToolCallDelta {
            index: 0,
            id: Some("call_1".into()),
            name: Some("get_weather".into()),
            arguments: Some(String::new()),
        });
        accumulator.apply(ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments: Some("{\"location\":".into()),
        });
        accumulator.apply(ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments: Some("\"Berlin\"}".into()),
        });

        let calls = accumulator.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].function.name, "get_weather");
        assert_eq!(calls[0].function.arguments, r#"{"location":"Berlin"}"#);
    }

    #[test]
    fn synthesizes_ids_when_a_provider_omits_them() {
        let mut accumulator = ToolCallAccumulator::default();
        accumulator.apply(ToolCallDelta {
            index: 0,
            id: None,
            name: Some("get_weather".into()),
            arguments: Some("{}".into()),
        });
        let calls = accumulator.finish();
        assert!(calls[0].id.starts_with("call_"));
    }

    #[test]
    fn body_requests_a_stream_and_keeps_tools() {
        let request = ChatRequest {
            base_url: "http://x/v1",
            api_key: "k",
            model: "m",
            messages: vec![WireMessage::from(&ChatMessage::text("1", "user", "hi"))],
            tools: Vec::new(),
        };
        let body = build_body(&request);
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn content_parts_survive_the_round_trip() {
        let message = ChatMessage {
            id: "1".into(),
            role: "user".into(),
            content: Some(Content::Parts(vec![crate::contract::ContentPart::Text {
                text: "hi".into(),
            }])),
            tool_calls: None,
            tool_call_id: None,
            error: None,
        };
        let wire = WireMessage::from(&message);
        let json = serde_json::to_value(&wire).expect("json");
        assert_eq!(json["content"][0]["type"], "text");
    }
}
