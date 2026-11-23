//! Reply suggestion (stage 13): a reply to the newest message of a thread, as plain text for
//! an editable draft. Two steps like the summary: [`prepare_reply`] (pure, shows what will be
//! sent) and [`suggest_reply`] (sends, checks the stop reason, reads the text).
//!
//! The suggestion is only text. Nothing here creates, saves or sends a draft: the engine puts
//! the text into a reply draft of stage 5 when the user opens it, and the user edits and sends
//! it (or not).

use mail_model::{Address, MessageId};
use serde_json::json;

use crate::prepare::{Budget, MailInput, Priority, SentMail, TransferInfo, prepare_mails};
use crate::prompt::{REPLY_TASK, block, check_model, nonce, reply_schema, system_text};
use crate::provider::{AiProvider, ChatRequest, Finish, OutputSchema, collect};
use crate::summary::Context;
use crate::text::{clean_line, clean_output_text, extract_json_object};
use crate::{AiError, CancelToken, CloudPolicy, policy};

/// Longest suggested reply, in characters.
pub const MAX_REPLY_CHARS: usize = 8_000;

/// A checked suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplySuggestion {
    /// Plain text (line breaks kept), for the body of a reply draft.
    pub text: String,
    /// The message the reply answers (the newest of the thread that was sent).
    pub in_reply_to: MessageId,
    pub finish: Finish,
}

/// A reply request, ready to send.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedReply {
    request: ChatRequest,
    sent: Vec<SentMail>,
    info: TransferInfo,
}

impl PreparedReply {
    pub fn transfer(&self) -> &TransferInfo {
        &self.info
    }

    pub fn request(&self) -> &ChatRequest {
        &self.request
    }
}

/// Builds the request for `thread` (oldest first; the reply answers the last message).
/// `user` is the identity the reply is written from (name and address), so the model knows
/// which side of the thread it writes for.
pub fn prepare_reply(
    model: &str,
    thread: &[MailInput],
    user: Option<&Address>,
    budget: Budget,
    context: &Context<'_>,
) -> Result<PreparedReply, AiError> {
    let model = check_model(model)?;
    if thread.is_empty() {
        return Err(AiError::NothingToSend);
    }
    let data = prepare_mails(thread, budget, Priority::Newest, true, context.time_zone);
    let nonce = nonce()?;
    let me = json!({
        "user": user.map(|address| json!({
            "name": address.name.as_deref().map(|name| clean_line(name, 120)),
            "email": clean_line(&address.email, 320),
        })),
    });
    let user_message = format!(
        "Write a reply to the newest message of this e-mail thread.\n\n{}\n\n{}",
        block("REPLY_CONTEXT", &nonce, &me)?,
        block("MAIL_DATA", &nonce, &data.json)?
    );
    let request = ChatRequest {
        model: model.clone(),
        system: system_text(REPLY_TASK, context.language),
        user: user_message,
        output: Some(OutputSchema {
            name: "reply_suggestion",
            schema: reply_schema(),
        }),
    };
    let mut info = data.info;
    info.model = model;
    info.bytes = request.text_bytes();
    Ok(PreparedReply {
        request,
        sent: data.sent,
        info,
    })
}

/// Sends a prepared reply request. Fails with [`AiError::NotPermitted`] (nothing sent) when
/// the provider is not local and an account of the thread has no cloud permission.
pub async fn suggest_reply(
    provider: &dyn AiProvider,
    prepared: &PreparedReply,
    cloud: &dyn CloudPolicy,
    cancel: &CancelToken,
    progress: impl FnMut(usize) + Send,
) -> Result<ReplySuggestion, AiError> {
    policy::check(provider.info().locality, &prepared.info.accounts, cloud)?;
    let stream = provider.chat(&prepared.request, cancel).await?;
    let completed = collect(stream, progress).await?;
    let finish = completed.finish.clone();
    let text = completed.into_text()?;
    parse_reply(&text, &prepared.sent, finish)
}

