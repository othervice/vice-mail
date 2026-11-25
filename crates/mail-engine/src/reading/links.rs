//! The real link targets of the last renderings, by render token (stage 3 IPC contract, links).
//!
//! The sanitizer writes every `href` as `https://link.invalid/<token>/<index>`; only the core
//! knows what `<index>` stands for. Tokens are 128 bits from the operating system's random
//! source, so a mail cannot guess the token of another rendering. The register keeps the last
//! [`RENDERINGS`] renderings; an older token is unknown (`not_found`), and the UI reloads the
//! message.
//!
//! **Results of AI in the app** (decision 0028) are kept here too: the core finds the links in
//! the text it returns itself ([`FoundLinks`], the same recognition as plain-text mail) and
//! remembers them under a token of their own, so `describeLink`/`openLink` check and open
//! them exactly like the links of a mail. The UI never hands a URL to the core. AI results
//! have their own bound ([`AI_RESULTS`]), so they never push out the renderings of mail.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use mail_content::{LinkCheck, LinkTarget, LinkWarning as ContentWarning};
use mail_model::{
    AccountId, CoreError, EmlFileId, LinkInfo, LinkWarning, MessageId, RenderToken, TextSegment,
};

/// Renderings whose links stay known.
pub const RENDERINGS: usize = 32;
/// AI results whose links stay known, besides the renderings.
pub const AI_RESULTS: usize = 32;

/// What a rendering shows: a stored message, an opened `.eml` file (decision 0027), or the
/// text of an AI result with the accounts whose mail it was made from (decision 0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenderOwner {
    Message {
        account: AccountId,
        message: MessageId,
    },
    File(EmlFileId),
    AiResult {
        accounts: Vec<AccountId>,
    },
}

impl RenderOwner {
    /// Entries of the same kind share one bound.
    fn is_ai(&self) -> bool {
        matches!(self, Self::AiResult { .. })
    }

    fn limit(&self) -> usize {
        if self.is_ai() { AI_RESULTS } else { RENDERINGS }
    }
}

/// The links found in texts the core itself returns (AI results), numbered across all texts of
/// one result. `segments` is the text for display; `link` segments point into the list.
#[derive(Debug, Default)]
pub(crate) struct FoundLinks {
    links: Vec<LinkTarget>,
}

impl FoundLinks {
    /// `text` as segments: `http(s)://`, `www.`, `mailto:` and bare e-mail addresses are
    /// links (as in plain-text mail, `mail_content::render_text`), everything else text.
    pub fn segments(&mut self, text: &str) -> Vec<TextSegment> {
        let rendered = mail_content::render_text(text);
        let offset = u32::try_from(self.links.len()).unwrap_or(u32::MAX);
        let mut segments = super::view::text_segments(&rendered);
        for segment in &mut segments {
            if let TextSegment::Link { index, .. } = segment {
                *index = index.saturating_add(offset);
            }
        }
        self.links
            .extend(rendered.links.into_iter().map(|link| LinkTarget {
                index: link.index.saturating_add(offset),
                ..link
            }));
        segments
    }

    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }
}

/// One rendering and its links.
#[derive(Debug)]
pub(crate) struct Rendering {
    pub owner: RenderOwner,
    pub links: Vec<LinkTarget>,
    /// Stage 20 (decision 0058): links of this rendering lose their tracking parameters before
    /// they are checked, shown and opened (the setting when it was rendered).
    pub clean_links: bool,
}

/// A link of a rendering, checked: the target as it opens (without tracking parameters when
/// the rendering cleans links) and the parameters removed.
#[derive(Debug, Clone)]
pub(crate) struct CheckedLink {
    pub link: LinkTarget,
    pub check: LinkCheck,
    pub removed: Vec<String>,
}

#[derive(Default)]
pub(crate) struct Renderings {
    entries: Mutex<VecDeque<(RenderToken, Arc<Rendering>)>>,
}

impl Renderings {
    /// A fresh token for a new rendering. Never fails: without OS randomness the core could not
    /// even run TLS, so a failure here is a broken system; it is logged and the token falls back
    /// to the clock and a counter (unique, but guessable).
    pub fn new_token() -> RenderToken {
        let mut bytes = [0_u8; 16];
        if mail_imap::tls::provider()
            .secure_random
            .fill(&mut bytes)
            .is_err()
        {
            tracing::error!("no OS randomness for a render token; using a predictable one");
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            bytes = (nanos ^ (u128::from(count) << 64)).to_be_bytes();
        }
        RenderToken::from_random(u128::from_be_bytes(bytes))
    }

