//! Records the plaintext bytes the adapter writes (inside TLS), for tests that must prove what
//! did or did not leave the adapter. Only compiled for tests and the `test-support` feature.
//!
//! A trigger ([`WireTap::on_write`]) runs a callback the moment a given text is written, for
//! example to cut the connection right after a command (stage 4 fault injection).

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

type Callback = Box<dyn Fn() + Send + Sync>;

struct Trigger {
    pattern: Vec<u8>,
    callback: Callback,
}

#[derive(Default)]
struct Inner {
    bytes: Mutex<Vec<u8>>,
    triggers: Mutex<Vec<Trigger>>,
}

/// Shared recorder of everything the client wrote, in order. Clone it into the connector and
/// keep a copy to inspect it.
#[derive(Clone, Default)]
pub struct WireTap(Arc<Inner>);

impl fmt::Debug for WireTap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WireTap")
            .field("bytes", &self.len())
            .finish_non_exhaustive()
    }
}

impl WireTap {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Vec<u8>> {
        // A panic while holding the lock cannot leave the byte buffer inconsistent.
        self.0.bytes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Calls `callback` once, as soon as `pattern` is written (it may span several writes). The
    /// callback runs inside the write, so it must not block: set a flag, notify, cut a proxy.
    pub fn on_write(&self, pattern: &str, callback: impl Fn() + Send + Sync + 'static) {
        self.0
            .triggers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Trigger {
                pattern: pattern.as_bytes().to_vec(),
                callback: Box::new(callback),
            });
    }

    pub(crate) fn record(&self, bytes: &[u8]) {
        let fired = {
            let mut all = self.lock();
            let before = all.len();
            all.extend_from_slice(bytes);
            let mut triggers = self
                .0
                .triggers
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut fired = Vec::new();
            let mut i = 0;
            while i < triggers.len() {
                let pattern_len = triggers.get(i).map_or(0, |t| t.pattern.len());
                let start = before.saturating_sub(pattern_len);
                let tail = all.get(start..).unwrap_or_default();
                let hit = triggers.get(i).is_some_and(|t| {
                    !t.pattern.is_empty() && tail.windows(t.pattern.len()).any(|w| w == t.pattern)
                });
                if hit {
                    fired.push(triggers.remove(i));
                } else {
                    i += 1;
                }
            }
            fired
        };
        for trigger in fired {
            (trigger.callback)();
        }
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

    #[test]
    fn triggers_fire_once_also_across_writes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let tap = WireTap::new();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        tap.on_write("UID MOVE", move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        tap.record(b"A1 NOOP\r\nA2 UID MO");
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        tap.record(b"VE 3 \"Archive\"\r\n");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        tap.record(b"A3 UID MOVE 4 \"Archive\"\r\n");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "a trigger fires once");
    }
}
