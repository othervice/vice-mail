//! Accuracy of the spam filter on a corpus (decision 0077, `docs/testing/spam-filter.md`).
//!
//! Not part of the normal test run: the corpus is not in the repository. Point
//! `VICE_MAIL_SPAM_CORPUS` at a directory whose subdirectories named `ham*` hold ham and `spam*`
//! hold spam, one raw message per file (the layout of the SpamAssassin public corpus once
//! unpacked), and run
//! `cargo test -p mail-content --locked spam::corpus -- --ignored --nocapture`.
//! The files are only read and tokenised; nothing is stored.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use mail_model::Address;

use super::*;
use crate::{parse, preview_from_text};

struct Mail {
    name: String,
    spam: bool,
    tokens: SpamTokens,
    fields: SpamTokens,
}

#[derive(Default)]
struct Model {
    counts: HashMap<u64, SpamCounts>,
    totals: SpamTotals,
}

impl Model {
    fn learn(&mut self, mail: &Mail) {
        for hash in mail.tokens.hashes() {
            let entry = self.counts.entry(*hash).or_default();
            if mail.spam {
                entry.spam += 1;
            } else {
                entry.ham += 1;
            }
        }
        if mail.spam {
            self.totals.spam += 1;
        } else {
            self.totals.ham += 1;
        }
    }

    fn score(&self, tokens: &SpamTokens) -> f64 {
        let counts = tokens
            .hashes()
            .iter()
            .map(|h| self.counts.get(h).copied().unwrap_or_default());
        spam_score(self.totals, counts).map_or(0.5, |s| s.score)
    }
}

/// A fixed shuffle (xorshift), so runs are comparable.
fn shuffle<T>(items: &mut [T], mut seed: u64) {
    for i in (1..items.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = usize::try_from(seed % (i as u64 + 1)).unwrap_or(0);
        items.swap(i, j);
    }
}

fn read_corpus(root: &Path) -> (Vec<Mail>, Vec<Duration>) {
    let mut mails = Vec::new();
    let mut times = Vec::new();
    let mut dirs: Vec<_> = std::fs::read_dir(root)
        .expect("corpus directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let spam = if name.starts_with("spam") {
            true
        } else if name.starts_with("ham") || name.contains("ham") {
            false
        } else {
            continue;
        };
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("corpus subdirectory")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.file_name().is_some_and(|n| n != "cmds"))
            .collect();
        files.sort();
        for file in files {
            let Ok(raw) = std::fs::read(&file) else {
                continue;
            };
            let started = Instant::now();
            let parsed = parse(&raw);
            let tokens = spam_tokens(&raw, &parsed);
            times.push(started.elapsed());
            let from = parsed.headers.from.first().cloned();
            let reply_to: Vec<Address> = parsed.headers.reply_to.clone();
            let preview = preview_from_text(&parsed.display_text());
            let fields = spam_tokens_of_fields(&SpamFields {
                from: from.as_ref(),
                reply_to: &reply_to,
                recipients: parsed.headers.to.len() + parsed.headers.cc.len(),
                subject: parsed.headers.subject.as_deref().unwrap_or_default(),
                list_id: parsed.headers.list.id.as_deref(),
                preview: Some(&preview),
            });
            mails.push(Mail {
                name: file.display().to_string(),
                spam,
                tokens,
                fields,
            });
        }
    }
    (mails, times)
}

struct Outcome {
    index: usize,
    spam: bool,
    score: f64,
    fields_score: f64,
}

fn report(label: &str, outcomes: &[Outcome], fields: bool) {
    let hams = outcomes.iter().filter(|o| !o.spam).count();
    let spams = outcomes.len() - hams;
    eprintln!("{label}: {hams} ham, {spams} spam");
    eprintln!("  threshold | false positives (ham as spam) | missed spam");
    for threshold in [0.5, 0.8, 0.9, 0.95, 0.98, 0.99, 0.995] {
        let score = |o: &Outcome| if fields { o.fields_score } else { o.score };
        let fp = outcomes
            .iter()
            .filter(|o| !o.spam && score(o) >= threshold)
            .count();
        let fn_ = outcomes
            .iter()
            .filter(|o| o.spam && score(o) < threshold)
            .count();
        #[allow(clippy::cast_precision_loss)]
        let (fp_rate, fn_rate) = (
            100.0 * fp as f64 / hams.max(1) as f64,
            100.0 * fn_ as f64 / spams.max(1) as f64,
        );
        eprintln!("  {threshold:>9} | {fp:>5} ({fp_rate:.2} %) | {fn_:>5} ({fn_rate:.2} %)");
    }
}