    /// Remembers a rendering; the oldest of its kind beyond [`RENDERINGS`] (mail and files) or
    /// [`AI_RESULTS`] is forgotten.
    pub fn insert(&self, token: RenderToken, rendering: Rendering) {
        let ai = rendering.owner.is_ai();
        let limit = rendering.owner.limit();
        let mut entries = self.lock();
        entries.retain(|(t, _)| *t != token);
        entries.push_back((token, Arc::new(rendering)));
        let mut count = entries
            .iter()
            .filter(|(_, r)| r.owner.is_ai() == ai)
            .count();
        while count > limit {
            let Some(oldest) = entries.iter().position(|(_, r)| r.owner.is_ai() == ai) else {
                break;
            };
            entries.remove(oldest);
            count -= 1;
        }
    }

    /// Remembers the links of an AI result under a new token; `None` when it has none.
    pub fn insert_found(
        &self,
        found: FoundLinks,
        accounts: Vec<AccountId>,
        clean_links: bool,
    ) -> Option<RenderToken> {
        if found.is_empty() {
            return None;
        }
        let token = Self::new_token();
        self.insert(
            token.clone(),
            Rendering {
                owner: RenderOwner::AiResult { accounts },
                links: found.links,
                clean_links,
            },
        );
        Some(token)
    }

    pub fn get(&self, token: &RenderToken) -> Option<Arc<Rendering>> {
        self.lock()
            .iter()
            .find(|(t, _)| t == token)
            .map(|(_, r)| Arc::clone(r))
    }

    /// Forgets the renderings of an account (removed), and the AI results made from its mail.
    pub fn forget_account(&self, account: AccountId) {
        self.lock().retain(|(_, r)| match &r.owner {
            RenderOwner::Message { account: a, .. } => *a != account,
            RenderOwner::AiResult { accounts } => !accounts.contains(&account),
            RenderOwner::File(_) => true,
        });
    }

    /// Forgets the renderings of an opened file (its window closed).
    pub fn forget_file(&self, file: &EmlFileId) {
        self.lock()
            .retain(|(_, r)| !matches!(&r.owner, RenderOwner::File(f) if f == file));
    }

