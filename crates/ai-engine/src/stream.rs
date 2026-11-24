//! Response body helpers: UTF-8 safe chunking and usage parsing.

/// Splits a byte stream into valid UTF-8 strings, holding back a partial
/// multibyte character until the next chunk.
#[derive(Default)]
pub struct Utf8Chunker {
    pending: Vec<u8>,
}

impl Utf8Chunker {
    pub fn push(&mut self, bytes: &[u8]) -> Option<String> {
        self.pending.extend_from_slice(bytes);
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            // Invalid bytes: pass them on lossily rather than stall.
            Err(_) => self.pending.len(),
        };
        if valid == 0 {
            return None;
        }
        let rest = self.pending.split_off(valid);
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending = rest;
        Some(text)
    }

    pub fn finish(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        Some(text)
    }
}

/// Reads token usage from a Messages response: SSE events when streaming
/// (`message_start` carries input, `message_delta` the running output), or
/// the `usage` object of a JSON body.
pub struct UsageParser {
    streaming: bool,
    buffer: String,
    input: u32,
    output: u32,
    overflow: bool,
}

impl UsageParser {
    pub fn new(streaming: bool) -> Self {
        Self {
            streaming,
            buffer: String::new(),
            input: 0,
            output: 0,
            overflow: false,
        }
    }

    pub fn feed(&mut self, text: &str, cap: usize) {
        if self.overflow {
            return;
        }
        self.buffer.push_str(text);
        if !self.streaming {
            if self.buffer.len() > cap {
                self.overflow = true;
                self.buffer.clear();
            }
            return;
        }
        while let Some(end) = self.buffer.find('\n') {
            let line: String = self.buffer.drain(..=end).collect();
            if let Some(data) = line.trim_end().strip_prefix("data:")
                && let Ok(value) = serde_json::from_str::<serde_json::Value>(data.trim())
            {
                self.read_event(&value);
            }
        }
    }

    fn read_event(&mut self, value: &serde_json::Value) {
        match value["type"].as_str() {
            Some("message_start") => self.read_usage(&value["message"]["usage"]),
            Some("message_delta") => self.read_usage(&value["usage"]),
            _ => {}
        }
    }

    fn read_usage(&mut self, usage: &serde_json::Value) {
        let field = |name: &str| usage[name].as_u64().map(|v| v.min(u32::MAX as u64) as u32);
        let input: Option<u32> = [
            "input_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
        ]
        .iter()
        .filter_map(|name| field(name))
        .reduce(|a, b| a.saturating_add(b));
        if let Some(input) = input.filter(|input| *input > 0) {
            self.input = input;
        }
        if let Some(output) = field("output_tokens") {
            self.output = self.output.max(output);
        }
    }

    pub fn totals(&mut self) -> (u32, u32) {
        if !self.streaming && !self.overflow && !self.buffer.is_empty() {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.buffer) {
                self.read_usage(&value["usage"]);
            }
            self.buffer.clear();
        }
        (self.input, self.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_chunker_never_splits_a_character() {
        let text = "héllo ✓ done";
        let bytes = text.as_bytes();
        let mut chunker = Utf8Chunker::default();
        let mut out = String::new();
        for byte in bytes {
            if let Some(part) = chunker.push(std::slice::from_ref(byte)) {
                out.push_str(&part);
            }
        }
        if let Some(rest) = chunker.finish() {
            out.push_str(&rest);
        }
        assert_eq!(out, text);
    }

    #[test]
    fn usage_from_sse_split_across_chunks() {
        let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":120,\"cache_read_input_tokens\":30,\"output_tokens\":1}}}\n\n\
                   event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":42}}\n\n";
        let mut parser = UsageParser::new(true);
        let (a, b) = sse.split_at(37);
        parser.feed(a, 1 << 20);
        parser.feed(b, 1 << 20);
        assert_eq!(parser.totals(), (150, 42));
    }

    #[test]
    fn usage_from_json_body() {
        let mut parser = UsageParser::new(false);
        parser.feed(
            r#"{"content":[],"usage":{"input_tokens":7,"output_tokens":3}}"#,
            1 << 20,
        );
        assert_eq!(parser.totals(), (7, 3));
    }
}
