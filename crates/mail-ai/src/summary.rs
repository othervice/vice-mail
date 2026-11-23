//! Thread summary: key points, open questions, deadlines (only when stated), every item with
//! the messages it is based on. Two steps, so the interface can show what will be sent and ask
//! for consent before anything leaves the device: [`prepare_summary`] (pure) and
//! [`summarize`] (sends, checks the stop reason, parses and validates).

use mail_model::MessageId;
use mail_model::search::{SlashDateOrder, TimeZone};

use crate::claims::{Claim, MAX_CLAIM_CHARS, claim, items};
use crate::prepare::{Budget, MailInput, Priority, SentMail, TransferInfo, prepare_mails};
use crate::prompt::{
    AnswerLanguage, SUMMARY_TASK, block, check_model, nonce, summary_schema, system_text,
};
use crate::provider::{AiProvider, ChatRequest, Finish, OutputSchema, collect};
use crate::text::{clean_output, extract_json_object};
use crate::{AiError, CancelToken, CloudPolicy, policy};

/// Most items kept per list (the prompt asks for fewer).
const MAX_KEY_POINTS: usize = 12;
const MAX_QUESTIONS: usize = 8;
const MAX_DEADLINES: usize = 8;

/// The user's language and time zone (dates in the data block are local times).
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub language: AnswerLanguage,
    /// `Sync`, so the futures of [`summarize`] and [`crate::ask`] are `Send`.
    pub time_zone: &'a (dyn TimeZone + Sync),
    /// Current time, Unix seconds ("today" for questions).
    pub now: i64,
    /// How `01/02/2026` in a generated query is read (as in the search field).
    pub slash_dates: SlashDateOrder,
}

impl std::fmt::Debug for Context<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("language", &self.language)
            .field("utc_offset", &self.time_zone.utc_offset(self.now))
            .field("now", &self.now)
            .finish()
    }
}

/// A deadline as the thread states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deadline {
    /// What is due.
    pub text: String,
    /// The deadline in the message's words ("Friday", "30.11.").
    pub due: String,
    pub sources: Vec<MessageId>,
}

/// A checked summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub key_points: Vec<Claim>,
    pub open_questions: Vec<Claim>,
    pub deadlines: Vec<Deadline>,
    /// Items dropped because they named no message that was sent (or had no text).
    pub dropped: u32,
    pub finish: Finish,
}

/// A summary request, ready to send.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedSummary {
    request: ChatRequest,
    sent: Vec<SentMail>,
    info: TransferInfo,
}

impl PreparedSummary {
    /// What would be sent, for the transparency display.
    pub fn transfer(&self) -> &TransferInfo {
        &self.info
    }

    pub fn request(&self) -> &ChatRequest {
        &self.request
    }
}

/// Builds the request for `thread` (oldest message first): the minimised data block inside
/// the user message, the fixed system text, the output schema.
pub fn prepare_summary(
    model: &str,
    thread: &[MailInput],
    budget: Budget,
    context: &Context<'_>,
) -> Result<PreparedSummary, AiError> {
    let model = check_model(model)?;
    if thread.is_empty() {
        return Err(AiError::NothingToSend);
    }
    let data = prepare_mails(thread, budget, Priority::Newest, true, context.time_zone);
    let user = format!(
        "Summarize this e-mail thread.\n\n{}",
        block("MAIL_DATA", &nonce()?, &data.json)?
    );
    let request = ChatRequest {
        model: model.clone(),
        system: system_text(SUMMARY_TASK, context.language),
        user,
        output: Some(OutputSchema {
            name: "thread_summary",
            schema: summary_schema(),
        }),
    };
    let mut info = data.info;
    info.model = model;
    info.bytes = request.text_bytes();
    Ok(PreparedSummary {
        request,
        sent: data.sent,
        info,
    })
}

/// Sends a prepared summary and checks the answer. Fails with [`AiError::NotPermitted`]
/// (nothing sent) when the provider is not local and an account of the thread has no cloud
/// permission. `progress` gets the characters received so far.
pub async fn summarize(
    provider: &dyn AiProvider,
    prepared: &PreparedSummary,
    cloud: &dyn CloudPolicy,
    cancel: &CancelToken,
    progress: impl FnMut(usize) + Send,
) -> Result<Summary, AiError> {
    policy::check(provider.info().locality, &prepared.info.accounts, cloud)?;
    let stream = provider.chat(&prepared.request, cancel).await?;
    let completed = collect(stream, progress).await?;
    let finish = completed.finish.clone();
    let text = completed.into_text()?;
    parse_summary(&text, &prepared.sent, finish)
}

