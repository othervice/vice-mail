//! Questions in natural language (stage 13): question → query in the search language of
//! stage 7 → local candidates → an answer only from the candidates, every statement with its
//! messages. No candidates (or no usable query): "nothing found", without asking the model
//! and without any statement.
//!
//! The steps are pure functions around the provider, so the engine can run the search in
//! between ([`CandidateSource`]) and the interface can show each transfer. The first request
//! carries only the question; mail content goes out only in the answer step, and there only
//! from accounts the [`CloudPolicy`] allows for a cloud provider (the others are left out and
//! counted in [`Answer::excluded`]).

use mail_model::search::{CivilDate, MAX_QUERY_BYTES, SearchContext, parse};
use serde_json::json;

use crate::claims::{Claim, claim, items};
use crate::prepare::{Budget, MailInput, Priority, SentMail, TransferInfo, prepare_mails};
use crate::prompt::{
    ANSWER_TASK, QUERY_SYSTEM, answer_schema, block, check_model, nonce, query_schema, system_text,
};
use crate::provider::{AiProvider, BoxFuture, ChatRequest, OutputSchema, collect};
use crate::summary::Context;
use crate::text::{clean_line, extract_json_object};
use crate::{AiError, CancelToken, CloudPolicy, policy};

/// Longest question, in characters.
pub const MAX_QUESTION_CHARS: usize = 1000;
/// Most statements kept.
const MAX_STATEMENTS: usize = 8;

/// The search the model proposed, checked by the parser of stage 7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlan {
    /// The query as the search field would take it.
    pub query: String,
    /// Problems the parser reported (the parser drops or re-reads such terms).
    pub problems: usize,
    /// The query has no usable term: nothing is searched.
    pub empty: bool,
}

/// Runs the search of stage 7 for a plan. Implemented by the engine; must return only
/// messages the user may see, best first, with their plain text.
pub trait CandidateSource: Send + Sync {
    fn search<'a>(&'a self, plan: &'a QueryPlan) -> BoxFuture<'a, Result<Vec<MailInput>, AiError>>;
}

/// A request without mail content: the question becomes a query.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedQuery {
    request: ChatRequest,
    info: TransferInfo,
}

impl PreparedQuery {
    pub fn transfer(&self) -> &TransferInfo {
        &self.info
    }

    pub fn request(&self) -> &ChatRequest {
        &self.request
    }
}

/// The answer request: question and candidates.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedAnswer {
    request: ChatRequest,
    sent: Vec<SentMail>,
    info: TransferInfo,
}

impl PreparedAnswer {
    pub fn transfer(&self) -> &TransferInfo {
        &self.info
    }

    pub fn request(&self) -> &ChatRequest {
        &self.request
    }
}

/// What came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerOutcome {
    /// Statements, each with at least one message that was sent.
    Found(Vec<Claim>),
    /// Honestly nothing: no candidates, the model found no answer in them, or none of its
    /// statements named a message that was sent.
    NothingFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub outcome: AnswerOutcome,
    /// The query that was searched (empty if the model gave none).
    pub query: String,
    /// Messages the search found.
    pub candidates: u32,
    /// Candidates not sent because their account has no cloud permission.
    pub excluded: u32,
    /// Statements dropped for lack of a valid reference.
    pub dropped: u32,
    /// One entry per request that was sent, for the request log.
    pub transfers: Vec<TransferInfo>,
}

/// The question, cleaned; empty is an error.
fn question_text(question: &str) -> Result<String, AiError> {
    let question = clean_line(question, MAX_QUESTION_CHARS);
    if question.is_empty() {
        return Err(AiError::NothingToSend);
    }
    Ok(question)
}

/// Step 1: the request that turns `question` into a query. Today's date is stated so that
/// "last week" can become a date filter.
pub fn prepare_query(
    model: &str,
    question: &str,
    context: &Context<'_>,
) -> Result<PreparedQuery, AiError> {
    let model = check_model(model)?;
    let question = question_text(question)?;
    let today = CivilDate::today(context.now, context.time_zone);
    let user = format!(
        "Today is {:04}-{:02}-{:02}.\n\n{}",
        today.year,
        today.month,
        today.day,
        block("QUESTION", &nonce()?, &json!({ "question": question }))?
    );
    let request = ChatRequest {
        model: model.clone(),
        system: QUERY_SYSTEM.to_owned(),
        user,
        output: Some(OutputSchema {
            name: "search_query",
            schema: query_schema(),
        }),
    };
    let info = TransferInfo {
        model,
        bytes: request.text_bytes(),
        ..TransferInfo::default()
    };
    Ok(PreparedQuery { request, info })
}

/// Reads the query from the model's answer and checks it with the search parser.
pub fn parse_query(text: &str, context: &Context<'_>) -> Result<QueryPlan, AiError> {
    let json = extract_json_object(text)
        .ok_or_else(|| AiError::InvalidOutput("no JSON object in the answer".into()))?;
    let raw = json
        .get("query")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| AiError::InvalidOutput("no query in the answer".into()))?;
    let mut query = clean_line(raw, MAX_QUERY_BYTES);
    while query.len() > MAX_QUERY_BYTES {
        query.pop();
    }
    let search = SearchContext {
        now: context.now,
        time_zone: context.time_zone,
        slash_dates: context.slash_dates,
    };
    let parsed = parse(&query, &search);
    Ok(QueryPlan {
        empty: parsed.is_empty(),
        problems: parsed.errors.len(),
        query,
    })
}

