//! [`JmapSender`]: `MailSender` over JMAP (RFC 8621 section 7).
//!
//! The message is uploaded, imported into Drafts (`$draft`, `$seen`) and submitted with
//! `EmailSubmission/set`, whose `onSuccessDestroyEmail` removes the imported copy again; the
//! engine files its own Sent copy, as for IMAP. Everything up to the submission request is
//! "nothing sent" (`Err`); a refused submission is a rejection (the imported copy is removed);
//! a submission request whose answer never arrives is unclear and never repeated.

use mail_model::{
    Access, BackendError, ConnectConfig, Envelope, MailSender, RecipientRefusal, SendFuture,
    SendOutcome, SendRejection, SmtpLimits, SmtpReply, SmtpStep,
};
use serde_json::{Value, json};

use crate::capability;
use crate::client::Client;
use crate::connector::TlsConfigFn;
use crate::error;
use crate::session::{self, Session};

const USING: &[&str] = &[capability::CORE, capability::MAIL, capability::SUBMISSION];

/// Submits messages over JMAP.
#[derive(Clone)]
pub struct JmapSender {
    tls: TlsConfigFn,
    insecure_loopback: bool,
    log: Option<crate::client::MethodLog>,
}

impl JmapSender {
    pub fn new(tls: TlsConfigFn) -> Self {
        Self {
            tls,
            insecure_loopback: false,
            log: None,
        }
    }

    /// Records the methods its connections send (test support).
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_method_log(mut self, log: crate::client::MethodLog) -> Self {
        self.log = Some(log);
        self
    }

    #[cfg(test)]
    pub(crate) fn insecure_loopback_for_tests(mut self) -> Self {
        self.insecure_loopback = true;
        self
    }

    async fn open(&self, config: &ConnectConfig) -> Result<(Client, Session), BackendError> {
        let client = Client::new(config, &self.tls, self.insecure_loopback, self.log.clone())?;
        let session = session::fetch(&client, config).await?;
        if !session.submission {
            return Err(BackendError::Unsupported(
                "sending (the account has no JMAP submission)".into(),
            ));
        }
        Ok((client, session))
    }

