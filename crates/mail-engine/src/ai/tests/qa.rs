//! The QA set of questions in natural language (`testmail/corpus/qa/questions.json`, 30
//! questions over the fixed seed corpus) against the in-memory mail server.
//!
//! - `qa_set_with_the_mock_model`: the mock answers the query step with the set's reference
//!   query and the answer step by citing every candidate it got. Measures the local part
//!   (search with a good query: is an expected message among the candidates, is it the first)
//!   and checks the pipeline: "nothing found" questions send one request and no mail, every
//!   source is a stored message, invented references are dropped.
//! - `qa_set_with_a_real_model` (ignored): the same questions against a local model the human
//!   names (`VICE_MAIL_QA_BASE_URL=http://localhost:11434/v1 VICE_MAIL_QA_MODEL=llama3.2`);
//!   prints how often the cited messages are the expected ones. Results go to
//!   `docs/testing/ai-search.md`.

use std::collections::HashMap;
use std::path::PathBuf;

use mail_model::{AiAnswer, BodyFormat, SearchIndexState};

use super::*;

#[derive(serde::Deserialize)]
struct QaSet {
    questions: Vec<Question>,
}

#[derive(serde::Deserialize)]
struct Question {
    id: u32,
    question: String,
    query: String,
    expect: Vec<String>,
}

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testmail/corpus")
}

struct Qa {
    h: QaApp,
    /// Seed file name by message.
    files: HashMap<MessageId, String>,
    set: QaSet,
}

struct QaApp {
    _dir: TempDir,
    core: AppCore<FakeConnector>,
    mock: Mock,
}

