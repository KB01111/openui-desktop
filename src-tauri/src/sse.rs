//! Incremental server-sent-events decoding.
//!
//! Network reads split wherever they like, including mid-line, so the partial
//! tail is held until the next chunk completes it. Every `data:` payload is
//! returned whole; blank lines and non-data fields are skipped.

#[derive(Default)]
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one network chunk and returns the payloads it completed.
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.buffer.push_str(chunk);
        let mut payloads = Vec::new();

        while let Some(index) = self.buffer.find('\n') {
            let line: String = self.buffer.drain(..=index).collect();
            if let Some(payload) = parse_line(&line) {
                payloads.push(payload);
            }
        }

        payloads
    }

    /// Flushes a final payload that arrived without a trailing newline.
    pub fn finish(&mut self) -> Vec<String> {
        let tail = std::mem::take(&mut self.buffer);
        parse_line(&tail).into_iter().collect()
    }
}

fn parse_line(line: &str) -> Option<String> {
    let line = line.trim_end_matches(['\r', '\n']);
    let payload = line.strip_prefix("data:")?.trim();
    if payload.is_empty() {
        None
    } else {
        Some(payload.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reassembles_a_payload_split_across_chunks() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push("data: {\"choices\":[{\"delta\":{\"con").is_empty());
        assert!(decoder.push("tent\":\"Hel").is_empty());
        let payloads = decoder.push("lo\"}}]}\n\n");
        assert_eq!(payloads, vec![r#"{"choices":[{"delta":{"content":"Hello"}}]}"#]);
    }

    #[test]
    fn handles_crlf_and_multiple_events_per_chunk() {
        let mut decoder = SseDecoder::new();
        let payloads = decoder.push("data: {\"a\":1}\r\n\r\ndata: {\"b\":2}\r\n\r\n");
        assert_eq!(payloads, vec![r#"{"a":1}"#.to_string(), r#"{"b":2}"#.to_string()]);
    }

    #[test]
    fn passes_through_the_done_sentinel_and_ignores_noise() {
        let mut decoder = SseDecoder::new();
        let payloads = decoder.push(": keep-alive\n\nevent: x\ndata: [DONE]\n\n");
        assert_eq!(payloads, vec!["[DONE]".to_string()]);
    }

    #[test]
    fn flushes_a_final_line_without_a_newline() {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push("data: {\"tail\":true}").is_empty());
        assert_eq!(decoder.finish(), vec![r#"{"tail":true}"#.to_string()]);
    }
}
