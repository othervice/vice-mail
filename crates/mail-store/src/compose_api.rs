//! Public API of the store for composing and sending (stage 5, decision 0018): drafts, sends
//! in the outbox, contacts for autocomplete, identity signatures and who files sent mail. Each
//! method is one writer transaction or one read. SQL and rules in `compose` and `contacts`.

use mail_model::{
    AccountId, Address, ComposeFormat, ContactNameRecord, DraftFile, DraftId, DraftKind,
    DraftState, Identity, IdentityId, MessageId, OperationId, QuotePosition, SendJob, SentCopyMode,
    ServerCopy, UnclearDecision,
};
use rusqlite::{OptionalExtension, params};

use crate::outbox_api::{OutboxChange, StoredOperation};
use crate::settings::now_millis;
use crate::{Store, StoreError, accounts, compose, contacts};

/// Most drafts one list returns.
pub const MAX_DRAFTS_PAGE: u32 = 500;

/// A local draft as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDraft {
    pub(crate) row: i64,
    pub(crate) account_row: i64,
    pub id: DraftId,
    pub account: AccountId,
    pub identity: Option<IdentityId>,
    pub kind: DraftKind,
    /// Without angle brackets; fixed when the draft was created.
    pub message_id_header: String,
    /// The answered or forwarded message, while it is stored locally.
    pub original: Option<MessageId>,
    pub original_message_id_header: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub format: ComposeFormat,
    pub body_html: String,
    pub body_text: String,
    pub quote_html: Option<String>,
    pub quote_text: Option<String>,
    pub include_quote: bool,
    pub quote_position: QuotePosition,
    pub include_signature: bool,
    pub attachments: Vec<DraftFile>,
    pub state: DraftState,
    pub revision: i64,
    /// The revision of the copy in the Drafts folder.
    pub server_revision: Option<i64>,
    /// Where the copy in the Drafts folder lies (an own copy).
    pub server_copy: Option<ServerCopy>,
    /// The send operation while the draft is frozen (queued, in flight, failed or unclear).
    pub outgoing: Option<OperationId>,
    /// An append of the draft is queued or in flight.
    pub saving: bool,
    /// The signature the composer switched to (stage 10); `None` = the identity's default.
    pub signature: Option<mail_model::SignatureId>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// The editable fields of a draft, checked by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftContent {
    pub identity: Option<IdentityId>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub format: ComposeFormat,
    pub body_html: String,
    pub body_text: String,
    pub include_quote: bool,
    pub quote_position: QuotePosition,
    pub include_signature: bool,
    /// The signature chosen in the composer (stage 10); `None` = the identity's default.
    pub signature: Option<mail_model::SignatureId>,
}

/// A new draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDraftRow {
    pub id: DraftId,
    pub account: AccountId,
    /// The identity (also in `content`; this one is checked against the account).
    pub identity: Option<IdentityId>,
    pub kind: DraftKind,
    pub message_id_header: String,
    pub original: Option<MessageId>,
    pub original_message_id_header: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub content: DraftContent,
    pub quote_html: Option<String>,
    pub quote_text: Option<String>,
    pub attachments: Vec<DraftFile>,
    /// A draft opened from the Drafts folder: that copy becomes the draft's server copy, so the
    /// first save replaces it.
    pub server_copy: Option<ServerCopy>,
    /// Unix ms.
    pub now: i64,
}

/// One address of the contact directory as stored (decision 0016).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredContact {
    /// `mail_content::NormalizedAddress::key`.
    pub key: String,
    /// The local part as last written.
    pub local_part: String,
    /// ASCII domain.
    pub domain: String,
    pub group: Option<i64>,
    pub sent: u32,
    pub received: u32,
    pub last_sent: Option<i64>,
    pub last_received: Option<i64>,
    /// `mail_content::Rank::key` (`f64::NEG_INFINITY` for none).
    pub rank: f64,
    pub names: Vec<ContactNameRecord>,
    pub user_name: Option<String>,
    /// Folded search words, space-separated.
    pub search: String,
}

/// The header data of a message whose addresses are not in the directory yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactSource {
    pub message: MessageId,
    pub account: AccountId,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub subject: String,
    pub list_id: Option<String>,
    pub list_unsubscribe: Option<String>,
    pub auto_submitted: Option<String>,
    pub precedence: Option<String>,
    /// Sort date, Unix seconds.
    pub date: i64,
}

impl Store {
    // -- drafts ----------------------------------------------------------------------------

    /// Stores a new draft (`revision` 1). `NoIdentity` if the identity is not the account's.
    pub async fn create_draft(&self, draft: NewDraftRow) -> Result<StoredDraft, StoreError> {
        self.write(move |tx| compose::insert_draft(tx, &draft))
            .await
    }