async fn qa(real: Option<(String, String)>) -> Qa {
    let set: QaSet = serde_json::from_slice(
        &std::fs::read(corpus().join("qa/questions.json")).expect("questions.json"),
    )
    .expect("valid QA set");
    assert_eq!(set.questions.len(), 30);
    let mut names: Vec<String> = std::fs::read_dir(corpus().join("seed"))
        .expect("seed dir")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".eml") && !n.starts_with("broken_"))
        .collect();
    names.sort();
    let server = FakeServer::new();
    for name in &names {
        let raw = std::fs::read(corpus().join("seed").join(name)).expect("seed file");
        server.deliver_raw("INBOX", &raw);
    }
    let dir = TempDir::new().expect("temp dir");
    let mut services = CoreServices::with_connector(
        Credentials::new(MemorySecretStore::new()),
        FakeConnector(Arc::clone(&server)),
    );
    services.sync = sync_config();
    services.reading = ReadingServices {
        opener: Arc::new(RecordingOpener::default()) as Arc<dyn crate::reading::SystemOpener>,
        ..ReadingServices::default()
    };
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0.0.0".into(),
                commit: None,
                build_date: None,
            },
        },
        services,
    )
    .await;
    let server_settings = |host: &str, port| ServerSettings {
        host: host.into(),
        port,
        security: Security::Tls,
        username: "alice".into(),
    };
    let account = core
        .add_account(
            AccountSettings {
                protocol: Default::default(),
                email: "alice@fake.test".into(),
                display_name: "Alice".into(),
                imap: server_settings("imap.fake.test", 993),
                smtp: server_settings("smtp.fake.test", 465),
            },
            Secret::new(PASSWORD.into()),
        )
        .await
        .expect("add account")
        .id;
    // Every message synced, every body stored (opened once), the index built.
    let deadline = Instant::now() + Duration::from_secs(60);
    let items = loop {
        let inbox = core
            .list_mailboxes(account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.role == MailboxRole::Inbox);
        if let Some(inbox) = inbox {
            let items = core
                .list_messages(inbox.id, None, 500)
                .await
                .expect("list")
                .items;
            if items.len() == names.len() {
                break items;
            }
        }
        assert!(Instant::now() < deadline, "sync did not finish");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let mut files = HashMap::new();
    for item in &items {
        // The in-memory server dates message `serial` at 1_700_000_000 + serial * 60 seconds
        // (list rows carry milliseconds), and the serials follow the delivery order.
        let serial = usize::try_from((item.date / 1000 - 1_700_000_000) / 60).expect("serial");
        files.insert(item.id, names[serial - 1].clone());
        let _ = core.get_message(item.id, BodyFormat::Text).await;
    }
    loop {
        let status = core.get_search_index_status().await.expect("index status");
        if status.state == SearchIndexState::Ready && status.percent == 100 {
            break;
        }
        assert!(Instant::now() < deadline, "index not ready: {status:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mock = Mock::start().await;
    let (url, model) = match real {
        Some(real) => real,
        None => {
            core.set_ai_provider_factory(route_to(&mock));
            (mock.url(), "test-model".to_owned())
        }
    };
    let profile = core
        .save_ai_profile(AiProfileInput {
            id: None,
            name: "QA".into(),
            kind: AiProviderKind::OpenaiCompatible,
            base_url: url,
            model,
            structured_output: true,
            max_output_tokens: None,
            key: AiKeyChange::Keep,
        })
        .await
        .expect("profile")
        .profiles[0]
        .id
        .clone();
    core.set_ai_function(AiFunction::Search, Some(profile))
        .await
        .expect("function");
    core.set_ai_enabled(true).await.expect("on");
    Qa {
        h: QaApp {
            _dir: dir,
            core,
            mock,
        },
        files,
        set,
    }
}

/// Per question: an expected message among the sources (any), the first source expected
/// (first), and for "nothing" questions whether the app said nothing.
#[derive(Default)]
struct Score {
    found_questions: u32,
    any: u32,
    first: u32,
    none_questions: u32,
    none_right: u32,
}

fn score(qa: &Qa, question: &Question, answer: &AiAnswer, score: &mut Score) -> String {
    let sources: Vec<&str> = answer
        .statements
        .iter()
        .flat_map(|s| &s.sources)
        .filter_map(|id| qa.files.get(id).map(String::as_str))
        .collect();
    if question.expect.is_empty() {
        score.none_questions += 1;
        if !answer.found {
            score.none_right += 1;
        }
    } else {
        score.found_questions += 1;
        if sources
            .iter()
            .any(|s| question.expect.iter().any(|e| e == s))
        {
            score.any += 1;
        }
        if sources
            .first()
            .is_some_and(|s| question.expect.iter().any(|e| e == s))
        {
            score.first += 1;
        }
    }
    format!(
        "{:>2} found={} candidates={} query={:?} sources={:?} expect={:?}",
        question.id, answer.found, answer.candidates, answer.query, sources, question.expect
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn qa_set_with_the_mock_model() {
    let qa = qa(None).await;
    let mut total = Score::default();
    let refs: Vec<String> = (1..=20)
        .map(|i| format!("m{i}"))
        .chain(["m99".into()])
        .collect();
    for question in &qa.set.questions {
        lock(&qa.h.mock.state).answers.clear();
        qa.h.mock
            .answer(&serde_json::json!({ "query": question.query }).to_string());
        qa.h.mock.answer(
            &serde_json::json!({"found": true, "statements": [{"text": "Cited.", "refs": refs}]})
                .to_string(),
        );
        let before = qa.h.mock.requests().len();
        let answer =
            qa.h.core
                .ai_ask(
                    format!("qa-{}", question.id),
                    question.question.clone(),
                    None,
                )
                .await
                .expect("answer");
        let line = score(&qa, question, &answer, &mut total);
        println!("{line}");
        // Every source is a stored message; the invented m99 is gone.
        for id in answer.statements.iter().flat_map(|s| &s.sources) {
            assert!(qa.files.contains_key(id), "{line}");
        }
        if answer.candidates == 0 {
            assert!(!answer.found, "{line}");
            assert_eq!(
                qa.h.mock.requests().len() - before,
                1,
                "only the query: {line}"
            );
            assert_eq!(answer.transfers[0].mails, 0);
        } else {
            assert_eq!(answer.dropped, 0, "{line}");
        }
    }
    println!(
        "found questions {}: expected among candidates {}, first {}; nothing-questions {}: said nothing {}",
        total.found_questions, total.any, total.first, total.none_questions, total.none_right
    );
    // A regression guard for the local search with good queries (see docs/testing/ai-search.md).
    assert_eq!(total.none_right, total.none_questions);
    assert!(
        total.any * 10 >= total.found_questions * 9,
        "expected messages among the candidates for fewer than 90% of the questions"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a local model: VICE_MAIL_QA_BASE_URL and VICE_MAIL_QA_MODEL"]
async fn qa_set_with_a_real_model() {
    let (Ok(url), Ok(model)) = (
        std::env::var("VICE_MAIL_QA_BASE_URL"),
        std::env::var("VICE_MAIL_QA_MODEL"),
    ) else {
        // `--include-ignored` runs every server test; without a model there is nothing to measure.
        eprintln!("skipped: set VICE_MAIL_QA_BASE_URL and VICE_MAIL_QA_MODEL to run the QA set");
        return;
    };
    let qa = qa(Some((url, model))).await;
    let mut total = Score::default();
    for question in &qa.set.questions {
        match qa
            .h
            .core
            .ai_ask(
                format!("qa-{}", question.id),
                question.question.clone(),
                None,
            )
            .await
        {
            Ok(answer) => {
                println!("{}", score(&qa, question, &answer, &mut total));
                for statement in &answer.statements {
                    println!("     - {}", statement.text);
                }
            }
            Err(err) => println!("{:>2} error {err}", question.id),
        }
    }
    println!(
        "found questions {}: expected cited {}, cited first {}; nothing-questions {}: said nothing {}",
        total.found_questions, total.any, total.first, total.none_questions, total.none_right
    );
}
