//! Stage 19 (decision 0055): a message's source for the source window, and its header fields
//! for "all headers" in the reading pane.
//!
//! The source is the message as the server has it ([`Reading::raw_message`]: the stored body if
//! it is complete, else fetched whole over the reading connection, not stored), shown as plain
//! text up to [`MAX_SOURCE_BYTES`] (`mail_content::source_text`). The header fields come from
//! the stored body, or the stored header section of a large message; only without either is the
//! message fetched.

use mail_model::{
    BodySection, CoreError, HeaderField, MAX_HEADER_FIELDS, MAX_HEADER_VALUE_CHARS,
    MAX_SOURCE_BYTES, MailConnector, MessageId, MessageSource,
};

use super::{Reading, blocking};
use crate::AppCore;
use crate::core::Connector;

fn usize_of(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

impl<C: MailConnector + 'static> Reading<C> {
    /// See [`AppCore::get_message_source`].
    pub(crate) async fn message_source(
        &self,
        message: MessageId,
    ) -> Result<MessageSource, CoreError> {
        let summary = self
            .store
            .message_summary(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))?;
        let raw = self.raw_message(message).await?;
        let total_bytes = u32::try_from(raw.len()).unwrap_or(u32::MAX);
        let source =
            blocking(move || mail_content::source_text(&raw, usize_of(MAX_SOURCE_BYTES))).await?;
        Ok(MessageSource {
            account_id: summary.account_id,
            message_id: message,
            subject: summary.subject,
            text: source.text,
            total_bytes,
            shown_bytes: u32::try_from(source.shown_bytes).unwrap_or(u32::MAX),
            truncated: source.truncated,
        })
    }

    /// See [`AppCore::get_message_headers`].
    pub(crate) async fn header_fields(
        &self,
        message: MessageId,
    ) -> Result<Vec<HeaderField>, CoreError> {
        let header = match self.store.load_message(message).await? {
            Some(raw) => raw,
            None => {
                let stored = self
                    .store
                    .load_sections(message, Some(vec![BodySection::Header]))
                    .await?
                    .into_iter()
                    .find(|s| s.section == BodySection::Header)
                    .map(|s| s.data);
                match stored {
                    Some(header) => header,
                    None => self.raw_message(message).await?,
                }
            }
        };
        let fields = blocking(move || {
            mail_content::header_fields(
                &header,
                usize_of(MAX_HEADER_FIELDS),
                usize_of(MAX_HEADER_VALUE_CHARS),
            )
        })
        .await?;
        Ok(fields
            .into_iter()
            .map(|f| HeaderField {
                name: f.name,
                value: f.value,
            })
            .collect())
    }
}

impl<C: Connector> AppCore<C> {
    /// Stage 19: the message as plain text for the source window (at most
    /// [`MAX_SOURCE_BYTES`], cut at a line break; `truncated` says so). Fetched whole when it is
    /// not stored complete. `not_found` for an unknown message or one no longer on the server.
    pub async fn get_message_source(&self, message: MessageId) -> Result<MessageSource, CoreError> {
        let source = self.reading_ref()?.message_source(message).await?;
        tracing::info!(
            message = message.0,
            truncated = source.truncated,
            "message source shown"
        );
        Ok(source)
    }

    /// Stage 19: every field of the message's header, in order, for "all headers" (at most
    /// [`MAX_HEADER_FIELDS`], values cut to [`MAX_HEADER_VALUE_CHARS`] characters).
    pub async fn get_message_headers(
        &self,
        message: MessageId,
    ) -> Result<Vec<HeaderField>, CoreError> {
        self.reading_ref()?.header_fields(message).await
    }

    /// Stage 19: the subject for the title of the source window, if the message belongs to
    /// `account`; `not_found` otherwise.
    pub async fn source_window_subject(
        &self,
        account: mail_model::AccountId,
        message: MessageId,
    ) -> Result<String, CoreError> {
        self.message_window_subject(account, message).await
    }
}

#[cfg(test)]
mod tests {
    use mail_model::{BodyFormat, ErrorCode, MAX_SOURCE_BYTES, MessageId};

    use crate::reading::RemoteConfig;
    use crate::reading::tests::Harness;
    use crate::sync::fake::FakeServer;

    const RAW: &[u8] = b"Received: from mx.example.com\r\n\tby mx.example.org\r\n\
From: =?utf-8?q?J=C3=B6rg?= <joerg@example.com>\r\n\
To: user@example.org\r\n\
Subject: Source test\r\n\
X-Trick: a\xe2\x80\xaeb\r\n\
Message-ID: <source.1@example.com>\r\n\
Date: Mon, 23 Nov 2026 12:00:00 +0000\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p onclick=\"x()\">Hello <script>alert(1)</script></p>\r\n";

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_source_is_the_raw_message_as_text_and_headers_come_one_by_one() {
        let server = FakeServer::new();
        server.deliver_raw("INBOX", RAW);
        let mut h = Harness::new(&server, RemoteConfig::default()).await;
        let account = h.add().await;
        let inbox = h.inbox(account).await;
        let message = h.items(inbox).await[0].id;

        // Not stored yet: the source fetches it whole (read-only) and keeps markup as text.
        let source = h.core.get_message_source(message).await.expect("source");
        assert_eq!(source.account_id, account);
        assert_eq!(source.subject, "Source test");
        assert!(!source.truncated);
        assert_eq!(source.total_bytes as usize, RAW.len());
        assert_eq!(source.shown_bytes, source.total_bytes);
        assert!(
            source
                .text
                .contains("<p onclick=\"x()\">Hello <script>alert(1)</script></p>\n")
        );
        assert!(
            source.text.contains("X-Trick: a\u{fffd}b\n"),
            "{}",
            source.text
        );
        assert!(!source.text.contains('\r'));

        // All headers, from the stored body once the pane opened it.
        h.open_fetched(message, BodyFormat::Auto).await;
        let headers = h.core.get_message_headers(message).await.expect("headers");
        let names: Vec<&str> = headers.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Received",
                "From",
                "To",
                "Subject",
                "X-Trick",
                "Message-ID",
                "Date",
                "Content-Type"
            ]
        );
        assert_eq!(headers[0].value, "from mx.example.com by mx.example.org");
        assert_eq!(headers[1].value, "Jörg <joerg@example.com>");
        assert!(server.read(|s| s.access.iter().all(|a| *a == mail_model::Access::ReadOnly)));

        let unknown = h
            .core
            .get_message_source(MessageId(999_999))
            .await
            .expect_err("unknown");
        assert_eq!(unknown.code(), ErrorCode::NotFound);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn large_sources_are_cut_with_the_size_of_the_whole() {
        let server = FakeServer::new();
        let mut raw = b"Subject: big\r\nMessage-ID: <big.1@example.com>\r\n\r\n".to_vec();
        while raw.len() <= MAX_SOURCE_BYTES as usize + 10_000 {
            raw.extend_from_slice(b"0123456789abcdefghijklmnopqrstuvwxyz\r\n");
        }
        server.deliver_raw("INBOX", &raw);
        let mut h = Harness::new(&server, RemoteConfig::default()).await;
        let account = h.add().await;
        let inbox = h.inbox(account).await;
        let message = h.items(inbox).await[0].id;
        let source = h.core.get_message_source(message).await.expect("source");
        assert!(source.truncated);
        assert_eq!(source.total_bytes as usize, raw.len());
        assert!(source.shown_bytes <= MAX_SOURCE_BYTES);
        assert!(source.text.ends_with("xyz\n"), "cut at a line break");
    }
}