    pub async fn draft(&self, id: DraftId) -> Result<Option<StoredDraft>, StoreError> {
        self.read(move |conn| compose::draft(conn, &id)).await
    }

    /// The account's draft with this Message-ID (a message of the Drafts folder that is one of
    /// the app's own drafts).
    pub async fn draft_for_message(
        &self,
        account: AccountId,
        message_id_header: String,
    ) -> Result<Option<StoredDraft>, StoreError> {
        self.read(move |conn| compose::draft_for_message(conn, account, &message_id_header))
            .await
    }

    /// The account's drafts, newest change first. `limit` 1..=[`MAX_DRAFTS_PAGE`].
    pub async fn drafts(
        &self,
        account: AccountId,
        limit: u32,
    ) -> Result<Vec<StoredDraft>, StoreError> {
        if limit == 0 || limit > MAX_DRAFTS_PAGE {
            return Err(StoreError::InvalidArgument(format!(
                "drafts limit {limit} is outside 1..={MAX_DRAFTS_PAGE}"
            )));
        }
        self.read(move |conn| compose::drafts(conn, account, limit))
            .await
    }

    /// Drafts being edited whose newest revision is not on the server yet (after a restart).
    pub async fn drafts_to_save(&self) -> Result<Vec<DraftId>, StoreError> {
        self.read(compose::drafts_to_save).await
    }

    /// Saves the editable fields and raises the revision. `Sending` while the draft is frozen.
    pub async fn save_draft(
        &self,
        id: DraftId,
        content: DraftContent,
    ) -> Result<StoredDraft, StoreError> {
        let now = now_millis();
        self.write(move |tx| compose::save_draft(tx, &id, &content, now))
            .await
    }

    /// Replaces the attachment list and raises the revision. `Sending` while frozen.
    pub async fn set_draft_attachments(
        &self,
        id: DraftId,
        attachments: Vec<DraftFile>,
    ) -> Result<StoredDraft, StoreError> {
        let now = now_millis();
        self.write(move |tx| compose::set_attachments(tx, &id, &attachments, now))
            .await
    }

    /// Deletes a draft that is not being sent; its queued appends are cancelled and its copy
    /// in the Drafts folder is removed through the outbox (with writes on).
    pub async fn delete_draft(
        &self,
        id: DraftId,
        request: String,
    ) -> Result<(StoredDraft, OutboxChange), StoreError> {
        let now = now_millis();
        self.write(move |tx| compose::delete_draft(tx, &id, &request, now))
            .await
    }

    /// Queues the newest version of the draft for its account's Drafts folder. `None` when
    /// nothing is queued (frozen, writes off, no Drafts folder, nothing new, or a queued append
    /// builds the newest version anyway).
    pub async fn enqueue_draft_save(
        &self,
        id: DraftId,
        request: String,
    ) -> Result<Option<OutboxChange>, StoreError> {
        let now = now_millis();
        self.write(move |tx| compose::enqueue_save(tx, &id, &request, now))
            .await
    }

    /// Freezes the draft and records its `send` operation, executed from `not_before` on.
    /// `ReadOnly` without writes, `Sending` if it is frozen already; the same request twice
    /// changes nothing.
    pub async fn enqueue_send(
        &self,
        id: DraftId,
        job: SendJob,
        not_before: Option<i64>,
        request: String,
    ) -> Result<OutboxChange, StoreError> {
        let now = now_millis();
        self.write(move |tx| compose::enqueue_send(tx, &id, job, not_before, &request, now))
            .await
    }

    /// Takes the draft out of the outbox: a queued send is deleted (nothing went out), a failed
    /// or unclear one discarded; the draft is editable again. `TooLate` once the submission
    /// started.
    pub async fn cancel_send(&self, id: DraftId) -> Result<OutboxChange, StoreError> {
        let now = now_millis();
        self.write(move |tx| compose::cancel_send(tx, &id, now))
            .await
    }

    /// The user's decision on an unclear send: finish it as sent, or queue it once more (same
    /// Message-ID, same operation).
    pub async fn resolve_unclear_send(
        &self,
        id: OperationId,
        decision: UnclearDecision,
    ) -> Result<OutboxChange, StoreError> {
        let now = now_millis();
        self.write(move |tx| compose::resolve_unclear(tx, id, decision, now))
            .await
    }

