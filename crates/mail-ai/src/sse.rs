//! Incremental parser for server-sent events (WHATWG HTML, "event stream interpretation"):
//! lines end with LF, CRLF or CR (also split across chunks); `:` starts a comment (keep-alives
//! of OpenRouter); `data` lines are joined with `\n`; an empty line dispatches the event; an
//! event without a terminating empty line at the end of the stream is discarded. `id` and
//! `retry` are ignored (no reconnecting). Lines and events have a size limit, and text must be
//! UTF-8; anything else is a protocol error.

use std::collections::VecDeque;

use crate::AiError;

/// One dispatched event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SseEvent {
    /// The `event:` field; `None` means the default type ("message").
    pub name: Option<String>,
    pub data: String,
}

#[derive(Debug)]
pub(crate) struct SseParser {
    line: Vec<u8>,
    /// The previous chunk ended with CR: a LF at the start of the next one belongs to it.
    after_cr: bool,
    /// Nothing seen yet (a BOM is skipped only at the very start).
    at_start: bool,
    name: Option<String>,
    data: String,
    has_data: bool,
    ready: VecDeque<SseEvent>,
    max_event_bytes: usize,
}

impl SseParser {
    pub fn new(max_event_bytes: usize) -> Self {
        Self {
            line: Vec::new(),
            after_cr: false,
            at_start: true,
            name: None,
            data: String::new(),
            has_data: false,
            ready: VecDeque::new(),
            max_event_bytes,
        }
    }

    /// Takes the next bytes of the stream.
    pub fn feed(&mut self, mut bytes: &[u8]) -> Result<(), AiError> {
        if self.at_start && !bytes.is_empty() {
            const BOM: &[u8] = b"\xEF\xBB\xBF";
            // A BOM split over chunks is not worth handling: servers send it whole if at all.
            if let Some(rest) = bytes.strip_prefix(BOM) {
                bytes = rest;
            }
            self.at_start = false;
        }
        for &byte in bytes {
            if self.after_cr {
                self.after_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\n' => self.end_line()?,
                b'\r' => {
                    self.end_line()?;
                    self.after_cr = true;
                }
                _ => {
                    if self.line.len() >= self.max_event_bytes {
                        return Err(AiError::Protocol("event stream line too long".into()));
                    }
                    self.line.push(byte);
                }
            }
        }
        Ok(())
    }

    /// The next complete event, if any.
    pub fn next_event(&mut self) -> Option<SseEvent> {
        self.ready.pop_front()
    }

    fn end_line(&mut self) -> Result<(), AiError> {
        let line = std::mem::take(&mut self.line);
        let line = String::from_utf8(line)
            .map_err(|_| AiError::Protocol("event stream is not UTF-8".into()))?;
        if line.is_empty() {
            self.dispatch();
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line.as_str(), ""),
        };
        match field {
            "event" => self.name = Some(value.to_owned()),
            "data" => {
                if self.data.len() + value.len() + 1 > self.max_event_bytes {
                    return Err(AiError::Protocol("event stream event too large".into()));
                }
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn dispatch(&mut self) {
        let name = self.name.take().filter(|name| !name.is_empty());
        if !self.has_data {
            // An `event:` line without data dispatches nothing.
            return;
        }
        self.has_data = false;
        self.ready.push_back(SseEvent {
            name,
            data: std::mem::take(&mut self.data),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(chunks: &[&[u8]]) -> Result<Vec<SseEvent>, AiError> {
        let mut parser = SseParser::new(1024);
        let mut events = Vec::new();
        for chunk in chunks {
            parser.feed(chunk)?;
            while let Some(event) = parser.next_event() {
                events.push(event);
            }
        }
        Ok(events)
    }

    fn event(name: Option<&str>, data: &str) -> SseEvent {
        SseEvent {
            name: name.map(str::to_owned),
            data: data.to_owned(),
        }
    }

    #[test]
    fn events_lines_and_comments() {
        let events = parse(&[
            b"\xEF\xBB\xBF: keep-alive\n\nevent: message_start\ndata: {\"a\":1}\n\n",
            b"data: first\ndata:second\n\n",
            b"event: ping\n\n",
            b"data: [DONE]\n\n",
        ])
        .expect("parse");
        assert_eq!(
            events,
            vec![
                event(Some("message_start"), "{\"a\":1}"),
                event(None, "first\nsecond"),
                event(None, "[DONE]"),
            ]
        );
    }

    #[test]
    fn line_ends_and_chunk_boundaries() {
        let events = parse(&[
            b"data: a\r",
            b"\ndata: b\r\n\r",
            b"\n",
            b"data: c\rdata: d\r\r",
            b"da",
            b"ta: e",
            b"\n\n",
        ])
        .expect("parse");
        assert_eq!(
            events,
            vec![event(None, "a\nb"), event(None, "c\nd"), event(None, "e")]
        );
    }

    #[test]
    fn unterminated_event_at_the_end_is_not_dispatched() {
        let events = parse(&[b"data: complete\n\ndata: cut off"]).expect("parse");
        assert_eq!(events, vec![event(None, "complete")]);
    }

    #[test]
    fn limits_and_encoding() {
        let mut parser = SseParser::new(16);
        assert!(matches!(
            parser.feed(&[b'x'; 40]),
            Err(AiError::Protocol(_))
        ));
        let mut parser = SseParser::new(16);
        assert!(matches!(
            parser.feed(b"data: 0123456789\ndata: 0123456789\n"),
            Err(AiError::Protocol(_))
        ));
        let mut parser = SseParser::new(64);
        assert!(matches!(
            parser.feed(b"data: \xff\xfe\n\n"),
            Err(AiError::Protocol(_))
        ));
    }
}
