//! Stage 20, task C (decision 0060): the "Attachments" view across accounts, as a method of
//! [`AppCore`]. The store keeps the attachments of every stored body (`attachment`, written
//! with the search index); open, save and preview are the reading pane's commands with the
//! same index, and an attachment whose bytes are not on this computer is fetched then.

use mail_model::{
    AttachmentCursor, AttachmentEntry, AttachmentFilter, AttachmentPage, CoreError,
    MAX_ATTACHMENT_FILTER_CHARS, MAX_ATTACHMENT_PAGE,
};
use mail_store::AttachmentQuery;

use crate::core::{AppCore, Connector};

/// A filter text as the store compares it: trimmed, lower case; `None` when empty.
fn needle(text: Option<String>, what: &str) -> Result<Option<String>, CoreError> {
    let Some(text) = text else { return Ok(None) };
    let text = text.trim();
    if text.chars().count() > MAX_ATTACHMENT_FILTER_CHARS {
        return Err(CoreError::InvalidArgument(format!(
            "the {what} filter has more than {MAX_ATTACHMENT_FILTER_CHARS} characters"
        )));
    }
    Ok((!text.is_empty()).then(|| text.to_lowercase()))
}

impl<C: Connector> AppCore<C> {
    /// One page of the attachments of the scope's accounts, newest message first: by kind,
    /// sender (part of name or address) and file name (part of it), any case. `limit` 1..=500.
    pub async fn list_attachments(
        &self,
        filter: AttachmentFilter,
        cursor: Option<AttachmentCursor>,
        limit: u32,
    ) -> Result<AttachmentPage, CoreError> {
        if limit == 0 || limit > MAX_ATTACHMENT_PAGE {
            return Err(CoreError::InvalidArgument(format!(
                "a page has 1 to {MAX_ATTACHMENT_PAGE} attachments"
            )));
        }
        let (accounts, _) = self.mark_scope(filter.scope).await?;
        let query = AttachmentQuery {
            accounts,
            kind: filter.kind,
            sender: needle(filter.sender, "sender")?,
            name: needle(filter.query, "name")?,
        };
        let (rows, next) = self
            .store_ref()?
            .list_attachments(query, cursor.map(|c| (c.message_id, c.index)), limit)
            .await?;
        let items = rows
            .into_iter()
            .map(|row| AttachmentEntry {
                preview: mail_content::preview_kind(&row.name, &row.mime_type),
                executable: mail_content::is_executable_type(&row.name, &row.mime_type),
                message_id: row.message,
                account_id: row.account,
                mailbox_id: row.mailbox,
                index: row.index,
                name: row.name,
                mime_type: row.mime_type,
                kind: row.kind,
                size: u32::try_from(row.size).unwrap_or(u32::MAX),
                local: row.local,
                from: row.from,
                subject: row.subject,
                date: row.date,
            })
            .collect();
        Ok(AttachmentPage {
            items,
            next_cursor: next.map(|(message_id, index)| AttachmentCursor { message_id, index }),
        })
    }
}