    /// The account's send operations, newest first, below `before`. `limit` 1..=500.
    pub async fn sends(
        &self,
        account: AccountId,
        before: Option<OperationId>,
        limit: u32,
    ) -> Result<Vec<StoredOperation>, StoreError> {
        if limit == 0 || limit > crate::MAX_OPERATIONS_PAGE {
            return Err(StoreError::InvalidArgument(format!(
                "sends limit {limit} is outside 1..={}",
                crate::MAX_OPERATIONS_PAGE
            )));
        }
        self.read(move |conn| compose::sends(conn, account, before, limit))
            .await
    }

    /// Payloads of every unfinished send and append (state other than `confirmed`), for the
    /// sweep of the compose files at start.
    pub async fn unfinished_compose_payloads(
        &self,
    ) -> Result<Vec<mail_model::OperationPayload>, StoreError> {
        self.read(contacts::unfinished_payloads).await
    }

    /// The account's selectable mailbox with `role`, with its wire name.
    pub async fn mailbox_with_role(
        &self,
        account: AccountId,
        role: mail_model::MailboxRole,
    ) -> Result<Option<mail_model::TargetMailbox>, StoreError> {
        self.read(move |conn| {
            let row = accounts::row_id(conn, account)?;
            compose::mailbox_with_role(conn, row, role)
        })
        .await
    }

    // -- identities and accounts -----------------------------------------------------------

    /// The account's identities with their text signatures, the default first.
    pub async fn compose_identities(
        &self,
        account: AccountId,
    ) -> Result<Vec<(Identity, Option<String>)>, StoreError> {
        self.read(move |conn| contacts::compose_identities(conn, account))
            .await
    }

    /// Sets (or with `None` removes) the identity's text signature; returns its account.
    pub async fn set_identity_signature(
        &self,
        identity: IdentityId,
        signature: Option<String>,
    ) -> Result<AccountId, StoreError> {
        self.write(move |tx| contacts::set_signature(tx, identity, signature.as_deref()))
            .await
    }

    /// Who files sent mail of the account (`Auto` when the user never chose).
    pub async fn sent_copy_mode(&self, account: AccountId) -> Result<SentCopyMode, StoreError> {
        self.read(move |conn| {
            let row = accounts::row_id(conn, account)?;
            let mode: Option<String> = conn
                .prepare_cached("SELECT sent_copy FROM account WHERE id = ?1")?
                .query_row([row], |r| r.get(0))
                .optional()?
                .flatten();
            Ok(mode
                .as_deref()
                .and_then(SentCopyMode::parse)
                .unwrap_or(SentCopyMode::Auto))
        })
        .await
    }

    pub async fn set_sent_copy_mode(
        &self,
        account: AccountId,
        mode: SentCopyMode,
    ) -> Result<(), StoreError> {
        self.write(move |tx| {
            let row = accounts::row_id(tx, account)?;
            let value = match mode {
                SentCopyMode::Auto => None,
                other => Some(other.as_str()),
            };
            tx.prepare_cached("UPDATE account SET sent_copy = ?2 WHERE id = ?1")?
                .execute(params![row, value])
                .map_err(StoreError::sqlite("set sent copy mode"))?;
            Ok(())
        })
        .await
    }

    // -- contacts --------------------------------------------------------------------------

    /// The stored contacts with these address keys.
    pub async fn contacts_by_keys(
        &self,
        keys: Vec<String>,
    ) -> Result<Vec<StoredContact>, StoreError> {
        self.read(move |conn| contacts::by_keys(conn, &keys)).await
    }

    /// Writes changed contacts and marks the messages they came from as read into the
    /// directory, in one transaction.
    pub async fn record_contacts(
        &self,
        contacts: Vec<StoredContact>,
        seen: Vec<MessageId>,
    ) -> Result<(), StoreError> {
        let now = now_millis();
        self.write(move |tx| contacts::record(tx, &contacts, &seen, now))
            .await
    }

    /// Contacts whose search words start with every one of `words` (folded by the caller), by
    /// rank. At most 8 words; `limit` 1..=[`mail_model::MAX_CONTACT_SUGGESTIONS`].
    pub async fn search_contacts(
        &self,
        words: Vec<String>,
        limit: u32,
    ) -> Result<Vec<StoredContact>, StoreError> {
        if limit == 0 || limit > mail_model::MAX_CONTACT_SUGGESTIONS {
            return Err(StoreError::InvalidArgument(format!(
                "contact limit {limit} is outside 1..={}",
                mail_model::MAX_CONTACT_SUGGESTIONS
            )));
        }
        self.read(move |conn| contacts::search(conn, &words, limit))
            .await
    }

    /// Messages whose addresses are not in the directory yet, oldest id first.
    pub async fn contacts_todo(&self, limit: u32) -> Result<Vec<ContactSource>, StoreError> {
        self.read(move |conn| contacts::todo(conn, limit)).await
    }
}