/// Reads the model's answer. Pure.
pub(crate) fn parse_reply(
    text: &str,
    sent: &[SentMail],
    finish: Finish,
) -> Result<ReplySuggestion, AiError> {
    let in_reply_to = sent
        .last()
        .map(|mail| mail.id)
        .ok_or(AiError::NothingToSend)?;
    let json = extract_json_object(text)
        .ok_or_else(|| AiError::InvalidOutput("no JSON object in the answer".into()))?;
    let reply = json
        .get("reply")
        .and_then(serde_json::Value::as_str)
        .map(|reply| clean_output_text(reply, MAX_REPLY_CHARS))
        .unwrap_or_default();
    if reply.is_empty() {
        return Err(AiError::InvalidOutput("the answer has no reply text".into()));
    }
    Ok(ReplySuggestion {
        text: reply,
        in_reply_to,
        finish,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::AnswerLanguage;
    use crate::provider::{StopReason, Usage};
    use mail_model::AccountId;
    use mail_model::search::{FixedOffset, SlashDateOrder};

    fn finish() -> Finish {
        Finish {
            stop: StopReason::Complete,
            usage: Usage::default(),
            model: None,
            request_id: None,
        }
    }

    fn sent() -> Vec<SentMail> {
        let account: AccountId = "00000000-0000-4000-8000-000000000001"
            .parse()
            .expect("account");
        vec![
            SentMail {
                reference: "m1".into(),
                id: MessageId(10),
                account,
            },
            SentMail {
                reference: "m2".into(),
                id: MessageId(11),
                account,
            },
        ]
    }

    #[test]
    fn the_reply_is_cleaned_text_for_the_newest_message() {
        let suggestion = parse_reply(
            r#"{"reply": "Hi Anna,\n\nyes, Friday works.\u0007\n\n\n\n![x](https://evil.example/p.png)Bob"}"#,
            &sent(),
            finish(),
        )
        .expect("reply");
        assert_eq!(suggestion.in_reply_to, MessageId(11));
        assert_eq!(suggestion.text, "Hi Anna,\n\nyes, Friday works.\n\n\nxBob");
        assert!(!suggestion.text.contains("https://"));
        assert!(parse_reply(r#"{"reply": "  "}"#, &sent(), finish()).is_err());
        assert!(parse_reply("no json", &sent(), finish()).is_err());
        assert!(parse_reply(r#"{"reply": "x"}"#, &[], finish()).is_err());
    }

    #[test]
    fn the_request_names_the_user_inside_a_data_block() {
        let zone = FixedOffset(0);
        let context = Context {
            language: AnswerLanguage::English,
            time_zone: &zone,
            now: 1_795_392_000,
            slash_dates: SlashDateOrder::DayFirst,
        };
        let account: AccountId = "00000000-0000-4000-8000-000000000001"
            .parse()
            .expect("account");
        let mail = MailInput {
            id: MessageId(3),
            account,
            from: None,
            to: Vec::new(),
            cc: Vec::new(),
            date: None,
            subject: "Offer".into(),
            text: "Can you send the offer?".into(),
            hidden_text_chars: 0,
            attachments: Vec::new(),
        };
        let me = Address {
            name: Some("Bob <admin>".into()),
            email: "bob@example.org".into(),
        };
        let prepared =
            prepare_reply("m", &[mail], Some(&me), Budget::THREAD, &context).expect("prepared");
        let user = prepared.request().user();
        assert!(user.contains("<<<REPLY_CONTEXT nonce="), "{user}");
        assert!(user.contains("bob@example.org"));
        assert!(!user.contains("<admin>"), "angle brackets are escaped: {user}");
        assert!(prepared.request().system().contains("Task: write a reply"));
        assert_eq!(prepared.transfer().mails, 1);
        assert_eq!(prepared.transfer().bytes, prepared.request().text_bytes());
        assert!(matches!(
            prepare_reply("m", &[], None, Budget::THREAD, &context),
            Err(AiError::NothingToSend)
        ));
    }
}
