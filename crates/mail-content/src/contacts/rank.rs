//! The ranking of contacts for autocomplete: frequency and recency in one number.
//!
//! Every message counts with a weight that halves every [`HALF_LIFE_SECS`] (exponential decay):
//! `score(now) = Σ wᵢ · 2^(−(now − tᵢ) / H)`, with `wᵢ` = [`SENT_WEIGHT`] for a message the user
//! sent to the address and [`RECEIVED_WEIGHT`] for one received from it. Why this form:
//!
//! - Frequency and recency in one sum: someone written to every week settles at about
//!   `1 / (1 − 2^(−7/30)) ≈ 6.7`; a single message yesterday counts about 1; ten messages a
//!   year ago count together less than 0.01.
//! - Memoryless: the score of a contact can be updated with each new message alone, in any
//!   order, without keeping the history.
//! - Time-invariant order: all scores decay by the same factor, so the order of two contacts
//!   changes only when one of them gets a new message. The stored value is therefore the
//!   logarithm `key = log2(score(t)) + t / H` ([`Rank::key`]), which needs no update as time
//!   passes and sorts with a plain index (`ORDER BY rank DESC`). Adding a message or merging two
//!   contacts is a log-sum-exp of keys.
//!
//! The half-life of 30 days favours whom the user writes to now; a sent message weighs four
//! received ones, because autocomplete is about whom the user writes to.

/// Half-life of a message's weight.
pub const HALF_LIFE_SECS: i64 = 30 * 86_400;
/// Weight of a message the user sent to the address.
pub const SENT_WEIGHT: f64 = 1.0;
/// Weight of a message received from the address.
pub const RECEIVED_WEIGHT: f64 = 0.25;

/// A contact's ranking; higher keys rank first. See module docs.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Rank {
    key: f64,
}

impl Default for Rank {
    fn default() -> Self {
        Self::NONE
    }
}

impl Rank {
    /// No messages yet.
    pub const NONE: Self = Self {
        key: f64::NEG_INFINITY,
    };

    /// A stored key (`f64::NEG_INFINITY` or anything not finite: no messages).
    pub fn from_key(key: f64) -> Self {
        if key.is_finite() {
            Self { key }
        } else {
            Self::NONE
        }
    }

    /// The time-independent sort key, in half-lives (see module docs).
    pub fn key(self) -> f64 {
        self.key
    }

    /// One message of weight `weight` at Unix time `at`.
    pub fn of(weight: f64, at: i64) -> Self {
        if !(weight > 0.0 && weight.is_finite()) {
            return Self::NONE;
        }
        Self::from_key(weight.log2() + at as f64 / HALF_LIFE_SECS as f64)
    }

    /// Both rankings together (a message added, or two contacts merged).
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        let (high, low) = if self.key >= other.key {
            (self.key, other.key)
        } else {
            (other.key, self.key)
        };
        if low == f64::NEG_INFINITY {
            return Self { key: high };
        }
        Self::from_key(high + (1.0 + (low - high).exp2()).log2())
    }

    /// The decayed score at `now` (Unix seconds).
    pub fn score_at(self, now: i64) -> f64 {
        if self.key == f64::NEG_INFINITY {
            return 0.0;
        }
        (self.key - now as f64 / HALF_LIFE_SECS as f64).exp2()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const NOW: i64 = 1_795_000_000;

    fn messages(times: &[(f64, i64)]) -> Rank {
        times
            .iter()
            .fold(Rank::NONE, |rank, (w, t)| rank.plus(Rank::of(*w, *t)))
    }

    #[test]
    fn decay_and_order() {
        // One sent message now scores 1, half after one half-life.
        assert!((Rank::of(SENT_WEIGHT, NOW).score_at(NOW) - 1.0).abs() < 1e-9);
        assert!((Rank::of(SENT_WEIGHT, NOW).score_at(NOW + HALF_LIFE_SECS) - 0.5).abs() < 1e-9);
        // Weekly correspondent for a year beats one message yesterday.
        let weekly: Vec<(f64, i64)> = (0..52).map(|w| (SENT_WEIGHT, NOW - w * 7 * DAY)).collect();
        let weekly = messages(&weekly);
        assert!(
            (weekly.score_at(NOW) - 6.7).abs() < 0.2,
            "{}",
            weekly.score_at(NOW)
        );
        let yesterday = Rank::of(SENT_WEIGHT, NOW - DAY);
        assert!(weekly > yesterday);
        // Ten messages a year ago lose against one message yesterday.
        let old: Vec<(f64, i64)> = (0..10)
            .map(|i| (SENT_WEIGHT, NOW - 365 * DAY - i))
            .collect();
        assert!(messages(&old) < yesterday);
        // Sent weighs more than received.
        assert!(Rank::of(SENT_WEIGHT, NOW) > Rank::of(RECEIVED_WEIGHT, NOW));
        assert!(
            (messages(&[(RECEIVED_WEIGHT, NOW); 4]).score_at(NOW) - 1.0).abs() < 1e-9,
            "four received weigh one sent"
        );
    }

    #[test]
    fn order_of_messages_and_merging_do_not_matter() {
        let times = [
            (SENT_WEIGHT, NOW - 3 * DAY),
            (RECEIVED_WEIGHT, NOW - 40 * DAY),
            (SENT_WEIGHT, NOW),
            (RECEIVED_WEIGHT, NOW - 400 * DAY),
        ];
        let forward = messages(&times);
        let mut reversed = times;
        reversed.reverse();
        let backward = messages(&reversed);
        assert!((forward.key() - backward.key()).abs() < 1e-9);
        let merged = messages(&times[..2]).plus(messages(&times[2..]));
        assert!((merged.key() - forward.key()).abs() < 1e-9);
        // The order of two contacts does not change as time passes.
        let a = messages(&times[..2]);
        let b = messages(&times[2..]);
        for later in [0, DAY, 365 * DAY] {
            assert_eq!(
                a.score_at(NOW + later) < b.score_at(NOW + later),
                a < b,
                "{later}"
            );
        }
    }

    #[test]
    fn empty_and_invalid() {
        assert_eq!(Rank::NONE.plus(Rank::NONE), Rank::NONE);
        assert_eq!(Rank::NONE.score_at(NOW), 0.0);
        assert_eq!(Rank::of(0.0, NOW), Rank::NONE);
        assert_eq!(Rank::of(f64::NAN, NOW), Rank::NONE);
        assert_eq!(Rank::from_key(f64::NAN), Rank::NONE);
        let one = Rank::of(SENT_WEIGHT, NOW);
        assert_eq!(Rank::NONE.plus(one), one);
        assert_eq!(Rank::from_key(one.key()), one);
    }
}