/// Step 3: the answer request from `candidates` (best first). Callers filter by cloud
/// permission first ([`ask`] does).
pub fn prepare_answer(
    model: &str,
    question: &str,
    candidates: &[MailInput],
    budget: Budget,
    context: &Context<'_>,
) -> Result<PreparedAnswer, AiError> {
    let model = check_model(model)?;
    let question = question_text(question)?;
    if candidates.is_empty() {
        return Err(AiError::NothingToSend);
    }
    let data = prepare_mails(
        candidates,
        budget,
        Priority::AsGiven,
        false,
        context.time_zone,
    );
    let nonce = nonce()?;
    let user = format!(
        "{}\n\n{}",
        block("QUESTION", &nonce, &json!({ "question": question }))?,
        block("MAIL_DATA", &nonce, &data.json)?
    );
    let request = ChatRequest {
        model: model.clone(),
        system: system_text(ANSWER_TASK, context.language),
        user,
        output: Some(OutputSchema {
            name: "answer",
            schema: answer_schema(),
        }),
    };
    let mut info = data.info;
    info.model = model;
    info.bytes = request.text_bytes();
    Ok(PreparedAnswer {
        request,
        sent: data.sent,
        info,
    })
}

/// Reads the answer: statements with valid references, and how many were dropped. `found:
/// false` or no valid statement is [`AnswerOutcome::NothingFound`].
pub(crate) fn parse_answer(text: &str, sent: &[SentMail]) -> Result<(AnswerOutcome, u32), AiError> {
    let json = extract_json_object(text)
        .ok_or_else(|| AiError::InvalidOutput("no JSON object in the answer".into()))?;
    let found = json.get("found").and_then(serde_json::Value::as_bool) != Some(false);
    let mut dropped = 0_u32;
    let mut statements = Vec::new();
    for item in items(&json, "statements") {
        match claim(item, sent) {
            Some(claim) if statements.len() < MAX_STATEMENTS => statements.push(claim),
            Some(_) => {}
            None => dropped += 1,
        }
    }
    if !found || statements.is_empty() {
        // "Nothing found" never carries statements, even if the model wrote some.
        return Ok((AnswerOutcome::NothingFound, dropped));
    }
    Ok((AnswerOutcome::Found(statements), dropped))
}

/// What [`ask`] needs besides the provider.
#[derive(Debug, Clone, Copy)]
pub struct AskRequest<'a> {
    pub model: &'a str,
    pub question: &'a str,
    pub budget: Budget,
    pub context: Context<'a>,
}

/// Step 1 on its own, for callers that show every transfer before it starts: sends the
/// question (no mail content) and checks the proposed query.
pub async fn send_query(
    provider: &dyn AiProvider,
    prepared: &PreparedQuery,
    context: &Context<'_>,
    cancel: &CancelToken,
    progress: impl FnMut(usize) + Send,
) -> Result<QueryPlan, AiError> {
    let stream = provider.chat(&prepared.request, cancel).await?;
    let text = collect(stream, progress).await?.into_text()?;
    parse_query(&text, context)
}

/// Step 3 on its own: sends the candidates and returns the checked outcome and the number of
/// statements dropped for lack of a valid reference. Fails with [`AiError::NotPermitted`]
/// (nothing sent) if a candidate's account has no cloud permission and the provider is not
/// local; callers filter first ([`partition_candidates`]).
pub async fn send_answer(
    provider: &dyn AiProvider,
    prepared: &PreparedAnswer,
    cloud: &dyn CloudPolicy,
    cancel: &CancelToken,
    progress: impl FnMut(usize) + Send,
) -> Result<(AnswerOutcome, u32), AiError> {
    policy::check(provider.info().locality, &prepared.info.accounts, cloud)?;
    let stream = provider.chat(&prepared.request, cancel).await?;
    let text = collect(stream, progress).await?.into_text()?;
    parse_answer(&text, &prepared.sent)
}

/// Splits candidates into those the provider may see and those it may not (accounts without
/// cloud permission, for a provider that is not local).
pub fn partition_candidates(
    candidates: Vec<MailInput>,
    locality: crate::Locality,
    cloud: &dyn CloudPolicy,
) -> (Vec<MailInput>, Vec<MailInput>) {
    candidates
        .into_iter()
        .partition(|mail| policy::allowed(locality, &mail.account, cloud))
}

