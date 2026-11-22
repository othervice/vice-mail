//! The adapter's own command layer (decision 0002).
//!
//! async-imap's stream commands ignore the tagged status (issue #151) and swallow response
//! codes, so every command after LOGIN goes through [`Conn::run`]: it checks the read-only
//! allowlist, sends the line, hands each untagged response to the caller while it is read,
//! records unsolicited mailbox changes and returns the tagged status with its text.
//!
//! **Read-only allowlist** ([`permit`]): with [`Access::ReadOnly`] only the commands listed in
//! the `MailBackend` docs pass; everything else fails with [`BackendError::ReadOnly`] before a
//! byte is written. It classifies the exact line that would be sent, so it holds for every way
//! a command can be built. LOGIN and STARTTLS, which async-imap sends itself, are checked here
//! too before the library call.
//!
//! **Broken connections:** a command that fails on the transport, or whose future is dropped
//! half-way, leaves the connection unusable ("poisoned"): every later call fails with
//! [`BackendError::ConnectionLost`] without touching the socket. A refusal (NO/BAD) ends the
//! command cleanly and keeps the connection.
//!
//! **Unsolicited responses** (EXISTS, EXPUNGE, FETCH, VANISHED, BYE) are seen here as they are
//! read, never parked in async-imap's bounded channel (which drops entries beyond 100), and
//! recorded in [`Watch`]; `idle` reports them.
//!
//! **IDLE** is done here rather than with async-imap's `Handle`: 0.12 can panic in
//! `Handle::init` on a BAD without text, reports a closed connection as a manual interrupt and
//! consumes the session.

use std::fmt;
use std::future::Future;
use std::io;
use std::time::Duration;

use async_imap::Session;
use async_imap::imap_proto::{MailboxDatum, Outcome, ResponseCode, Status};
use mail_model::{Access, BackendError, IdleOutcome};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

pub(crate) use async_imap::imap_proto::Response;

use crate::error::{from_imap, from_io, short};
use crate::timeout_io::{IoTimeout, TimeoutHandle};

/// A byte stream an IMAP connection can run on (TLS in the product, an in-memory pipe in unit
/// tests).
pub trait Transport: AsyncRead + AsyncWrite + Unpin + fmt::Debug + Send + 'static {}

impl<T> Transport for T where T: AsyncRead + AsyncWrite + Unpin + fmt::Debug + Send + 'static {}

// ---------------------------------------------------------------------------------------------
// Read-only allowlist

/// Checks `line` (a command without its tag, or `DONE`) against the access mode. In read-only
/// mode only the allowlist passes; the error names the refused command.
pub(crate) fn permit(access: Access, line: &str) -> Result<(), BackendError> {
    match access {
        Access::ReadWrite => Ok(()),
        Access::ReadOnly => read_only_check(line).map_err(BackendError::ReadOnly),
    }
}

/// Commands that never change anything on the server. FETCH is checked further.
const READ_ONLY_COMMANDS: &[&str] = &[
    "CAPABILITY",
    "NOOP",
    "LOGOUT",
    "STARTTLS",
    "LOGIN",
    "AUTHENTICATE",
    "ID",
    "ENABLE",
    "LIST",
    "STATUS",
    "EXAMINE",
    "SEARCH",
    "IDLE",
    "DONE",
];

fn read_only_check(line: &str) -> Result<(), String> {
    let mut words = line.split_ascii_whitespace();
    let first = words.next().unwrap_or_default().to_ascii_uppercase();
    let (name, verb, uid) = if first == "UID" {
        let verb = words.next().unwrap_or_default().to_ascii_uppercase();
        (format!("UID {verb}").trim_end().to_owned(), verb, true)
    } else {
        (first.clone(), first, false)
    };
    let allowed = if uid {
        verb == "SEARCH" || verb == "FETCH"
    } else {
        verb == "FETCH" || READ_ONLY_COMMANDS.contains(&verb.as_str())
    };
    if !allowed {
        return Err(name);
    }
    // One command per line: a line break could smuggle a second command past the check.
    if line.contains(['\r', '\n']) {
        return Err(format!("{name} (multi-line)"));
    }
    if verb == "FETCH" {
        fetch_sets_no_flag(line).map_err(|item| format!("{name} {item}"))?;
    }
    if verb == "DONE" && words.next().is_some() {
        return Err(name);
    }
    Ok(())
}

