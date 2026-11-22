//! Records the plaintext bytes the adapter writes (inside TLS), for tests that must prove what
//! did or did not leave the adapter. Only compiled for tests and the `test-support` feature.

use std::sync::{Arc, Mutex, MutexGuard};

/// Shared recorder of everything the client wrote, in order. Clone it into the connector and
/// keep a copy to inspect it.
#[derive(Debug, Clone, Default)]
pub struct WireTap(Arc<Mutex<Vec<u8>>>);

impl WireTap {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Vec<u8>> {
        // A panic while holding the lock cannot leave the byte buffer inconsistent.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn record(&self, bytes: &[u8]) {
        self.lock().extend_from_slice(bytes);
    }

    /// Every byte written so far, including the LOGIN line (test credentials only).
    pub fn bytes(&self) -> Vec<u8> {
        self.lock().clone()
    }

    /// Number of bytes written so far.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// The written lines (split at CRLF), with the credentials of LOGIN and AUTHENTICATE
    /// (initial response or the line answering the server's challenge) replaced by
    /// `[redacted]`, so they can be printed in test output.
    pub fn lines(&self) -> Vec<String> {
        let bytes = self.bytes();
        let mut challenge_answer = false;
        String::from_utf8_lossy(&bytes)
            .split("\r\n")
            .filter(|line| !line.is_empty())
            .map(|line| {
                if std::mem::take(&mut challenge_answer) {
                    return "[redacted]".to_owned();
                }
                let (redacted, awaits_answer) = redact_credentials(line);
                challenge_answer = awaits_answer;
                redacted
            })
            .collect()
    }

    /// Command names (`UID FETCH`, `EXAMINE`, `DONE`, ...) of the written lines, upper case.
    /// Lines that continue a literal are included as whatever their first words are.
    pub fn verbs(&self) -> Vec<String> {
        self.lines().iter().map(|line| verb_of(line)).collect()
    }
}

/// The line with credentials removed, and whether the next line answers a SASL challenge.
fn redact_credentials(line: &str) -> (String, bool) {
    let words: Vec<&str> = line.split(' ').collect();
    match words.as_slice() {
        [tag, verb, ..] if verb.eq_ignore_ascii_case("LOGIN") => {
            (format!("{tag} LOGIN [redacted]"), false)
        }
        [tag, verb, mechanism] if verb.eq_ignore_ascii_case("AUTHENTICATE") => {
            (format!("{tag} AUTHENTICATE {mechanism}"), true)
        }
        [tag, verb, mechanism, ..] if verb.eq_ignore_ascii_case("AUTHENTICATE") => {
            (format!("{tag} AUTHENTICATE {mechanism} [redacted]"), false)
        }
        _ => (line.to_owned(), false),
    }
}

/// `A0003 UID FETCH 1:5 (FLAGS)` → `UID FETCH`; `DONE` → `DONE`.
fn verb_of(line: &str) -> String {
    let words: Vec<&str> = line.split(' ').collect();
    // Untagged lines (DONE, literal data) have no tag in front.
    let rest = match words.as_slice() {
        [single] => return single.to_ascii_uppercase(),
        [first, rest @ ..] if crate::command::looks_like_tag(first) => rest,
        all => all,
    };
    match rest {
        [uid, verb, ..] if uid.eq_ignore_ascii_case("UID") => {
            format!("UID {}", verb.to_ascii_uppercase())
        }
        [verb, ..] => verb.to_ascii_uppercase(),
        [] => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_redact_login_and_verbs_name_commands() {
        let tap = WireTap::new();
        tap.record(b"A0001 LOGIN \"alice@x\" \"secret\"\r\nA0002 UID FETCH 1:2 (FLAGS)\r\n");
        tap.record(b"A0003 IDLE\r\nDONE\r\n");
        tap.record(b"A0004 AUTHENTICATE PLAIN\r\nAGFsaWNlAHNlY3JldA==\r\n");
        tap.record(b"A0005 AUTHENTICATE PLAIN AGFsaWNlAHNlY3JldA==\r\nA0006 NOOP\r\n");
        let lines = tap.lines();
        assert_eq!(lines[0], "A0001 LOGIN [redacted]");
        assert!(lines.iter().all(|l| !l.contains("secret")));
        assert_eq!(
            lines[4..7],
            [
                "A0004 AUTHENTICATE PLAIN",
                "[redacted]",
                "A0005 AUTHENTICATE PLAIN [redacted]"
            ]
        );
        assert!(lines.iter().all(|l| !l.contains("AGFsaWNl")), "{lines:?}");
        assert_eq!(
            tap.verbs(),
            [
                "LOGIN",
                "UID FETCH",
                "IDLE",
                "DONE",
                "AUTHENTICATE",
                "[REDACTED]",
                "AUTHENTICATE",
                "NOOP"
            ]
        );
        assert_eq!(tap.len(), tap.bytes().len());
    }
}
