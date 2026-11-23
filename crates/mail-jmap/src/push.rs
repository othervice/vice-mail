//! Push over EventSource (RFC 8620 section 7.3, the `text/event-stream` format of the HTML
//! standard): a small parser for the stream and the state changes it carries.

use serde_json::Value;

/// One dispatched event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Event {
    /// `event:` field; `message` when absent.
    pub kind: String,
    /// `data:` lines joined with line feeds.
    pub data: String,
}

/// Longest event kept; a server that never ends an event cannot grow the buffer further.
const MAX_EVENT: usize = 1024 * 1024;

/// Collects bytes and yields complete events.
#[derive(Debug, Default)]
pub(crate) struct Parser {
    buffer: Vec<u8>,
    kind: Option<String>,
    data: Vec<String>,
}

impl Parser {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            if line.is_empty() {
                if !self.data.is_empty() || self.kind.is_some() {
                    events.push(Event {
                        kind: self.kind.take().unwrap_or_else(|| "message".into()),
                        data: std::mem::take(&mut self.data).join("\n"),
                    });
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = match line.split_once(':') {
                Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
                None => (line.as_str(), ""),
            };
            match field {
                "event" => self.kind = Some(value.to_owned()),
                "data" => self.data.push(value.to_owned()),
                _ => {}
            }
        }
        if self.buffer.len() > MAX_EVENT {
            self.buffer.clear();
            self.data.clear();
            self.kind = None;
        }
        events
    }
}

/// The new Email state of `account` in a `state` event, if the event names one.
pub(crate) fn email_state(event: &Event, account: &str) -> Option<String> {
    if event.kind != "state" {
        return None;
    }
    let value: Value = serde_json::from_str(&event.data).ok()?;
    value
        .get("changed")?
        .get(account)?
        .get("Email")?
        .as_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalwart_s_events_parse_in_any_split() {
        // As Stalwart 1.1.1 sends them (recorded).
        let stream = "event: state\ndata: {\"@type\":\"StateChange\",\"changed\":{\"b\":{\"Thread\":\"s7qaq\",\"Mailbox\":\"s7qaq\",\"EmailDelivery\":\"s7qaq\",\"Email\":\"s7qaq\"}}}\n\n\
                      event: state\ndata: {\"@type\":\"StateChange\",\"changed\":{\"b\":{\"EmailSubmission\":\"sae\"}}}\n\n\
                      : comment\r\nevent: ping\r\ndata: {\"interval\":15}\r\n\r\n";
        for split in [1, 7, 50, stream.len()] {
            let mut parser = Parser::default();
            let mut events = Vec::new();
            for chunk in stream.as_bytes().chunks(split) {
                events.extend(parser.push(chunk));
            }
            assert_eq!(events.len(), 3, "split {split}");
            assert_eq!(email_state(&events[0], "b").as_deref(), Some("s7qaq"));
            assert_eq!(email_state(&events[0], "c"), None, "another account");
            assert_eq!(email_state(&events[1], "b"), None, "no Email change");
            assert_eq!(events[2].kind, "ping");
            assert_eq!(email_state(&events[2], "b"), None);
        }
        let mut parser = Parser::default();
        let events = parser.push(b"data: a\ndata: b\n\n");
        assert_eq!(
            events,
            [Event {
                kind: "message".into(),
                data: "a\nb".into()
            }]
        );
        assert!(parser.push(&vec![b'x'; MAX_EVENT + 1]).is_empty());
        assert!(parser.buffer.is_empty(), "an endless line is dropped");
    }
}
