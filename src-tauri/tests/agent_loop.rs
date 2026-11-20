//! End-to-end test of the agent loop against a local Chat Completions server.
//!
//! This is the core of the desktop port: build the request, parse a chunked SSE
//! stream, execute a tool natively, feed the result back, and emit AG-UI events
//! in the order the chat UI requires.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use helios_ai_lib::agent::{self, AgentContext};
use helios_ai_lib::contract::ChatFrame;
use helios_ai_lib::contract::ChatMessage;

/// One scripted response body: the SSE frames a provider would send.
fn sse(events: &[&str]) -> String {
    events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>()
        + "data:"
}

/// Reads one complete HTTP request, returning its body.
///
/// The response must not be sent before the body has arrived: closing the
/// socket mid-write would surface as a transport error instead of the status
/// code under test.
fn read_request(socket: &mut std::net::TcpStream) -> String {
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match socket.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                raw.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&raw);
                if let Some(index) = text.find("\r\n\r\n") {
                    let length = text[..index]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if raw.len() >= index + 4 + length {
                        return text[index + 4..].to_string();
                    }
                }
            }
            Err(_) => break,
        }
    }
    String::new()
}

/// Serves `responses` in order, one per accepted connection, recording requests.
fn serve(responses: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let sink = recorded.clone();

    thread::spawn(move || {
        for body in responses {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };

            let request_body = read_request(&mut socket);
            sink.lock().expect("lock").push(request_body);

            // Split the body across writes to exercise the SSE reassembly path.
            let head =
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
            let _ = socket.write_all(head.as_bytes());
            for chunk in body.as_bytes().chunks(17) {
                let _ = socket.write_all(chunk);
                let _ = socket.flush();
            }
        }
    });

    (format!("http://127.0.0.1:{port}"), recorded)
}

fn collect_events(frames: &[ChatFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|frame| match frame {
            ChatFrame::Event { event } => Some(
                serde_json::to_value(event).expect("json")["type"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            ),
            _ => None,
        })
        .collect()
}

fn text(payload: &str) -> String {
    format!(r#"{{"choices":[{{"delta":{{"content":"{payload}"}}}}]}}"#)
}

#[tokio::test]
async fn streams_text_and_executes_a_tool_round() {
    // Round one: prose, then a fragmented tool call.
    let first = sse(&[
        &text("Let me check."),
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather","arguments":""}}]}}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"location\":"}}]}}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"Nowhere\"}"}}]}}]}"#,
    ]);
    // Round two: the final answer, no tools.
    let second = sse(&[&text("Done.")]);

    let (base_url, recorded) = serve(vec![first, second]);

    let context = AgentContext {
        client: reqwest::Client::new(),
        base_url,
        model: "test-model".to_string(),
        api_key: "test-key".to_string(),
        temperature: 0.7,
        max_tokens: None,
        instructions: None,
        weather_tool: true,
        cancelled: Arc::new(AtomicBool::new(false)),
    };

    let mut frames = Vec::new();
    let mut emit = |frame: ChatFrame| frames.push(frame);

    let history = vec![ChatMessage::text("u1", "user", "weather?")];
    let outcome = agent::run(&context, &history, &mut emit)
        .await
        .expect("run");

    let types = collect_events(&frames);
    assert_eq!(
        types,
        vec![
            "TEXT_MESSAGE_START",
            "TEXT_MESSAGE_CONTENT",
            "TEXT_MESSAGE_END",
            "TOOL_CALL_START",
            "TOOL_CALL_ARGS",
            "TOOL_CALL_END",
            "TOOL_CALL_RESULT",
            "TEXT_MESSAGE_START",
            "TEXT_MESSAGE_CONTENT",
            "TEXT_MESSAGE_END",
        ]
    );

    // Assistant (with the tool call), the tool result, then the final assistant.
    assert_eq!(outcome.produced.len(), 3);
    assert_eq!(outcome.produced[0].role, "assistant");
    assert_eq!(
        outcome.produced[0].tool_calls.as_ref().expect("calls")[0]
            .function
            .name,
        "get_weather"
    );
    assert_eq!(outcome.produced[1].role, "tool");
    assert_eq!(outcome.produced[2].role, "assistant");

    // The second request must carry the system prompt, the user message, the
    // assistant tool call, and the tool result.
    let requests = recorded.lock().expect("lock");
    assert_eq!(requests.len(), 2);
    let second_request: serde_json::Value = serde_json::from_str(&requests[1]).expect("json");
    let roles: Vec<&str> = second_request["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|message| message["role"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(roles, vec!["system", "user", "assistant", "tool"]);
    assert_eq!(second_request["stream"], true);
    assert!(
        second_request["messages"][0]["content"]
            .as_str()
            .expect("system prompt")
            .len()
            > 1000
    );
}

