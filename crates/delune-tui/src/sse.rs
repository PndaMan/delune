//! A small incremental Server-Sent Events parser.
//!
//! Only the parts the delune API uses: `data:` lines joined by newlines, events
//! separated by a blank line, comments (`:` keep-alives) ignored. Feed it chunks as
//! they arrive from the network; complete events come out.

#[derive(Debug, Default)]
pub struct SseParser {
    buffer: String,
    data: Vec<String>,
}

impl SseParser {
    /// Feed a chunk of bytes; returns the `data` payload of every event it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));
        let mut events = Vec::new();
        while let Some(end) = self.buffer.find('\n') {
            let line: String = self.buffer.drain(..=end).collect();
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    events.push(self.data.join("\n"));
                    self.data.clear();
                }
            } else if let Some(value) = line.strip_prefix("data:") {
                self.data.push(value.strip_prefix(' ').unwrap_or(value).to_owned());
            }
            // `event:`, `id:`, `retry:` and `:` comments are not used by the API.
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_split_across_chunks() {
        let mut parser = SseParser::default();
        assert!(parser.push(b"data: {\"a\":").is_empty());
        assert!(parser.push(b"1}\n").is_empty());
        assert_eq!(parser.push(b"\n: keep-alive\n\ndata: two\r\n\r\n"), vec!["{\"a\":1}", "two"]);
    }

    #[test]
    fn joins_multiline_data() {
        let mut parser = SseParser::default();
        assert_eq!(parser.push(b"data: one\ndata: two\n\n"), vec!["one\ntwo"]);
    }
}