    async fn submit(
        &self,
        config: &ConnectConfig,
        envelope: &Envelope,
        message: &[u8],
    ) -> Result<SendOutcome, BackendError> {
        if config.access != Access::ReadWrite {
            return Err(BackendError::ReadOnly("EmailSubmission/set".into()));
        }
        let (client, session) = self.open(config).await?;
        let account = session.account_id.clone();
        let responses = client
            .call(
                &session.api_url,
                USING,
                vec![
                    ("Identity/get", json!({"accountId": account}), "i"),
                    (
                        "Mailbox/get",
                        json!({"accountId": account, "ids": null, "properties": ["id", "role"]}),
                        "m",
                    ),
                ],
            )
            .await?;
        let identities = responses
            .get("i", "Identity/get")?
            .get("list")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let identity = identity_for(&identities, &envelope.from).ok_or_else(|| {
            BackendError::Rejected(format!(
                "the server has no sending identity for {}",
                envelope.from
            ))
        })?;
        let mailboxes = responses
            .get("m", "Mailbox/get")?
            .get("list")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let holder = ["drafts", "sent", "inbox"]
            .iter()
            .find_map(|role| {
                mailboxes
                    .iter()
                    .find(|m| m.get("role").and_then(Value::as_str) == Some(role))
            })
            .or_else(|| mailboxes.first())
            .and_then(|m| m.get("id").and_then(Value::as_str))
            .ok_or_else(|| BackendError::Rejected("the account has no mailbox".into()))?
            .to_owned();
        let blob = client
            .upload(&session.upload()?, "message/rfc822", message)
            .await?;
        let responses = client
            .call(
                &session.api_url,
                USING,
                vec![(
                    "Email/import",
                    json!({
                        "accountId": account,
                        "emails": {"m": {
                            "blobId": blob,
                            "mailboxIds": {holder: true},
                            "keywords": {"$draft": true, "$seen": true},
                        }},
                    }),
                    "e",
                )],
            )
            .await?;
        let args = responses.get("e", "Email/import")?;
        let Some(email_id) = args
            .get("created")
            .and_then(|c| c.get("m"))
            .and_then(|c| c.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            let err = args
                .get("notCreated")
                .and_then(|n| n.get("m"))
                .cloned()
                .unwrap_or(Value::Null);
            return Err(error::from_set_error("Email/import", &err));
        };

        // From here on the server may have taken the message.
        let submission = json!({
            "accountId": account,
            "create": {"s": {
                "identityId": identity,
                "emailId": email_id,
                "envelope": {
                    "mailFrom": {"email": envelope.from},
                    "rcptTo": envelope
                        .recipients
                        .iter()
                        .map(|r| json!({"email": r}))
                        .collect::<Vec<_>>(),
                },
            }},
            "onSuccessDestroyEmail": ["#s"],
        });
        let answer = client
            .call(
                &session.api_url,
                USING,
                vec![("EmailSubmission/set", submission, "s")],
            )
            .await;
        let responses = match answer {
            Ok(responses) => responses,
            // Nothing reached the server: safe to report as not sent.
            Err(
                err @ (BackendError::Dns(_)
                | BackendError::ConnectionRefused
                | BackendError::ConnectTimeout
                | BackendError::AuthFailed(_)
                | BackendError::ReadOnly(_)),
            ) => {
                return Err(err);
            }
            Err(err) => return Ok(SendOutcome::Unclear(err.to_string())),
        };
        let args = match responses.get("s", "EmailSubmission/set") {
            Ok(args) => args,
            Err(err) => {
                self.discard(&client, &session, &email_id).await;
                return Err(err);
            }
        };
        if let Some(created) = args.get("created").and_then(|c| c.get("s")) {
            let id = created.get("id").and_then(Value::as_str).unwrap_or("?");
            return Ok(SendOutcome::Accepted(SmtpReply {
                code: 250,
                enhanced: None,
                text: format!("accepted as JMAP submission {}", error::short(id)),
            }));
        }
        let err = args
            .get("notCreated")
            .and_then(|n| n.get("s"))
            .cloned()
            .unwrap_or(Value::Null);
        self.discard(&client, &session, &email_id).await;
        Ok(SendOutcome::Rejected(rejection(&err)))
    }

    /// Removes the imported copy after a refused submission (best effort).
    async fn discard(&self, client: &Client, session: &Session, email_id: &str) {
        let result = client
            .call(
                &session.api_url,
                USING,
                vec![(
                    "Email/set",
                    json!({"accountId": session.account_id, "destroy": [email_id]}),
                    "d",
                )],
            )
            .await;
        if let Err(err) = result {
            tracing::warn!(error = %err, "imported copy of a refused message not removed");
        }
    }
}

