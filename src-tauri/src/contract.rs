//! Wire types shared with the React frontend.
//!
//! The frontend speaks canonical AG-UI messages for both transport and storage,
//! so this module is the single place that knows how they serialize. Naming is
//! camelCase throughout because the TypeScript contract in `src/lib/contract.ts`
//! is the source of truth.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrlRef {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlRef },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl Content {
    /// Flattens to plain text, which is all the title derivation and the tool
    /// loop need. Images contribute nothing.
    pub fn as_text(&self) -> String {
        match self {
            Content::Text(text) => text.clone(),
            Content::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text } => Some(text.as_str()),
                    ContentPart::ImageUrl { .. } => None,
                })
                .collect::<Vec<_>>()
                .join(""),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ChatMessage {
    pub fn text(id: impl Into<String>, role: &str, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: role.to_string(),
            content: Some(Content::Text(text.into())),
            tool_calls: None,
            tool_call_id: None,
            error: None,
        }
    }

    pub fn assistant_with_tools(
        id: impl Into<String>,
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            id: id.into(),
            role: "assistant".to_string(),
            content: content.filter(|text| !text.is_empty()).map(Content::Text),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            error: None,
        }
    }

    pub fn tool_result(
        id: impl Into<String>,
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            role: "tool".to_string(),
            content: Some(Content::Text(content.into())),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            error: None,
        }
    }
}

/// The subset of AG-UI the chat consumes. Tag and field names must stay in sync
/// with `AgUiEvent` in `src/lib/contract.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgUiEvent {
    #[serde(rename_all = "camelCase")]
    TextMessageStart { message_id: String, role: String },
    #[serde(rename_all = "camelCase")]
    TextMessageContent { message_id: String, delta: String },
    #[serde(rename_all = "camelCase")]
    TextMessageEnd { message_id: String },
    #[serde(rename_all = "camelCase")]
    ToolCallStart {
        tool_call_id: String,
        tool_call_name: String,
    },
    #[serde(rename_all = "camelCase")]
    ToolCallArgs { tool_call_id: String, delta: String },
    #[serde(rename_all = "camelCase")]
    ToolCallEnd { tool_call_id: String },
    #[serde(rename_all = "camelCase")]
    ToolCallResult {
        message_id: String,
        tool_call_id: String,
        content: String,
        role: String,
        #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    RunError {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
}

/// One frame on the streaming channel. `kind` discriminates the three shapes.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ChatFrame {
    Event { event: AgUiEvent },
    Error { message: String },
    Done,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRunRequest {
    pub run_id: String,
    pub thread_id: String,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredThread {
    pub id: String,
    pub title: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadList {
    pub threads: Vec<StoredThread>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// What the frontend may read. The API key never crosses the boundary.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub base_url: String,
    pub model: String,
    pub api_key_set: bool,
    pub temperature: f64,
    pub max_tokens: Option<u32>,
    pub instructions: Option<String>,
    pub weather_tool: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub weather_tool: Option<bool>,
    #[serde(default)]
    pub remove_api_key: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(frame: ChatFrame) -> String {
        serde_json::to_string(&frame).expect("frames must serialize")
    }

    #[test]
    fn frames_match_the_frontend_contract() {
        assert_eq!(
            frame(ChatFrame::Event {
                event: AgUiEvent::TextMessageStart {
                    message_id: "m1".into(),
                    role: "assistant".into(),
                }
            }),
            r#"{"kind":"event","event":{"type":"TEXT_MESSAGE_START","messageId":"m1","role":"assistant"}}"#
        );

        assert_eq!(
            frame(ChatFrame::Event {
                event: AgUiEvent::ToolCallResult {
                    message_id: "m2".into(),
                    tool_call_id: "c1".into(),
                    content: "{}".into(),
                    role: "tool".into(),
                    is_error: None,
                    error: None,
                }
            }),
            r#"{"kind":"event","event":{"type":"TOOL_CALL_RESULT","messageId":"m2","toolCallId":"c1","content":"{}","role":"tool"}}"#
        );

        assert_eq!(
            frame(ChatFrame::Error {
                message: "boom".into()
            }),
            r#"{"kind":"error","message":"boom"}"#
        );
        assert_eq!(frame(ChatFrame::Done), r#"{"kind":"done"}"#);
    }

    #[test]
    fn run_error_carries_an_optional_code() {
        assert_eq!(
            frame(ChatFrame::Event {
                event: AgUiEvent::RunError {
                    message: "no key".into(),
                    code: Some("missing_api_key".into()),
                }
            }),
            r#"{"kind":"event","event":{"type":"RUN_ERROR","message":"no key","code":"missing_api_key"}}"#
        );
    }

    #[test]
    fn messages_round_trip_through_the_frontend_shape() {
        let raw = r#"{"id":"m1","role":"assistant","toolCalls":[{"id":"c1","type":"function","function":{"name":"get_weather","arguments":"{\"location\":\"Berlin\"}"}}]}"#;
        let message: ChatMessage = serde_json::from_str(raw).expect("must parse");
        assert_eq!(message.role, "assistant");
        let calls = message.tool_calls.expect("tool calls");
        assert_eq!(calls[0].function.name, "get_weather");

        let text = r#"{"id":"m2","role":"user","content":[{"type":"text","text":"hi"}]}"#;
        let message: ChatMessage = serde_json::from_str(text).expect("must parse");
        assert_eq!(message.content.expect("content").as_text(), "hi");
    }

    #[test]
    fn settings_patch_accepts_the_frontend_shape() {
        let patch: SettingsPatch = serde_json::from_str(
            r#"{"temperature":1.1,"maxTokens":512,"instructions":"Be brief.","weatherTool":false,"removeApiKey":true}"#,
        )
        .expect("patch must parse");
        assert_eq!(patch.temperature, Some(1.1));
        assert_eq!(patch.max_tokens, Some(512));
        assert_eq!(patch.instructions.as_deref(), Some("Be brief."));
        assert_eq!(patch.weather_tool, Some(false));
        assert_eq!(patch.remove_api_key, Some(true));
    }
}
