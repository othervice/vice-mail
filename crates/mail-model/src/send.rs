//! Sending (stage 5): the SMTP envelope and what the server answered to a submission.
//!
//! Internal types: the SMTP adapter returns them, the engine turns them into the state of the
//! outbox operation `send`. Nothing here crosses the IPC bridge; for the UI the engine maps a
//! refusal or an unclear outcome onto [`BackendError::SendRejected`] or
//! [`BackendError::SendUnclear`].

use std::fmt;

use crate::BackendError;

/// `MAIL FROM` and `RCPT TO` of one submission. Blind-copy recipients are listed here and appear
/// nowhere in the message that is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// Address of the sending identity.
    pub from: String,
    /// Every recipient (To, Cc and Bcc), each once.
    pub recipients: Vec<String>,
}

/// One reply of an SMTP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpReply {
    /// Three-digit reply code (`250`, `451`, `550`, ...).
    pub code: u16,
    /// Enhanced status code (RFC 3463) such as `5.1.1`, when the server sent one.
    pub enhanced: Option<String>,
    /// Text after the codes, lines joined with spaces, control characters removed, shortened.
    /// May name addresses; never contains credentials.
    pub text: String,
}

impl SmtpReply {
    /// A 5xx reply: trying again unchanged fails again.
    pub fn is_permanent(&self) -> bool {
        self.code >= 500
    }
}

impl fmt::Display for SmtpReply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)?;
        if let Some(enhanced) = &self.enhanced {
            write!(f, " {enhanced}")?;
        }
        if !self.text.is_empty() {
            write!(f, " {}", self.text)?;
        }
        Ok(())
    }
}

/// The step of the SMTP transaction a refusal answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmtpStep {
    /// `MAIL FROM` (sender not allowed, message too large for the declared size, ...).
    MailFrom,
    /// One or more `RCPT TO`.
    Recipients,
    /// The `DATA` command itself.
    Data,
    /// The end of the message data (content refused, too large, policy).
    Message,
}

/// A recipient the server refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientRefusal {
    pub address: String,
    pub reply: SmtpReply,
}

/// The server refused the submission. Nothing was delivered to anyone: a refused recipient
/// abandons the whole transaction, so the user can correct it and send once more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendRejection {
    pub step: SmtpStep,
    /// The refusing reply; for [`SmtpStep::Recipients`] the first refused recipient's.
    pub reply: SmtpReply,
    /// Every refused recipient, in envelope order (only for [`SmtpStep::Recipients`]).
    pub recipients: Vec<RecipientRefusal>,
}

impl SendRejection {
    /// A refusal that fails again unchanged (any 5xx reply). Otherwise (4xx only) the engine
    /// may try again later.
    pub fn is_permanent(&self) -> bool {
        self.reply.is_permanent() || self.recipients.iter().any(|r| r.reply.is_permanent())
    }

    /// The refusal as an error for the UI ([`crate::ErrorCode::SendRejected`]).
    pub fn to_error(&self) -> BackendError {
        let detail = match self.step {
            SmtpStep::Recipients if !self.recipients.is_empty() => self
                .recipients
                .iter()
                .map(|r| format!("{}: {}", r.address, r.reply))
                .collect::<Vec<_>>()
                .join("; "),
            _ => self.reply.to_string(),
        };
        BackendError::SendRejected {
            permanent: self.is_permanent(),
            detail,
        }
    }
}

/// What became of a message handed to the SMTP server. Connection, TLS and login failures are
/// errors of the send call instead ([`BackendError`]); they all mean "nothing was transmitted".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// The server confirmed the end of the data (2xx): it took over delivery.
    Accepted(SmtpReply),
    /// The server refused; nothing was delivered.
    Rejected(SendRejection),
    /// The whole message, end-of-data marker included, went out, but the connection ended
    /// before the server's reply. It may or may not have been accepted. **Never send it again
    /// automatically:** look for its Message-ID in the Sent folder, and ask the user if it stays
    /// unclear. Holds the technical reason.
    Unclear(String),
}

impl SendOutcome {
    /// `None` when accepted, otherwise the error to show
    /// ([`crate::ErrorCode::SendRejected`] or [`crate::ErrorCode::SendUnclear`]).
    pub fn error(&self) -> Option<BackendError> {
        match self {
            Self::Accepted(_) => None,
            Self::Rejected(rejection) => Some(rejection.to_error()),
            Self::Unclear(detail) => Some(BackendError::SendUnclear(detail.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;

    fn reply(code: u16, text: &str) -> SmtpReply {
        SmtpReply {
            code,
            enhanced: Some(format!("{}.1.1", code / 100)),
            text: text.to_owned(),
        }
    }

    #[test]
    fn refusals_become_typed_errors() {
        let rejection = SendRejection {
            step: SmtpStep::Recipients,
            reply: reply(450, "mailbox busy"),
            recipients: vec![
                RecipientRefusal {
                    address: "a@example.org".into(),
                    reply: reply(450, "mailbox busy"),
                },
                RecipientRefusal {
                    address: "b@example.org".into(),
                    reply: reply(550, "no such user"),
                },
            ],
        };
        assert!(rejection.is_permanent(), "one 5xx makes it permanent");
        let err = SendOutcome::Rejected(rejection).error().expect("error");
        assert_eq!(err.code(), ErrorCode::SendRejected);
        assert!(!err.is_transient());
        assert_eq!(
            err.detail().as_deref(),
            Some("a@example.org: 450 4.1.1 mailbox busy; b@example.org: 550 5.1.1 no such user")
        );

        let transient = SendRejection {
            step: SmtpStep::Message,
            reply: reply(451, "try later"),
            recipients: Vec::new(),
        };
        assert!(!transient.is_permanent());
        let err = transient.to_error();
        assert!(err.is_transient());
        assert_eq!(err.detail().as_deref(), Some("451 4.1.1 try later"));

        let unclear = SendOutcome::Unclear("connection closed".into());
        let err = unclear.error().expect("error");
        assert_eq!(err.code(), ErrorCode::SendUnclear);
        assert!(!err.is_transient(), "an unclear send is never retried");

        assert_eq!(SendOutcome::Accepted(reply(250, "ok")).error(), None);
        let bare = SmtpReply {
            code: 250,
            enhanced: None,
            text: String::new(),
        };
        assert_eq!(bare.to_string(), "250");
    }
}