/// The identity whose address is `from` (case-insensitive), else one for the whole domain
/// (`*@domain`).
fn identity_for(identities: &[Value], from: &str) -> Option<String> {
    let email = |i: &Value| {
        i.get("email")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    let from = from.to_ascii_lowercase();
    let domain = from.rsplit_once('@').map(|(_, d)| format!("*@{d}"));
    identities
        .iter()
        .find(|i| email(i) == from)
        .or_else(|| identities.iter().find(|i| Some(email(i)) == domain))
        .and_then(|i| i.get("id").and_then(Value::as_str))
        .map(str::to_owned)
}

/// A refused submission (RFC 8621 section 7.5 set errors) in SMTP terms.
fn rejection(err: &Value) -> SendRejection {
    let kind = err.get("type").and_then(Value::as_str).unwrap_or("unknown");
    let text = error::set_error_text("EmailSubmission/set", err);
    let reply = |code: u16| SmtpReply {
        code,
        enhanced: None,
        text: text.clone(),
    };
    match kind {
        "invalidRecipients" => {
            let recipients: Vec<RecipientRefusal> = err
                .get("invalidRecipients")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|address| RecipientRefusal {
                    address: address.to_owned(),
                    reply: reply(550),
                })
                .collect();
            SendRejection {
                step: SmtpStep::Recipients,
                reply: reply(550),
                recipients,
            }
        }
        "noRecipients" | "tooManyRecipients" | "invalidProperties" => SendRejection {
            step: SmtpStep::Recipients,
            reply: reply(550),
            recipients: Vec::new(),
        },
        "forbiddenFrom" | "forbiddenMailFrom" | "forbiddenToSend" => SendRejection {
            step: SmtpStep::MailFrom,
            reply: reply(550),
            recipients: Vec::new(),
        },
        "tooLarge" => SendRejection {
            step: SmtpStep::Message,
            reply: reply(552),
            recipients: Vec::new(),
        },
        // Try again later.
        "rateLimit" | "serverUnavailable" | "serverFail" => SendRejection {
            step: SmtpStep::Message,
            reply: reply(451),
            recipients: Vec::new(),
        },
        _ => SendRejection {
            step: SmtpStep::Message,
            reply: reply(554),
            recipients: Vec::new(),
        },
    }
}

impl MailSender for JmapSender {
    fn send<'a>(
        &'a self,
        config: &'a ConnectConfig,
        envelope: &'a Envelope,
        message: &'a [u8],
    ) -> SendFuture<'a, SendOutcome> {
        Box::pin(self.submit(config, envelope, message))
    }

    fn verify<'a>(&'a self, config: &'a ConnectConfig) -> SendFuture<'a, ()> {
        Box::pin(async move { self.open(config).await.map(drop) })
    }

    fn limits<'a>(&'a self, config: &'a ConnectConfig) -> SendFuture<'a, SmtpLimits> {
        Box::pin(async move {
            let (_, session) = self.open(config).await?;
            Ok(SmtpLimits {
                max_message_bytes: session.max_message_size,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_by_address_then_domain() {
        let identities = serde_json::json!([
            {"id": "w", "email": "*@vicemail.test"},
            {"id": "b", "email": "Alice@Vicemail.test"}
        ]);
        let list = identities.as_array().expect("array");
        assert_eq!(
            identity_for(list, "alice@vicemail.test").as_deref(),
            Some("b")
        );
        assert_eq!(
            identity_for(list, "bob@vicemail.test").as_deref(),
            Some("w")
        );
        assert_eq!(identity_for(list, "eve@other.test"), None);
    }

    #[test]
    fn refusals_become_smtp_terms() {
        // Stalwart 1.1.1 refuses an address it cannot deliver to like this (recorded).
        let stalwart = serde_json::json!({"type": "invalidProperties",
            "description": "Invalid e-mail address \"nobody@elsewhere.example\".",
            "properties": ["envelope"]});
        let r = rejection(&stalwart);
        assert_eq!(r.step, SmtpStep::Recipients);
        assert!(r.is_permanent());
        assert!(
            r.reply.text.contains("nobody@elsewhere.example"),
            "{}",
            r.reply.text
        );
        let r = rejection(&serde_json::json!({"type": "invalidRecipients",
            "invalidRecipients": ["x@y.test"]}));
        assert_eq!(r.recipients.len(), 1);
        assert_eq!(r.recipients[0].address, "x@y.test");
        let r = rejection(&serde_json::json!({"type": "rateLimit"}));
        assert!(!r.is_permanent(), "try later");
        assert_eq!(
            rejection(&serde_json::json!({"type": "tooLarge"}))
                .reply
                .code,
            552
        );
        assert_eq!(
            rejection(&serde_json::json!({"type": "forbiddenFrom"})).step,
            SmtpStep::MailFrom
        );
    }
}