/// FETCH items that set `\Seen` (RFC 3501): `BODY[...]`, `BINARY[...]`, `RFC822`, `RFC822.TEXT`.
/// Their `.PEEK` forms, `RFC822.HEADER` and `RFC822.SIZE` do not. False alarms only refuse more.
fn fetch_sets_no_flag(line: &str) -> Result<(), String> {
    let upper = line.to_ascii_uppercase();
    for item in ["BODY[", "BINARY["] {
        if upper.contains(item) {
            return Err(item.to_owned());
        }
    }
    let atom = |c: char| !(c.is_ascii_alphanumeric() || c == '.');
    match upper
        .split(atom)
        .find(|token| *token == "RFC822" || *token == "RFC822.TEXT")
    {
        Some(token) => Err(token.to_owned()),
        None => Ok(()),
    }
}

fn command_name(line: &str) -> String {
    let mut words = line.split_ascii_whitespace();
    match (words.next(), words.next()) {
        (Some(uid), Some(verb)) if uid.eq_ignore_ascii_case("UID") => {
            format!("UID {}", verb.to_ascii_uppercase())
        }
        (Some(verb), _) => verb.to_ascii_uppercase(),
        (None, _) => String::new(),
    }
}

#[cfg(any(test, feature = "test-support"))]
/// Whether `word` is one of async-imap's tags (`A0001`).
pub(crate) fn looks_like_tag(word: &str) -> bool {
    word.len() >= 2 && word.starts_with('A') && word.bytes().skip(1).all(|b| b.is_ascii_digit())
}

