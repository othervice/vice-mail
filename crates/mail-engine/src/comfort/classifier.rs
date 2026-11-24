//! The store's [`Classifier`]: the category rules of `mail_content::classify` (decision 0016)
//! over the header block the adapter fetched, or over the stored header data for messages
//! stored before stage 9.

use mail_content::{
    ClassifyHeaders, LocalSignals, SenderIdentity, classify_automatic, normalize_address,
};
use mail_model::MessageHeader;
use mail_store::{Classified, Classifier, LocalFacts, SenderKeys};

/// The rules of `mail_content::classify`.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ContentClassifier;

fn headers_of(header: &MessageHeader) -> ClassifyHeaders {
    if header.category_headers.is_empty() {
        return ClassifyHeaders::from_message_header(header);
    }
    let mut headers = ClassifyHeaders::from_raw(&header.category_headers);
    // The ENVELOPE is decoded already; a block without From or Subject falls back to it.
    if headers.from.is_empty() {
        headers.from.clone_from(&header.from);
    }
    if headers.subject.is_none() && !header.subject.is_empty() {
        headers.subject = Some(header.subject.clone());
    }
    headers
}

/// Whether `email` is one of `own` (case ignored, IDNA, also as `name+tag@`).
pub(crate) fn is_own(email: &str, own: &[String]) -> bool {
    let Some(address) = normalize_address(email) else {
        return false;
    };
    let untagged = address.without_tag();
    own.iter()
        .filter_map(|o| normalize_address(o))
        .any(|o| o.key == address.key || untagged.as_deref() == Some(o.key.as_str()))
}

impl Classifier for ContentClassifier {
    fn sender_keys(&self, header: &MessageHeader) -> SenderKeys {
        let identity = SenderIdentity::of(&headers_of(header));
        SenderKeys {
            address: identity.address,
            domain: identity.domain,
            list_id: identity.list_id,
        }
    }

    fn classify(&self, header: &MessageHeader, local: &LocalFacts<'_>) -> Classified {
        let headers = headers_of(header);
        let replies_to_own = local
            .referenced_senders
            .iter()
            .any(|from| is_own(from, local.own_addresses));
        let decision = classify_automatic(
            &headers,
            &LocalSignals {
                own_addresses: local.own_addresses,
                sent_to_sender: local.sent_to_sender,
                replies_to_own,
            },
        );
        let one_click = (!header.category_headers.is_empty())
            .then(|| super::unsubscribe::one_click_in(&header.category_headers));
        // Stage 17: Reply-To only from the fetched block (the envelope's is not stored).
        let reply_to = if header.category_headers.is_empty() {
            Vec::new()
        } else {
            headers.reply_to
        };
        Classified {
            category: decision.category,
            reason: decision.reason,
            one_click,
            reply_to,
        }
    }
}
