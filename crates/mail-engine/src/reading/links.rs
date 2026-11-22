//! The real link targets of the last renderings, by render token (stage 3 IPC contract, links).
//!
//! The sanitizer writes every `href` as `https://link.invalid/<token>/<index>`; only the core
//! knows what `<index>` stands for. Tokens are 128 bits from the operating system's random
//! source, so a mail cannot guess the token of another rendering. The register keeps the last
//! [`RENDERINGS`] renderings; an older token is unknown (`not_found`), and the UI reloads the
//! message.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use mail_content::{LinkCheck, LinkTarget, LinkWarning as ContentWarning};
use mail_model::{AccountId, CoreError, LinkInfo, LinkWarning, MessageId, RenderToken};

/// Renderings whose links stay known.
pub const RENDERINGS: usize = 32;

/// One rendering and its links.
#[derive(Debug)]
pub(crate) struct Rendering {
    pub account: AccountId,
    pub message: MessageId,
    pub links: Vec<LinkTarget>,
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
        let provider = rustls::crypto::ring::default_provider();
        if provider.secure_random.fill(&mut bytes).is_err() {
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

    /// Remembers a rendering; the oldest beyond [`RENDERINGS`] is forgotten.
    pub fn insert(&self, token: RenderToken, rendering: Rendering) {
        let mut entries = self.lock();
        entries.retain(|(t, _)| *t != token);
        entries.push_back((token, Arc::new(rendering)));
        while entries.len() > RENDERINGS {
            entries.pop_front();
        }
    }

    pub fn get(&self, token: &RenderToken) -> Option<Arc<Rendering>> {
        self.lock()
            .iter()
            .find(|(t, _)| t == token)
            .map(|(_, r)| Arc::clone(r))
    }

    /// Forgets the renderings of an account (removed).
    pub fn forget_account(&self, account: AccountId) {
        self.lock().retain(|(_, r)| r.account != account);
    }

    /// Link `index` of the rendering `token`, checked.
    pub fn link(
        &self,
        token: &RenderToken,
        index: u32,
    ) -> Result<(LinkTarget, LinkCheck), CoreError> {
        let rendering = self.get(token).ok_or_else(|| {
            CoreError::NotFound(format!("rendering {} (reload the message)", token.as_str()))
        })?;
        let link = rendering
            .links
            .iter()
            .find(|l| l.index == index)
            .cloned()
            .ok_or_else(|| CoreError::NotFound(format!("link {index} of the rendering")))?;
        let check = mail_content::check_link(&link.text, &link.href);
        Ok((link, check))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<(RenderToken, Arc<Rendering>)>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What `describeLink` answers for a checked link.
pub(crate) fn link_info(link: &LinkTarget, check: &LinkCheck) -> LinkInfo {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendering(links: &[(&str, &str)]) -> Rendering {
        Rendering {
            account: "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id"),
            message: MessageId(1),
            links: links
                .iter()
                .enumerate()
                .map(|(i, (href, text))| LinkTarget {
                    index: u32::try_from(i).expect("index"),
                    href: (*href).into(),
                    text: (*text).into(),
                })
                .collect(),
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
        let info = |i| {
            let (link, check) = register.link(&token, i).expect("link");
            link_info(&link, &check)
        };
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
}