/// The strongest clues of a mail (test diagnostics).
fn explain(model: &Model, mail: &Mail, score: f64) {
    let mut clues: Vec<(f64, String, SpamCounts)> = mail
        .tokens
        .hashes()
        .iter()
        .map(|h| {
            let counts = model.counts.get(h).copied().unwrap_or_default();
            let name = super::tokens::NAMES
                .with(|names| names.borrow().get(h).cloned())
                .unwrap_or_default();
            (token_probability(model.totals, counts), name, counts)
        })
        .filter(|(p, _, _)| (p - 0.5).abs() >= MIN_PROB_STRENGTH)
        .collect();
    clues.sort_by(|a, b| (b.0 - 0.5).abs().total_cmp(&(a.0 - 0.5).abs()));
    eprintln!("{} (score {score:.4}, {} clues):", mail.name, clues.len());
    let spammy: Vec<String> = clues
        .iter()
        .filter(|c| c.0 > 0.5)
        .take(25)
        .map(|c| format!("{} {:.2} ({}/{})", c.1, c.0, c.2.spam, c.2.ham))
        .collect();
    eprintln!("  spam clues: {}", spammy.join(", "));
}

fn percentile(sorted: &[Duration], p: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let index = (sorted.len() - 1) * p / 100;
    sorted.get(index).copied().unwrap_or_default()
}

#[test]
#[ignore = "needs a corpus: VICE_MAIL_SPAM_CORPUS"]
fn accuracy_on_a_corpus() {
    let Some(root) = std::env::var_os("VICE_MAIL_SPAM_CORPUS") else {
        eprintln!("VICE_MAIL_SPAM_CORPUS is not set");
        return;
    };
    let started = Instant::now();
    let (mut mails, mut times) = read_corpus(Path::new(&root));
    assert!(!mails.is_empty(), "no mail found");
    times.sort();
    let total: Duration = times.iter().sum();
    #[allow(clippy::cast_possible_truncation)]
    let mean = total / times.len() as u32;
    let tokens: usize = mails.iter().map(|m| m.tokens.len()).sum();
    eprintln!(
        "read {} mails in {:.1} s; tokens per mail {:.0}; tokenising per mail: mean {:?}, median {:?}, p95 {:?}, p99 {:?}, max {:?}",
        mails.len(),
        started.elapsed().as_secs_f64(),
        tokens as f64 / mails.len() as f64,
        mean,
        percentile(&times, 50),
        percentile(&times, 95),
        percentile(&times, 99),
        times.last().copied().unwrap_or_default(),
    );
    shuffle(&mut mails, 0x5eed_2026_1126);

    // 1. Ten-fold cross-validation: learn nine tenths, score the tenth.
    let folds = 10;
    let mut outcomes = Vec::with_capacity(mails.len());
    let mut score_times = Vec::with_capacity(mails.len());
    for fold in 0..folds {
        let mut model = Model::default();
        for (i, mail) in mails.iter().enumerate() {
            if i % folds != fold {
                model.learn(mail);
            }
        }
        for (i, mail) in mails.iter().enumerate() {
            if i % folds == fold {
                let started = Instant::now();
                let score = model.score(&mail.tokens);
                score_times.push(started.elapsed());
                outcomes.push(Outcome {
                    index: i,
                    spam: mail.spam,
                    score,
                    fields_score: model.score(&mail.fields),
                });
            }
        }
    }
    score_times.sort();
    eprintln!(
        "scoring per mail (counts in memory): median {:?}, p99 {:?}",
        percentile(&score_times, 50),
        percentile(&score_times, 99)
    );
    report("10-fold cross-validation, whole mails", &outcomes, false);
    report(
        "10-fold cross-validation, only the stored header data and preview",
        &outcomes,
        true,
    );
    let mut worst: Vec<&Outcome> = outcomes.iter().filter(|o| !o.spam).collect();
    worst.sort_by(|a, b| b.score.total_cmp(&a.score));
    eprintln!(
        "highest ham scores: {:?}",
        worst.iter().take(10).map(|o| o.score).collect::<Vec<_>>()
    );
    if std::env::var_os("VICE_MAIL_SPAM_EXPLAIN").is_some() {
        let model = {
            let mut model = Model::default();
            for mail in &mails {
                model.learn(mail);
            }
            model
        };
        for outcome in worst.iter().take(8) {
            let mail = &mails[outcome.index];
            explain(&model, mail, outcome.score);
        }
    }

    // 2. Learning curve: learn n spam and m ham (the newest of a junk folder and an inbox),
    //    score everything else.
    let (spam, ham): (Vec<&Mail>, Vec<&Mail>) = mails.iter().partition(|m| m.spam);
    for (n, m) in [
        (10, 10),
        (20, 20),
        (20, 100),
        (50, 50),
        (50, 200),
        (50, 500),
        (100, 100),
        (100, 500),
        (200, 200),
        (200, 400),
        (200, 1000),
        (500, 500),
        (500, 1000),
        (500, 2000),
    ] {
        if n * 2 > spam.len() || m * 2 > ham.len() {
            continue;
        }
        let mut model = Model::default();
        for mail in spam.iter().take(n).chain(ham.iter().take(m)) {
            model.learn(mail);
        }
        let outcomes: Vec<Outcome> = spam
            .iter()
            .skip(n)
            .chain(ham.iter().skip(m))
            .map(|mail| Outcome {
                index: 0,
                spam: mail.spam,
                score: model.score(&mail.tokens),
                fields_score: model.score(&mail.fields),
            })
            .collect();
        report(&format!("learned {n} spam and {m} ham"), &outcomes, false);
    }
}