#[tokio::test]
async fn generation_settings_reach_the_request() {
    let (base_url, recorded) = serve(vec![sse(&[&text("Hi")])]);

    let context = AgentContext {
        client: reqwest::Client::new(),
        base_url,
        model: "test-model".to_string(),
        api_key: "test-key".to_string(),
        temperature: 1.3,
        max_tokens: Some(256),
        instructions: Some("Answer in haiku.".to_string()),
        weather_tool: false,
        cancelled: Arc::new(AtomicBool::new(false)),
    };

    let mut frames = Vec::new();
    let mut emit = |frame: ChatFrame| frames.push(frame);
    agent::run(
        &context,
        &[ChatMessage::text("u1", "user", "hi")],
        &mut emit,
    )
    .await
    .expect("run");

    let requests = recorded.lock().expect("lock");
    let body: serde_json::Value = serde_json::from_str(&requests[0]).expect("json");
    assert_eq!(body["temperature"], 1.3);
    assert_eq!(body["max_tokens"], 256);
    assert_eq!(body["tools"].as_array().expect("tools").len(), 0);
    assert!(body["messages"][0]["content"]
        .as_str()
        .expect("system prompt")
        .contains("Answer in haiku."));
}

#[tokio::test]
async fn reports_provider_failures_as_a_run_error() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    thread::spawn(move || {
        if let Ok((mut socket, _)) = listener.accept() {
            read_request(&mut socket);
            let body = r#"{"error":{"message":"invalid api key"}}"#;
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes());
        }
    });

    let context = AgentContext {
        client: reqwest::Client::new(),
        base_url: format!("http://127.0.0.1:{port}"),
        model: "m".to_string(),
        api_key: "k".to_string(),
        temperature: 0.7,
        max_tokens: None,
        instructions: None,
        weather_tool: true,
        cancelled: Arc::new(AtomicBool::new(false)),
    };

    let mut frames = Vec::new();
    let mut emit = |frame: ChatFrame| frames.push(frame);
    let result = agent::run(
        &context,
        &[ChatMessage::text("u1", "user", "hi")],
        &mut emit,
    )
    .await;

    let error = result.expect_err("must fail");
    assert!(error.contains("invalid api key"), "got: {error}");
    assert_eq!(collect_events(&frames), vec!["RUN_ERROR"]);
}

#[tokio::test]
async fn a_cancelled_run_stops_without_erroring() {
    let (base_url, _recorded) = serve(vec![sse(&[&text("ignored")])]);
    let cancelled = Arc::new(AtomicBool::new(true));

    let context = AgentContext {
        client: reqwest::Client::new(),
        base_url,
        model: "m".to_string(),
        api_key: "k".to_string(),
        temperature: 0.7,
        max_tokens: None,
        instructions: None,
        weather_tool: true,
        cancelled,
    };

    let mut frames = Vec::new();
    let mut emit = |frame: ChatFrame| frames.push(frame);
    let outcome = agent::run(
        &context,
        &[ChatMessage::text("u1", "user", "hi")],
        &mut emit,
    )
    .await
    .expect("cancelled runs are clean");

    assert!(outcome.produced.is_empty());
    assert!(frames.is_empty());
}
