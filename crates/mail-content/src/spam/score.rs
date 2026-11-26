//! The score of a message from the learned counts of its tokens (decision 0077).
//!
//! For each token, Robinson's probability `f(w) = (s·x + n·p(w)) / (s + n)`, where `p(w)` is the
//! share of spam among the classes' relative frequencies of the token and `n` the number of
//! learned mails that had it; `s` and `x` pull rare tokens towards "unknown". The tokens whose
//! `f(w)` lies at least [`MIN_PROB_STRENGTH`] away from 0.5 count, at most
//! [`MAX_DISCRIMINATORS`] of the strongest. Fisher's method combines them twice, once for the
//! hypothesis "spam" and once for "ham"; the score is `(1 + S − H) / 2`. Values near 1 are
//! spam, near 0 ham, around 0.5 undecided (both or neither hypothesis fits).

/// Robinson's `s`: how strongly a token's probability is pulled towards
/// [`UNKNOWN_WORD_PROB`] (SpamBayes default).
pub const UNKNOWN_WORD_STRENGTH: f64 = 0.45;
/// Robinson's `x`: the probability of a token never seen (SpamBayes default).
pub const UNKNOWN_WORD_PROB: f64 = 0.5;
/// Tokens whose probability lies closer than this to 0.5 are left out (SpamBayes default).
pub const MIN_PROB_STRENGTH: f64 = 0.1;
/// At most this many tokens, the strongest, decide (SpamBayes default).
pub const MAX_DISCRIMINATORS: usize = 150;

/// How many learned spam and ham mails had a token.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpamCounts {
    pub spam: u32,
    pub ham: u32,
}

/// How many mails were learned as spam and as ham.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpamTotals {
    pub spam: u32,
    pub ham: u32,
}

/// The result of [`spam_score`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpamScore {
    /// 0 (ham) to 1 (spam).
    pub score: f64,
    /// How many tokens decided it.
    pub clues: u32,
}

/// Robinson's probability that a mail with this token is spam.
pub fn token_probability(totals: SpamTotals, counts: SpamCounts) -> f64 {
    let spam = f64::from(counts.spam);
    let ham = f64::from(counts.ham);
    let n = spam + ham;
    if n == 0.0 {
        return UNKNOWN_WORD_PROB;
    }
    let spam_ratio = spam / f64::from(totals.spam.max(1));
    let ham_ratio = ham / f64::from(totals.ham.max(1));
    let p = spam_ratio / (spam_ratio + ham_ratio);
    (UNKNOWN_WORD_STRENGTH * UNKNOWN_WORD_PROB + n * p) / (UNKNOWN_WORD_STRENGTH + n)
}

/// The score of a message whose tokens have these learned counts (one entry per token; tokens
/// never learned may be left out or given as zero counts). `None` while either class has no
/// learned mail: then nothing can be said.
pub fn spam_score(
    totals: SpamTotals,
    counts: impl IntoIterator<Item = SpamCounts>,
) -> Option<SpamScore> {
    if totals.spam == 0 || totals.ham == 0 {
        return None;
    }
    let mut clues: Vec<f64> = counts
        .into_iter()
        .map(|c| token_probability(totals, c))
        .filter(|p| (p - 0.5).abs() >= MIN_PROB_STRENGTH)
        .collect();
    if clues.len() > MAX_DISCRIMINATORS {
        // The strongest first; only their set matters, not their order.
        clues.select_nth_unstable_by(MAX_DISCRIMINATORS - 1, |a, b| {
            (b - 0.5).abs().total_cmp(&(a - 0.5).abs())
        });
        clues.truncate(MAX_DISCRIMINATORS);
    }
    let n = clues.len();
    if n == 0 {
        return Some(SpamScore {
            score: 0.5,
            clues: 0,
        });
    }
    // Logarithms of the products ∏ f and ∏ (1 − f); every f lies strictly between 0 and 1.
    let (mut ln_ham, mut ln_spam) = (0.0_f64, 0.0_f64);
    for p in &clues {
        ln_ham += p.ln();
        ln_spam += (1.0 - p).ln();
    }
    let spam = 1.0 - chi2q(-2.0 * ln_spam, 2 * n);
    let ham = 1.0 - chi2q(-2.0 * ln_ham, 2 * n);
    let score = ((spam - ham + 1.0) / 2.0).clamp(0.0, 1.0);
    Some(SpamScore {
        score,
        clues: u32::try_from(n).unwrap_or(u32::MAX),
    })
}

/// The probability that a chi-square distributed value with `v` (even) degrees of freedom is at
/// least `x2`.
pub(crate) fn chi2q(x2: f64, v: usize) -> f64 {
    let m = x2 / 2.0;
    let mut term = (-m).exp();
    let mut sum = term;
    let mut i = 1.0;
    for _ in 1..v / 2 {
        term *= m / i;
        sum += term;
        i += 1.0;
    }
    sum.min(1.0)
}