/// Reads the model's answer. Pure, so it can be tested with any text.
pub(crate) fn parse_summary(
    text: &str,
    sent: &[SentMail],
    finish: Finish,
) -> Result<Summary, AiError> {
    let json = extract_json_object(text)
        .ok_or_else(|| AiError::InvalidOutput("no JSON object in the answer".into()))?;
    let mut dropped = 0_u32;
    let mut claims = |key: &str, max: usize| {
        let mut kept = Vec::new();
        for item in items(&json, key) {
            match claim(item, sent) {
                Some(claim) if kept.len() < max => kept.push(claim),
                Some(_) => {}
                None => dropped += 1,
            }
        }
        kept
    };
    let key_points = claims("key_points", MAX_KEY_POINTS);
    let open_questions = claims("open_questions", MAX_QUESTIONS);
    let mut deadlines = Vec::new();
    for item in items(&json, "deadlines") {
        let due = item
            .get("due")
            .and_then(serde_json::Value::as_str)
            .map(|due| clean_output(due, MAX_CLAIM_CHARS))
            .unwrap_or_default();
        match claim(item, sent) {
            // A deadline without a stated date is no deadline.
            Some(claim) if !due.is_empty() => {
                if deadlines.len() < MAX_DEADLINES {
                    deadlines.push(Deadline {
                        text: claim.text,
                        due,
                        sources: claim.sources,
                    });
                }
            }
            _ => dropped += 1,
        }
    }
    Ok(Summary {
        key_points,
        open_questions,
        deadlines,
        dropped,
        finish,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{StopReason, Usage};
    use mail_model::AccountId;

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
                id: MessageId(11),
                account,
            },
            SentMail {
                reference: "m2".into(),
                id: MessageId(12),
                account,
            },
        ]
    }

    #[test]
    fn invented_references_are_removed() {
        let answer = r#"Here is the summary:
```json
{"key_points": [
   {"text": "Anna needs the 2027 budget figures.", "refs": ["m1"]},
   {"text": "Bob will send them tomorrow.", "refs": ["m2", "m5"]},
   {"text": "The CEO approved everything.", "refs": ["m7"]},
   {"text": "No source at all.", "refs": []}
 ],
 "open_questions": [{"text": "Which format?", "refs": ["m1"]}, "junk"],
 "deadlines": [
   {"text": "Budget figures", "due": "Friday", "refs": ["m1"]},
   {"text": "Invented deadline", "due": "", "refs": ["m1"]},
   {"text": "Board meeting", "due": "Monday", "refs": ["m9"]}
 ]}
```"#;
        let summary = parse_summary(answer, &sent(), finish()).expect("summary");
        assert_eq!(
            summary.key_points,
            vec![
                Claim {
                    text: "Anna needs the 2027 budget figures.".into(),
                    sources: vec![MessageId(11)]
                },
                Claim {
                    text: "Bob will send them tomorrow.".into(),
                    sources: vec![MessageId(12)]
                },
            ]
        );
        assert_eq!(summary.open_questions.len(), 1);
        assert_eq!(
            summary.deadlines,
            vec![Deadline {
                text: "Budget figures".into(),
                due: "Friday".into(),
                sources: vec![MessageId(11)]
            }]
        );
        assert_eq!(summary.dropped, 5);
    }

    #[test]
    fn missing_lists_are_empty_and_prose_is_an_error() {
        let summary = parse_summary(r#"{"key_points": []}"#, &sent(), finish()).expect("summary");
        assert!(summary.key_points.is_empty() && summary.deadlines.is_empty());
        assert!(matches!(
            parse_summary("I cannot do that.", &sent(), finish()),
            Err(AiError::InvalidOutput(_))
        ));
    }

    #[test]
    fn output_texts_are_plain() {
        let answer = concat!(
            r#"{"key_points": [{"text": "See ![x](http://127.0.0.1:9/p.png) and "#,
            r#"<img src=http://e/x> \u202Enow", "refs": ["m1"]}], "#,
            r#""open_questions": [], "deadlines": []}"#
        );
        let summary = parse_summary(answer, &sent(), finish()).expect("summary");
        assert_eq!(
            summary.key_points[0].text,
            "See x and <img src=http://e/x> now"
        );
    }
}