/// A string argument (mailbox name) as an IMAP quoted string. Modified UTF-7 names are 7-bit;
/// anything that would need a literal is refused.
pub(crate) fn quote(value: &str) -> Result<String, BackendError> {
    if value
        .chars()
        .any(|c| !c.is_ascii() || matches!(c, '\r' | '\n' | '\0'))
    {
        return Err(BackendError::Unsupported(format!(
            "mailbox names with 8-bit or control characters ({})",
            short(value)
        )));
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

// ---------------------------------------------------------------------------------------------
// Tagged status

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaggedStatus {
    Ok,
    No,
    Bad,
}

/// The tagged response that completed a command.
#[derive(Debug, Clone)]
pub(crate) struct Tagged {
    pub(crate) status: TaggedStatus,
    /// `[READ-ONLY]` code (EXAMINE/SELECT).
    pub(crate) read_only: bool,
    /// Response code and text, shortened, for error details.
    pub(crate) text: String,
}

impl Tagged {
    fn new(status: &Status, outcome: &Outcome<'_>) -> Self {
        let status = match status {
            Status::Ok => TaggedStatus::Ok,
            Status::No => TaggedStatus::No,
            // PREAUTH and BYE cannot complete a command; treat them as a refusal.
            Status::Bad | Status::PreAuth | Status::Bye => TaggedStatus::Bad,
        };
        let info = outcome.information.as_deref().unwrap_or_default();
        let text = match &outcome.code {
            Some(code) => format!("[{}] {info}", code_name(code)),
            None => info.to_owned(),
        };
        Self {
            status,
            read_only: matches!(outcome.code, Some(ResponseCode::ReadOnly)),
            text: short(&text),
        }
    }

    /// The tagged response if it is OK, otherwise [`BackendError::Rejected`] naming `what`.
    pub(crate) fn check(self, what: &str) -> Result<Self, BackendError> {
        match self.status {
            TaggedStatus::Ok => Ok(self),
            TaggedStatus::No => Err(BackendError::Rejected(format!("{what}: NO {}", self.text))),
            TaggedStatus::Bad => Err(BackendError::Rejected(format!("{what}: BAD {}", self.text))),
        }
    }

    /// The response code at the start of the text, upper case (`[NONEXISTENT] ...` gives
    /// `NONEXISTENT`; imap-proto leaves codes it does not know in the text).
    pub(crate) fn bracket_code(&self) -> Option<String> {
        let rest = self.text.strip_prefix('[')?;
        let end = rest.find([']', ' '])?;
        Some(rest[..end].to_ascii_uppercase())
    }
}

fn code_name(code: &ResponseCode<'_>) -> String {
    match code {
        ResponseCode::ReadOnly => "READ-ONLY".to_owned(),
        ResponseCode::ReadWrite => "READ-WRITE".to_owned(),
        ResponseCode::TryCreate => "TRYCREATE".to_owned(),
        ResponseCode::Alert => "ALERT".to_owned(),
        ResponseCode::Parse => "PARSE".to_owned(),
        other => {
            let debug = format!("{other:?}");
            short(&debug.chars().take(40).collect::<String>())
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Connection

/// Unsolicited changes to the open mailbox, noticed while reading responses.
#[derive(Debug, Default)]
pub(crate) struct Watch {
    /// Last known EXISTS of the open mailbox.
    pub(crate) exists: Option<u32>,
    /// EXISTS, EXPUNGE, FETCH or VANISHED arrived that the caller did not ask for.
    pub(crate) changed: bool,
    /// Text of a `* BYE` (the server is about to close the connection).
    bye: Option<String>,
}

impl Watch {
    pub(crate) fn reset(&mut self, exists: u32) {
        self.exists = Some(exists);
        self.changed = false;
    }

    fn note(&mut self, resp: &Response<'_>) {
        match resp {
            Response::MailboxData(MailboxDatum::Exists(n)) => {
                if self.exists != Some(*n) {
                    self.changed = true;
                }
                self.exists = Some(*n);
            }
            Response::Expunge(_) => {
                self.changed = true;
                self.exists = self.exists.map(|n| n.saturating_sub(1));
            }
            Response::Vanished {
                earlier: false,
                uids,
            } => {
                self.changed = true;
                let gone = uids.iter().fold(0u32, |sum, range| {
                    let count = range.end().saturating_sub(*range.start()).saturating_add(1);
                    sum.saturating_add(count)
                });
                self.exists = self.exists.map(|n| n.saturating_sub(gone));
            }
            Response::Fetch(..) => self.changed = true,
            Response::Data {
                status: Status::Bye,
                outcome,
            } => {
                self.bye = Some(short(outcome.information.as_deref().unwrap_or_default()));
            }
            _ => {}
        }
    }

    fn closed(&self) -> BackendError {
        BackendError::ConnectionLost(match &self.bye {
            Some(text) => format!("server said BYE: {text}"),
            None => "server closed the connection".into(),
        })
    }
}

/// The next response, or why there is none.
fn got<T>(read: io::Result<Option<T>>, watch: &Watch) -> Result<T, BackendError> {
    match read {
        Ok(Some(resp)) => Ok(resp),
        Ok(None) => Err(watch.closed()),
        Err(err) => Err(from_io(&err)),
    }
}

/// Errors after which the connection is still in a clean state (the command completed).
fn leaves_connection_usable(err: &BackendError) -> bool {
    matches!(
        err,
        BackendError::Rejected(_)
            | BackendError::ReadOnly(_)
            | BackendError::InvalidState(_)
            | BackendError::Unsupported(_)
            | BackendError::MailboxNotFound(_)
    )
}

enum Wake<R> {
    Stop,
    Timeout,
    Read(R),
}

/// One authenticated connection with the command layer on top.
pub(crate) struct Conn<S: Transport> {
    session: Session<IoTimeout<S>>,
    timeouts: TimeoutHandle,
    read_limit: Duration,
    access: Access,
    /// Why the connection is unusable; set while a command runs and cleared when it ends cleanly.
    poisoned: Option<String>,
    pub(crate) watch: Watch,
}

impl<S: Transport> Conn<S> {
    pub(crate) fn new(
        session: Session<IoTimeout<S>>,
        timeouts: TimeoutHandle,
        access: Access,
    ) -> Self {
        let read_limit = timeouts.read();
        Self {
            session,
            timeouts,
            read_limit,
            access,
            poisoned: None,
            watch: Watch::default(),
        }
    }

    fn usable(&self) -> Result<(), BackendError> {
        match &self.poisoned {
            None => Ok(()),
            Some(reason) => Err(BackendError::ConnectionLost(format!(
                "connection unusable after an earlier failure ({reason})"
            ))),
        }
    }

    fn settle<T>(&mut self, result: &Result<T, BackendError>) {
        self.poisoned = match result {
            Ok(_) => None,
            Err(err) if leaves_connection_usable(err) => None,
            Err(err) => Some(err.to_string()),
        };
    }

    /// Runs one command. `on_untagged` sees every untagged response and returns `true` if it
    /// took it as part of the answer; the rest is recorded in [`Conn::watch`]. NO and BAD are
    /// returned as a [`Tagged`] status, not as an error.
    pub(crate) async fn run<F>(
        &mut self,
        line: &str,
        mut on_untagged: F,
    ) -> Result<Tagged, BackendError>
    where
        F: FnMut(&Response<'_>) -> bool + Send,
    {
        self.usable()?;
        permit(self.access, line)?;
        self.poisoned = Some(format!("{} was aborted", command_name(line)));
        let result = self.exchange(line, &mut on_untagged).await;
        self.settle(&result);
        result
    }

    async fn exchange<F>(&mut self, line: &str, on_untagged: &mut F) -> Result<Tagged, BackendError>
    where
        F: FnMut(&Response<'_>) -> bool + Send,
    {
        let tag = self.session.run_command(line).await.map_err(from_imap)?;
        loop {
            let resp = got(self.session.read_response().await, &self.watch)?;
            match resp.parsed() {
                Response::Done {
                    tag: done,
                    status,
                    outcome,
                } if *done == tag => return Ok(Tagged::new(status, outcome)),
                Response::Continue(_) => {
                    return Err(BackendError::Protocol(format!(
                        "unexpected continuation request during {}",
                        command_name(line)
                    )));
                }
                other => {
                    if !on_untagged(other) {
                        self.watch.note(other);
                    }
                }
            }
        }
    }

    /// IDLE until a change arrives, `max` passes or `stop` completes (see
    /// [`mail_model::MailBackend::idle`]). Pending changes must be checked by the caller.
    pub(crate) async fn idle<St>(
        &mut self,
        max: Duration,
        stop: St,
    ) -> Result<IdleOutcome, BackendError>
    where
        St: Future<Output = ()> + Send,
    {
        self.usable()?;
        permit(self.access, "IDLE")?;
        permit(self.access, "DONE")?;
        self.poisoned = Some("IDLE was aborted".into());
        let result = self.idle_exchange(max, stop).await;
        self.timeouts.set_read(self.read_limit);
        self.settle(&result);
        result
    }

    async fn idle_exchange<St>(
        &mut self,
        max: Duration,
        stop: St,
    ) -> Result<IdleOutcome, BackendError>
    where
        St: Future<Output = ()> + Send,
    {
        let tag = self.session.run_command("IDLE").await.map_err(from_imap)?;
        loop {
            let resp = got(self.session.read_response().await, &self.watch)?;
            match resp.parsed() {
                Response::Continue(_) => break,
                Response::Done {
                    tag: done,
                    status,
                    outcome,
                } if *done == tag => {
                    // Refused (or ended at once): the command is complete.
                    Tagged::new(status, outcome).check("IDLE")?;
                    return Ok(self.idle_result(IdleOutcome::TimedOut));
                }
                other => self.watch.note(other),
            }
        }

        // Waiting: the read limit is off, `max` is the limit. A silent connection is noticed
        // at the latest after `max` plus the read limit (DONE below).
        self.timeouts.set_read(Duration::ZERO);
        let deadline = tokio::time::Instant::now() + max;
        let mut stop = std::pin::pin!(stop);
        let waited = loop {
            if self.watch.changed {
                break IdleOutcome::Changed;
            }
            let wake = tokio::select! {
                biased;
                () = &mut stop => Wake::Stop,
                () = tokio::time::sleep_until(deadline) => Wake::Timeout,
                read = self.session.read_response() => Wake::Read(read),
            };
            match wake {
                Wake::Stop => break IdleOutcome::Stopped,
                Wake::Timeout => break IdleOutcome::TimedOut,
                Wake::Read(read) => {
                    let resp = got(read, &self.watch)?;
                    match resp.parsed() {
                        Response::Done {
                            tag: done,
                            status,
                            outcome,
                        } if *done == tag => {
                            // The server ended IDLE itself; no DONE needed.
                            self.timeouts.set_read(self.read_limit);
                            Tagged::new(status, outcome).check("IDLE")?;
                            return Ok(self.idle_result(IdleOutcome::TimedOut));
                        }
                        other => self.watch.note(other),
                    }
                }
            }
        };

        self.timeouts.set_read(self.read_limit);
        self.session
            .run_command_untagged("DONE")
            .await
            .map_err(from_imap)?;
        loop {
            let resp = got(self.session.read_response().await, &self.watch)?;
            match resp.parsed() {
                Response::Done {
                    tag: done,
                    status,
                    outcome,
                } if *done == tag => {
                    Tagged::new(status, outcome).check("IDLE")?;
                    return Ok(self.idle_result(waited));
                }
                other => self.watch.note(other),
            }
        }
    }

    /// Final outcome: changes that arrived (also while ending IDLE) win over a timeout; a stop
    /// request is reported as such and the changes stay pending for the next call.
    fn idle_result(&mut self, outcome: IdleOutcome) -> IdleOutcome {
        match outcome {
            IdleOutcome::Stopped => IdleOutcome::Stopped,
            _ if self.watch.changed => {
                self.watch.changed = false;
                IdleOutcome::Changed
            }
            other => other,
        }
    }

    /// LOGOUT, then close TLS. EOF after BYE is the expected end.
    pub(crate) async fn logout(mut self) -> Result<(), BackendError> {
        self.usable()?;
        permit(self.access, "LOGOUT")?;
        let tag = self
            .session
            .run_command("LOGOUT")
            .await
            .map_err(from_imap)?;
        loop {
            match self.session.read_response().await {
                Ok(Some(resp)) => match resp.parsed() {
                    Response::Done { tag: done, .. } if *done == tag => break,
                    other => self.watch.note(other),
                },
                Ok(None) => break,
                Err(_) if self.watch.bye.is_some() => break,
                Err(err) => return Err(from_io(&err)),
            }
        }
        if let Err(err) = AsMut::<IoTimeout<S>>::as_mut(&mut self.session)
            .shutdown()
            .await
        {
            tracing::debug!(error = %err, "closing the connection after LOGOUT failed");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_lets_reads_through_and_names_refused_writes() {
        let allowed = [
            "CAPABILITY",
            "NOOP",
            "LOGOUT",
            "STARTTLS",
            "LOGIN",
            "ID NIL",
            "ENABLE CONDSTORE QRESYNC",
            "LIST \"\" \"*\" RETURN (SPECIAL-USE STATUS (MESSAGES UNSEEN))",
            "STATUS \"Sent\" (MESSAGES UIDNEXT)",
            "EXAMINE \"INBOX\" (CONDSTORE)",
            "UID SEARCH ALL",
            "search all",
            "UID FETCH 1:5 (UID FLAGS RFC822.SIZE ENVELOPE BODYSTRUCTURE BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)])",
            "UID FETCH 1:* (UID FLAGS MODSEQ) (CHANGEDSINCE 7)",
            "FETCH * (UID)",
            "UID FETCH 1 (RFC822.HEADER RFC822.SIZE BINARY.PEEK[1] BINARY.SIZE[1])",
            "IDLE",
            "DONE",
        ];
        for line in allowed {
            assert_eq!(permit(Access::ReadOnly, line), Ok(()), "{line}");
        }
        let refused = [
            ("SELECT \"INBOX\"", "SELECT"),
            ("UID STORE 1 +FLAGS (\\Seen)", "UID STORE"),
            ("STORE 1 +FLAGS (\\Seen)", "STORE"),
            ("APPEND \"Sent\" {10+}", "APPEND"),
            ("UID COPY 1 \"Trash\"", "UID COPY"),
            ("COPY 1 \"Trash\"", "COPY"),
            ("UID MOVE 1 \"Trash\"", "UID MOVE"),
            ("MOVE 1 \"Trash\"", "MOVE"),
            ("EXPUNGE", "EXPUNGE"),
            ("UID EXPUNGE 1", "UID EXPUNGE"),
            ("CLOSE", "CLOSE"),
            ("UNSELECT", "UNSELECT"),
            ("CREATE \"x\"", "CREATE"),
            ("DELETE \"x\"", "DELETE"),
            ("RENAME \"x\" \"y\"", "RENAME"),
            ("SUBSCRIBE \"x\"", "SUBSCRIBE"),
            ("UNSUBSCRIBE \"x\"", "UNSUBSCRIBE"),
            ("SETMETADATA \"x\" (/private/comment \"y\")", "SETMETADATA"),
            ("SETACL \"x\" bob lr", "SETACL"),
            ("SETQUOTA \"\" (STORAGE 1)", "SETQUOTA"),
            ("UID FETCH 1 BODY[]", "UID FETCH BODY["),
            ("UID FETCH 1 (BODY[TEXT])", "UID FETCH BODY["),
            ("FETCH 1 (RFC822)", "FETCH RFC822"),
            ("UID FETCH 1 RFC822.TEXT", "UID FETCH RFC822.TEXT"),
            ("UID FETCH 1 (BINARY[1])", "UID FETCH BINARY["),
            ("UID SELECT x", "UID SELECT"),
            ("UID", "UID"),
            ("XUNKNOWN", "XUNKNOWN"),
            ("", ""),
            (
                "NOOP\r\nA0009 UID STORE 1 +FLAGS (\\Deleted)",
                "NOOP (multi-line)",
            ),
            ("DONE extra", "DONE"),
        ];
        for (line, name) in refused {
            assert_eq!(
                permit(Access::ReadOnly, line),
                Err(BackendError::ReadOnly(name.to_owned())),
                "{line:?}"
            );
            assert_eq!(permit(Access::ReadWrite, line), Ok(()), "{line:?}");
        }
    }

    #[test]
    fn quoting() {
        assert_eq!(quote("INBOX").as_deref(), Ok("\"INBOX\""));
        assert_eq!(quote("a\"b\\c").as_deref(), Ok("\"a\\\"b\\\\c\""));
        assert_eq!(
            quote("Projects/&ANw-berpr&APw-fung").as_deref(),
            Ok("\"Projects/&ANw-berpr&APw-fung\"")
        );
        assert!(matches!(
            quote("Überprüfung"),
            Err(BackendError::Unsupported(_))
        ));
        assert!(matches!(quote("a\r\nb"), Err(BackendError::Unsupported(_))));
    }

    #[test]
    fn watch_tracks_exists_across_expunge_and_vanished() {
        let parse = |line: &'static [u8]| match Response::parse(line) {
            Ok((_, resp)) => resp,
            Err(err) => panic!("{err:?}"),
        };
        let mut watch = Watch::default();
        watch.reset(10);
        watch.note(&parse(b"* 10 EXISTS\r\n"));
        assert!(!watch.changed, "a repeated EXISTS is no change");
        watch.note(&parse(b"* VANISHED 3:4,7\r\n"));
        assert!(watch.changed);
        assert_eq!(watch.exists, Some(7));
        watch.note(&parse(b"* 2 EXPUNGE\r\n"));
        assert_eq!(watch.exists, Some(6));
        // A new message after the deletions brings EXISTS back to an old number: still a change.
        watch.changed = false;
        watch.note(&parse(b"* 7 EXISTS\r\n"));
        assert!(watch.changed);
        // VANISHED (EARLIER) answers a QRESYNC request and is no news.
        watch.changed = false;
        watch.note(&parse(b"* VANISHED (EARLIER) 1:2\r\n"));
        assert!(!watch.changed);
        watch.note(&parse(b"* BYE going down\r\n"));
        assert!(
            matches!(watch.closed(), BackendError::ConnectionLost(t) if t.contains("going down"))
        );
    }

    #[test]
    fn tags() {
        assert!(looks_like_tag("A0001") && looks_like_tag("A12"));
        assert!(!looks_like_tag("A") && !looks_like_tag("DONE") && !looks_like_tag("a1x"));
    }
}