    /// Link `index` of the rendering `token`, checked (stage 20: without its tracking
    /// parameters when the rendering cleans links; a redirect of a tracking service is never
    /// followed).
    pub fn link(&self, token: &RenderToken, index: u32) -> Result<CheckedLink, CoreError> {
        let rendering = self.get(token).ok_or_else(|| {
            CoreError::NotFound(format!("rendering {} (reload the message)", token.as_str()))
        })?;
        let link = rendering
            .links
            .iter()
            .find(|l| l.index == index)
            .cloned()
            .ok_or_else(|| CoreError::NotFound(format!("link {index} of the rendering")))?;
        let cleaned = rendering
            .clean_links
            .then(|| mail_content::strip_tracking_parameters(&link.href))
            .flatten();
        let (check, removed) = match cleaned {
            Some(cleaned) => (
                mail_content::check_link(&link.text, &cleaned.url),
                cleaned.removed,
            ),
            None => (mail_content::check_link(&link.text, &link.href), Vec::new()),
        };
        Ok(CheckedLink {
            link,
            check,
            removed,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<(RenderToken, Arc<Rendering>)>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What `describeLink` answers for a checked link.
pub(crate) fn link_info(checked: &CheckedLink) -> LinkInfo {
    let CheckedLink {
        link,
        check,
        removed,
    } = checked;
    fn push(warnings: &mut Vec<LinkWarning>, warning: LinkWarning) {
        // One warning for the host, however many reasons: the UI shows the ASCII form once.
        let duplicate = warnings.contains(&warning)
            || (matches!(warning, LinkWarning::ConfusableHost { .. })
                && warnings
                    .iter()
                    .any(|w| matches!(w, LinkWarning::ConfusableHost { .. })));
        if !duplicate {
            warnings.push(warning);
        }
    }
    let mut warnings: Vec<LinkWarning> = Vec::new();
    for warning in &check.warnings {
        let mapped = match warning {
            ContentWarning::TextNamesOtherDomain {
                text_domain,
                target_domain,
            } => LinkWarning::DomainMismatch {
                shown_domain: text_domain.clone(),
                target_domain: target_domain.clone(),
            },
            ContentWarning::InternationalDomain { ascii: host, .. }
            | ContentWarning::MixedScripts { domain: host }
            | ContentWarning::Confusable { domain: host } => LinkWarning::ConfusableHost {
                ascii_host: host.clone(),
            },
            ContentWarning::IpAddress { .. } => LinkWarning::IpAddressHost,
            ContentWarning::UserInfo { .. } => LinkWarning::CredentialsInUrl,
            ContentWarning::Blocked { .. } | ContentWarning::Invalid => {
                LinkWarning::UnsupportedScheme
            }
        };
        push(&mut warnings, mapped);
    }
    LinkInfo {
        target: check.display.clone(),
        scheme: check.scheme.clone(),
        host: check.host.clone(),
        text: link.text.clone(),
        warnings,
        openable: check.is_openable(),
        tracking_removed: removed.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendering(links: &[(&str, &str)]) -> Rendering {
        Rendering {
            owner: RenderOwner::Message {
                account: "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id"),
                message: MessageId(1),
            },
            links: links
                .iter()
                .enumerate()
                .map(|(i, (href, text))| LinkTarget {
                    index: u32::try_from(i).expect("index"),
                    href: (*href).into(),
                    text: (*text).into(),
                })
                .collect(),
            clean_links: true,
        }
    }

    #[test]
    fn tokens_are_random_and_well_formed() {
        let a = Renderings::new_token();
        let b = Renderings::new_token();
        assert_ne!(a, b);
        assert_eq!(RenderToken::parse(a.as_str()), Some(a));
    }

    #[test]
    fn the_register_keeps_the_last_renderings() {
        let register = Renderings::default();
        let first = Renderings::new_token();
        register.insert(first.clone(), rendering(&[("https://example.org/", "x")]));
        let mut last = first.clone();
        for _ in 0..RENDERINGS {
            last = Renderings::new_token();
            register.insert(last.clone(), rendering(&[]));
        }
        assert!(register.get(&first).is_none(), "the oldest is forgotten");
        assert!(register.get(&last).is_some());
        let err = register.link(&first, 0).expect_err("unknown token");
        assert_eq!(err.code(), mail_model::ErrorCode::NotFound);
        let err = register.link(&last, 0).expect_err("unknown index");
        assert_eq!(err.code(), mail_model::ErrorCode::NotFound);
    }

    #[test]
    fn ai_results_have_their_own_bound_and_go_with_their_accounts() {
        let register = Renderings::default();
        let mail = Renderings::new_token();
        register.insert(mail.clone(), rendering(&[("https://example.org/", "x")]));
        // Not the account of the mail rendering.
        let account: AccountId = "8f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
        let other: AccountId = "7f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
        let result = |accounts: Vec<AccountId>| {
            let mut found = FoundLinks::default();
            found.segments("see https://example.org/a");
            register.insert_found(found, accounts, true).expect("token")
        };
        let first = result(vec![account]);
        let mut last = first.clone();
        for _ in 0..AI_RESULTS {
            last = result(vec![other]);
        }
        assert!(
            register.get(&first).is_none(),
            "the oldest AI result is forgotten"
        );
        assert!(register.get(&last).is_some());
        assert!(
            register.get(&mail).is_some(),
            "AI results never push out mail"
        );
        assert_eq!(
            register.insert_found(FoundLinks::default(), vec![account], true),
            None,
            "no links, no token"
        );
        let kept = result(vec![account, other]);
        register.forget_account(account);
        assert!(
            register.get(&kept).is_none(),
            "made from the removed account's mail"
        );
        assert!(register.get(&last).is_some());
        assert!(
            register.get(&mail).is_some(),
            "the mail belongs to another account"
        );
    }

    #[test]
    fn found_links_are_numbered_across_the_texts_of_a_result() {
        let mut found = FoundLinks::default();
        let first = found.segments("Pay at https://pay.example/x or write to bob@friends.example.");
        let second = found.segments("Plain <img src=https://evil.example/p.png> www.example.org");
        assert_eq!(
            first,
            [
                TextSegment::Text {
                    text: "Pay at ".into()
                },
                TextSegment::Link {
                    text: "https://pay.example/x".into(),
                    index: 0
                },
                TextSegment::Text {
                    text: " or write to ".into()
                },
                TextSegment::Link {
                    text: "bob@friends.example".into(),
                    index: 1
                },
                TextSegment::Text { text: ".".into() },
            ]
        );
        // Markup stays text; an image address in it is a link to check like any other, never
        // an image (nothing in a result is ever loaded).
        assert_eq!(
            second,
            [
                TextSegment::Text {
                    text: "Plain <img src=".into()
                },
                TextSegment::Link {
                    text: "https://evil.example/p.png".into(),
                    index: 2
                },
                TextSegment::Text { text: "> ".into() },
                TextSegment::Link {
                    text: "www.example.org".into(),
                    index: 3
                },
            ]
        );
        let hrefs: Vec<&str> = found.links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(
            hrefs,
            [
                "https://pay.example/x",
                "mailto:bob@friends.example",
                "https://evil.example/p.png",
                "https://www.example.org"
            ]
        );
        assert_eq!(
            found.links.iter().map(|l| l.index).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn warnings_map_to_the_contract() {
        let register = Renderings::default();
        let token = Renderings::new_token();
        register.insert(
            token.clone(),
            rendering(&[
                ("https://evil.example/login", "https://bank.example"),
                ("https://xn--pple-43d.com/", "apple"),
                ("http://192.168.1.1/", ""),
                ("https://bank.example@evil.example/", "Bank"),
                ("javascript:alert(1)", "click"),
                ("mailto:someone@example.org", "write"),
                ("relative/path", "rel"),
            ]),
        );
        let info = |i| link_info(&register.link(&token, i).expect("link"));
        let first = info(0);
        assert_eq!(first.target, "https://evil.example/login");
        assert_eq!(first.host.as_deref(), Some("evil.example"));
        assert_eq!(
            first.warnings,
            [LinkWarning::DomainMismatch {
                shown_domain: "bank.example".into(),
                target_domain: "evil.example".into()
            }]
        );
        assert!(first.openable);

        let idn = info(1);
        assert_eq!(
            idn.warnings
                .iter()
                .filter(|w| matches!(w, LinkWarning::ConfusableHost { .. }))
                .count(),
            1,
            "{:?}",
            idn.warnings
        );
        assert!(idn.warnings.contains(&LinkWarning::ConfusableHost {
            ascii_host: "xn--pple-43d.com".into()
        }));
        assert_eq!(info(2).warnings, [LinkWarning::IpAddressHost]);
        assert!(info(3).warnings.contains(&LinkWarning::CredentialsInUrl));
        let js = info(4);
        assert_eq!(js.warnings, [LinkWarning::UnsupportedScheme]);
        assert!(!js.openable);
        let mail = info(5);
        assert!(mail.openable);
        assert_eq!(mail.scheme, "mailto");
        assert_eq!(mail.host, None);
        assert!(!info(6).openable);
        assert_eq!(info(6).warnings, [LinkWarning::UnsupportedScheme]);
    }

    #[test]
    fn tracking_parameters_go_when_the_rendering_cleans_links() {
        let register = Renderings::default();
        let link = (
            "https://shop.example/a?id=1&utm_source=news&fbclid=x",
            "Shop",
        );
        let clean = Renderings::new_token();
        register.insert(clean.clone(), rendering(&[link]));
        let info = link_info(&register.link(&clean, 0).expect("link"));
        assert_eq!(info.target, "https://shop.example/a?id=1");
        assert_eq!(info.tracking_removed, ["utm_source", "fbclid"]);
        let checked = register.link(&clean, 0).expect("link");
        assert_eq!(
            checked.check.open_url.as_deref(),
            Some("https://shop.example/a?id=1")
        );

        let kept = Renderings::new_token();
        register.insert(
            kept.clone(),
            Rendering {
                clean_links: false,
                ..rendering(&[link])
            },
        );
        let info = link_info(&register.link(&kept, 0).expect("link"));
        assert_eq!(info.target, link.0);
        assert!(info.tracking_removed.is_empty());
    }
}
