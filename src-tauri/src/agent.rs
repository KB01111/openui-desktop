//! The agent loop, replacing the template's in-process LangGraph agent.
//!
//! Each round streams one completion. Text streams straight through as AG-UI
//! events; tool calls are accumulated, executed natively, and fed back as
//! `tool` messages until the model answers without asking for more. Every
//! assistant segment and tool call gets fresh ids so the frontend can keep
//! interleaved prose and tool activity separate.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::contract::{AgUiEvent, ChatFrame, ChatMessage, ToolCall};
use crate::openai::{stream_chat, ChatRequest, ToolCallAccumulator, WireMessage, CANCELLED};
use crate::tools;

const MAX_ROUNDS: usize = 8;
const SYSTEM_PROMPT: &str = include_str!("../prompt.txt");

pub struct AgentContext {
    pub client: reqwest::Client,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub cancelled: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct RunOutcome {
    /// Messages produced by this run, to append to the thread.
    pub produced: Vec<ChatMessage>,
}

fn is_cancelled(cancelled: &AtomicBool) -> bool {
    cancelled.load(Ordering::Relaxed)
}

/// Builds the Chat Completions history: the OpenUI system prompt first, then
/// the thread exactly as the frontend holds it.
fn seed_messages(history: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(history.len() + 1);
    messages.push(ChatMessage::text("system", "system", SYSTEM_PROMPT));
    messages.extend(history.iter().filter(|message| message.role != "system").cloned());
    messages
}

struct RoundOutput {
    text: String,
    tool_calls: Vec<ToolCall>,
}

/// Streams one assistant round, emitting every text event for it as it arrives.
async fn run_round<F>(
    context: &AgentContext,
    messages: &[ChatMessage],
    cancelled: &Arc<AtomicBool>,
    emit: &mut F,
) -> Result<RoundOutput, String>
where
    F: FnMut(ChatFrame) + Send,
{
    let mut accumulator = ToolCallAccumulator::default();
    let mut text = String::new();
    let mut segment_open = false;
    let message_id = uuid::Uuid::new_v4().to_string();

    let request = ChatRequest {
        base_url: &context.base_url,
        api_key: &context.api_key,
        model: &context.model,
        messages: messages.iter().map(WireMessage::from).collect(),
        tools: tools::specs(),
    };

    let flag = cancelled.clone();
    let is_cancelled_now = move || is_cancelled(&flag);

    let result = stream_chat(&context.client, request, is_cancelled_now, |chunk| {
        if let Some(delta) = chunk.content {
            if !delta.is_empty() {
                if !segment_open {
                    emit(ChatFrame::Event {
                        event: AgUiEvent::TextMessageStart {
                            message_id: message_id.clone(),
                            role: "assistant".to_string(),
                        },
                    });
                    segment_open = true;
                }
                text.push_str(&delta);
                emit(ChatFrame::Event {
                    event: AgUiEvent::TextMessageContent {
                        message_id: message_id.clone(),
                        delta,
                    },
                });
            }
        }
        for delta in chunk.tool_call_deltas {
            accumulator.apply(delta);
        }
        Ok(())
    })
    .await;

    if segment_open {
        emit(ChatFrame::Event {
            event: AgUiEvent::TextMessageEnd {
                message_id: message_id.clone(),
            },
        });
    }

    result?;

    Ok(RoundOutput {
        text,
        tool_calls: accumulator.finish(),
    })
}

pub async fn run<F>(
    context: &AgentContext,
    history: &[ChatMessage],
    emit: &mut F,
) -> Result<RunOutcome, String>
where
    F: FnMut(ChatFrame) + Send,
{
    let mut messages = seed_messages(history);
    let mut produced = Vec::new();
    let cancelled = context.cancelled.clone();

    for _ in 0..MAX_ROUNDS {
        if is_cancelled(&cancelled) {
            return Ok(RunOutcome { produced });
        }

        let output = match run_round(context, &messages, &cancelled, emit).await {
            Ok(output) => output,
            Err(error) if error == CANCELLED => return Ok(RunOutcome { produced }),
            Err(error) => {
                emit(ChatFrame::Event {
                    event: AgUiEvent::RunError {
                        message: error.clone(),
                        code: None,
                    },
                });
                return Err(error);
            }
        };

        let assistant = if output.tool_calls.is_empty() {
            ChatMessage::text(uuid::Uuid::new_v4().to_string(), "assistant", output.text)
        } else {
            ChatMessage::assistant_with_tools(
                uuid::Uuid::new_v4().to_string(),
                Some(output.text).filter(|text| !text.is_empty()),
                output.tool_calls.clone(),
            )
        };
        produced.push(assistant.clone());
        messages.push(assistant);

        if output.tool_calls.is_empty() {
            return Ok(RunOutcome { produced });
        }

        for call in output.tool_calls {
            if is_cancelled(&cancelled) {
                return Ok(RunOutcome { produced });
            }

            emit(ChatFrame::Event {
                event: AgUiEvent::ToolCallStart {
                    tool_call_id: call.id.clone(),
                    tool_call_name: call.function.name.clone(),
                },
            });
            emit(ChatFrame::Event {
                event: AgUiEvent::ToolCallArgs {
                    tool_call_id: call.id.clone(),
                    delta: call.function.arguments.clone(),
                },
            });
            emit(ChatFrame::Event {
                event: AgUiEvent::ToolCallEnd {
                    tool_call_id: call.id.clone(),
                },
            });

            let content =
                tools::run(&context.client, &call.function.name, &call.function.arguments).await;

            emit(ChatFrame::Event {
                event: AgUiEvent::ToolCallResult {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    tool_call_id: call.id.clone(),
                    content: content.clone(),
                    role: "tool".to_string(),
                    is_error: None,
                    error: None,
                },
            });

            let result_message =
                ChatMessage::tool_result(uuid::Uuid::new_v4().to_string(), call.id.clone(), content);
            produced.push(result_message.clone());
            messages.push(result_message);
        }
    }

    let message = format!("Stopped after {MAX_ROUNDS} tool rounds without a final answer.");
    emit(ChatFrame::Event {
        event: AgUiEvent::RunError {
            message: message.clone(),
            code: Some("max_rounds".to_string()),
        },
    });
    Err(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_prompt_leads_and_history_system_messages_are_dropped() {
        let history = vec![
            ChatMessage::text("s", "system", "stale"),
            ChatMessage::text("u", "user", "hi"),
        ];
        let seeded = seed_messages(&history);
        assert_eq!(seeded.len(), 2);
        assert_eq!(seeded[0].role, "system");
        assert_eq!(seeded[1].role, "user");
        assert!(seeded[0]
            .content
            .as_ref()
            .map(|content| content.as_text().len() > 1000)
            .unwrap_or(false));
    }

    #[test]
    fn the_embedded_prompt_is_present() {
        assert!(SYSTEM_PROMPT.len() > 1000);
    }
}