/// The whole flow. Sends at most two requests: the query step (question only) and, if the
/// search found permitted candidates, the answer step.
pub async fn ask(
    provider: &dyn AiProvider,
    request: &AskRequest<'_>,
    source: &dyn CandidateSource,
    cloud: &dyn CloudPolicy,
    cancel: &CancelToken,
) -> Result<Answer, AiError> {
    let context = &request.context;
    let prepared = prepare_query(request.model, request.question, context)?;
    let mut answer = Answer {
        outcome: AnswerOutcome::NothingFound,
        query: String::new(),
        candidates: 0,
        excluded: 0,
        dropped: 0,
        transfers: vec![prepared.info.clone()],
    };
    let stream = provider.chat(&prepared.request, cancel).await?;
    let text = collect(stream, |_| {}).await?.into_text()?;
    let plan = parse_query(&text, context)?;
    answer.query.clone_from(&plan.query);
    if plan.empty {
        return Ok(answer);
    }
    if cancel.is_cancelled() {
        return Err(AiError::Cancelled);
    }

    let found = source.search(&plan).await?;
    answer.candidates = u32::try_from(found.len()).unwrap_or(u32::MAX);
    let locality = provider.info().locality;
    let (allowed, refused): (Vec<MailInput>, Vec<MailInput>) = found
        .into_iter()
        .partition(|mail| policy::allowed(locality, &mail.account, cloud));
    answer.excluded = u32::try_from(refused.len()).unwrap_or(u32::MAX);
    if allowed.is_empty() {
        return Ok(answer);
    }

    let prepared = prepare_answer(
        request.model,
        request.question,
        &allowed,
        request.budget,
        context,
    )?;
    // The same check `summarize` makes; here it can only pass (refused accounts are gone).
    policy::check(locality, &prepared.info.accounts, cloud)?;
    answer.transfers.push(prepared.info.clone());
    let stream = provider.chat(&prepared.request, cancel).await?;
    let text = collect(stream, |_| {}).await?.into_text()?;
    let (outcome, dropped) = parse_answer(&text, &prepared.sent)?;
    answer.outcome = outcome;
    answer.dropped = dropped;
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::AnswerLanguage;
    use mail_model::search::{FixedOffset, SlashDateOrder};
    use mail_model::{AccountId, MessageId};

    fn context(zone: &FixedOffset) -> Context<'_> {
        Context {
            language: AnswerLanguage::German,
            time_zone: zone,
            now: 1_795_392_000, // 2026-11-23 00:00 UTC
            slash_dates: SlashDateOrder::DayFirst,
        }
    }

    #[test]
    fn the_query_request_carries_only_the_question() {
        let zone = FixedOffset(3600);
        let prepared = prepare_query(
            "m",
            "  Wann kommt die Rechnung\n von Anna? ",
            &context(&zone),
        )
        .expect("prepared");
        let user = prepared.request().user();
        assert!(
            user.starts_with("Today is 2026-11-23.\n\n<<<QUESTION nonce="),
            "{user}"
        );
        assert!(
            user.contains(r#"{"question":"Wann kommt die Rechnung von Anna?"}"#),
            "{user}"
        );
        assert_eq!(prepared.request().system(), QUERY_SYSTEM);
        assert_eq!(prepared.transfer().mails, 0);
        assert_eq!(prepared.transfer().bytes, prepared.request().text_bytes());
        assert!(matches!(
            prepare_query("m", " \n ", &context(&zone)),
            Err(AiError::NothingToSend)
        ));
    }

    #[test]
    fn queries_are_checked_by_the_search_parser() {
        let zone = FixedOffset(0);
        let plan = parse_query(
            r#"{"query": "from:anna rechnung newer_than:30d"}"#,
            &context(&zone),
        )
        .expect("plan");
        assert_eq!(plan.query, "from:anna rechnung newer_than:30d");
        assert!(!plan.empty);
        assert_eq!(plan.problems, 0);
        let bad = parse_query(r#"{"query": "is:bogus !!!"}"#, &context(&zone)).expect("plan");
        assert!(bad.empty);
        assert!(bad.problems >= 1);
        assert!(parse_query("{}", &context(&zone)).is_err());
        assert!(parse_query("no", &context(&zone)).is_err());
    }

    fn sent() -> Vec<SentMail> {
        let account: AccountId = "00000000-0000-4000-8000-000000000001"
            .parse()
            .expect("account");
        vec![SentMail {
            reference: "m1".into(),
            id: MessageId(5),
            account,
        }]
    }

    #[test]
    fn nothing_found_never_carries_statements() {
        let (outcome, dropped) = parse_answer(
            r#"{"found": false, "statements": [{"text": "Maybe Tuesday", "refs": ["m1"]}]}"#,
            &sent(),
        )
        .expect("answer");
        assert_eq!(outcome, AnswerOutcome::NothingFound);
        assert_eq!(dropped, 0);
        let (outcome, dropped) = parse_answer(
            r#"{"found": true, "statements": [{"text": "Invented", "refs": ["m4"]}]}"#,
            &sent(),
        )
        .expect("answer");
        assert_eq!(outcome, AnswerOutcome::NothingFound);
        assert_eq!(dropped, 1);
        let (outcome, _) = parse_answer(
            r#"{"found": true, "statements": [{"text": "On Tuesday", "refs": ["m1"]}]}"#,
            &sent(),
        )
        .expect("answer");
        assert_eq!(
            outcome,
            AnswerOutcome::Found(vec![Claim {
                text: "On Tuesday".into(),
                sources: vec![MessageId(5)]
            }])
        );
    }
}
