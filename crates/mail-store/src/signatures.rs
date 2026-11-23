//! Signatures and templates (stage 10, decision 0023): one pool of signatures for all
//! identities, the default signatures of every identity (new mails, replies and forwards), the
//! signature a draft switched to, and the templates. The engine sanitises HTML and checks
//! limits of content; the store keeps rows, ids and the counts.
//!
//! A default or a draft's choice that names no signature (deleted, or not arrived from another
//! device yet) means "none": readers resolve it with a join, nothing points into the table by
//! foreign key (the settings sync applies identities before signatures), and deleting a
//! signature leaves the defaults alone (so an edit elsewhere that beats the delete comes back
//! as the default it was).

use mail_model::{
    AccountId, Address, DraftKind, IdentityId, MAX_SIGNATURES, MAX_TEMPLATES, SignatureId,
    TemplateId,
};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::{Value, json};

use crate::settings::now_millis;
use crate::{Store, StoreError};

/// An image of a signature as stored (`signature.images`): the bytes in base64.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSignatureImage {
    /// Unique within the signature.
    pub id: String,
    /// Without angle brackets; the HTML refers to it as `cid:<content_id>`.
    pub content_id: String,
    pub file_name: String,
    pub mime: String,
    pub data_base64: String,
}

impl StoredSignatureImage {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "contentId": self.content_id,
            "fileName": self.file_name,
            "mime": self.mime,
            "dataBase64": self.data_base64,
        })
    }

    pub(crate) fn from_json(value: &Value) -> Option<Self> {
        let text = |field: &str| value.get(field).and_then(Value::as_str).map(str::to_owned);
        Some(Self {
            id: text("id")?,
            content_id: text("contentId")?,
            file_name: text("fileName")?,
            mime: text("mime")?,
            data_base64: text("dataBase64")?,
        })
    }
}

/// The images column as JSON text.
pub(crate) fn images_text(images: &[StoredSignatureImage]) -> String {
    Value::Array(images.iter().map(StoredSignatureImage::to_json).collect()).to_string()
}

/// Images from the column (entries that do not have the shape are left out).
pub(crate) fn images_from_text(text: &str) -> Vec<StoredSignatureImage> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(items)) => items
            .iter()
            .filter_map(StoredSignatureImage::from_json)
            .collect(),
        _ => Vec::new(),
    }
}

/// A signature as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSignature {
    pub id: SignatureId,
    pub name: String,
    /// Allowlisted editor HTML; `None` for a stage 5 signature that only has text.
    pub html: Option<String>,
    pub text: String,
    pub images: Vec<StoredSignatureImage>,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A signature to store (new or replacing the one with this id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureRow {
    pub id: SignatureId,
    pub name: String,
    pub html: Option<String>,
    pub text: String,
    pub images: Vec<StoredSignatureImage>,
}

/// A template as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTemplate {
    pub id: TemplateId,
    pub name: String,
    pub subject: String,
    pub html: String,
    /// Stage 16: recipients the template prefills (checked by the engine).
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A template to store (new or replacing the one with this id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRow {
    pub id: TemplateId,
    pub name: String,
    pub subject: String,
    pub html: String,
    /// Stage 16: recipients to prefill.
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
}

/// The default signatures of an identity, as stored (they may name a signature that is gone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureDefaults {
    pub identity: IdentityId,
    pub account: AccountId,
    pub new: Option<SignatureId>,
    pub reply: Option<SignatureId>,
}

// ---------------------------------------------------------------------------------------------
// SQL

const SIGNATURE_COLUMNS: &str =
    "SELECT uuid, name, html, text, images, position, created_at, updated_at FROM signature";

const UPSERT_SIGNATURE: &str = "
    INSERT INTO signature (uuid, name, html, text, images, position, created_at, updated_at)
    VALUES (?1, ?2, ?3, ?4, ?5,
            (SELECT coalesce(max(position), -1) + 1 FROM signature), ?6, ?6)
    ON CONFLICT (uuid) DO UPDATE SET
      name = excluded.name, html = excluded.html, text = excluded.text,
      images = excluded.images, updated_at = excluded.updated_at";

const TEMPLATE_COLUMNS: &str = "SELECT uuid, name, subject, html, position, created_at, updated_at,
           to_addrs, cc_addrs, bcc_addrs FROM template";

const UPSERT_TEMPLATE: &str = "
    INSERT INTO template (uuid, name, subject, html, position, created_at, updated_at,
                          to_addrs, cc_addrs, bcc_addrs)
    VALUES (?1, ?2, ?3, ?4, (SELECT coalesce(max(position), -1) + 1 FROM template), ?5, ?5,
            ?6, ?7, ?8)
    ON CONFLICT (uuid) DO UPDATE SET
      name = excluded.name, subject = excluded.subject, html = excluded.html,
      to_addrs = excluded.to_addrs, cc_addrs = excluded.cc_addrs,
      bcc_addrs = excluded.bcc_addrs, updated_at = excluded.updated_at";

/// The signature a draft of `kind` from `identity` uses when the composer chose `chosen`
/// (resolved in one statement: the choice if it exists, else the identity's default).
const SIGNATURE_FOR: &str = "
    SELECT uuid, name, html, text, images, position, created_at, updated_at FROM signature
    WHERE uuid = coalesce(
      (SELECT s.uuid FROM signature s WHERE s.uuid = ?1),
      (SELECT CASE WHEN ?3 = 'new' THEN i.signature_new ELSE i.signature_reply END
         FROM identity i WHERE i.id = ?2))";

fn id_at<T>(r: &Row<'_>, idx: usize, parse: fn(&str) -> Option<T>) -> rusqlite::Result<T> {
    let text: String = r.get(idx)?;
    parse(&text).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            idx,
            rusqlite::types::Type::Text,
            format!("not a UUID: {text:?}").into(),
        )
    })
}

fn signature_from_row(r: &Row<'_>) -> rusqlite::Result<StoredSignature> {
    let images: String = r.get(4)?;
    Ok(StoredSignature {
        id: id_at(r, 0, SignatureId::parse)?,
        name: r.get(1)?,
        html: r.get(2)?,
        text: r.get(3)?,
        images: images_from_text(&images),
        position: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

/// Recipients stored as `[{"name", "email"}]` (stage 16).
fn addresses_at(r: &Row<'_>, idx: usize) -> rusqlite::Result<Vec<Address>> {
    let text: String = r.get(idx)?;
    serde_json::from_str(&text)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(err)))
}

pub(crate) fn addresses_text(list: &[Address]) -> Result<String, StoreError> {
    serde_json::to_string(list).map_err(crate::outbox::json_error("encode template addresses"))
}

fn template_from_row(r: &Row<'_>) -> rusqlite::Result<StoredTemplate> {
    Ok(StoredTemplate {
        id: id_at(r, 0, TemplateId::parse)?,
        name: r.get(1)?,
        subject: r.get(2)?,
        html: r.get(3)?,
        position: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        to: addresses_at(r, 7)?,
        cc: addresses_at(r, 8)?,
        bcc: addresses_at(r, 9)?,
    })
}

pub(crate) fn signatures(conn: &Connection) -> Result<Vec<StoredSignature>, StoreError> {
    Ok(conn
        .prepare_cached(&format!("{SIGNATURE_COLUMNS} ORDER BY position, id"))?
        .query_map([], signature_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn signature(
    conn: &Connection,
    id: &SignatureId,
) -> Result<Option<StoredSignature>, StoreError> {
    Ok(conn
        .prepare_cached(&format!("{SIGNATURE_COLUMNS} WHERE uuid = ?1"))?
        .query_row([id.as_str()], signature_from_row)
        .optional()?)
}

pub(crate) fn put_signature(
    conn: &Connection,
    row: &SignatureRow,
    now: i64,
) -> Result<StoredSignature, StoreError> {
    if signature(conn, &row.id)?.is_none() {
        let count: i64 = conn
            .prepare_cached("SELECT count(*) FROM signature")?
            .query_row([], |r| r.get(0))?;
        if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_SIGNATURES {
            return Err(StoreError::InvalidArgument(format!(
                "at most {MAX_SIGNATURES} signatures"
            )));
        }
    }
    conn.prepare_cached(UPSERT_SIGNATURE)?
        .execute(params![
            row.id.as_str(),
            row.name,
            row.html,
            row.text,
            images_text(&row.images),
            now
        ])
        .map_err(StoreError::sqlite("store signature"))?;
    signature(conn, &row.id)?.ok_or_else(|| StoreError::NotFound(format!("signature {}", row.id)))
}

/// Deletes the signature. Defaults that name it are left as they are and mean "none" from now
/// on: clearing them would change the identity's sync record, and a concurrent edit of the
/// signature on another device (which beats the delete) would then come back without being
/// anyone's default. Draft choices (device data) are cleared.
pub(crate) fn delete_signature(conn: &Connection, id: &SignatureId) -> Result<bool, StoreError> {
    let n = conn
        .prepare_cached("DELETE FROM signature WHERE uuid = ?1")?
        .execute([id.as_str()])?;
    conn.prepare_cached("UPDATE draft SET signature_uuid = NULL WHERE signature_uuid = ?1")?
        .execute([id.as_str()])?;
    Ok(n > 0)
}

fn identity_row(conn: &Connection, identity: IdentityId) -> Result<(String, String), StoreError> {
    conn.prepare_cached(
        "SELECT i.uuid, (SELECT a.uuid FROM account a WHERE a.id = i.account_id)
         FROM identity i WHERE i.id = ?1",
    )?
    .query_row([identity.0], |r| Ok((r.get(0)?, r.get(1)?)))
    .optional()?
    .ok_or_else(|| StoreError::NotFound(format!("identity {}", identity.0)))
}

fn account_of(uuid: &str, identity: IdentityId) -> Result<AccountId, StoreError> {
    uuid.parse::<AccountId>()
        .map_err(|_| StoreError::NotFound(format!("identity {}", identity.0)))
}

pub(crate) fn defaults(
    conn: &Connection,
    identity: IdentityId,
) -> Result<SignatureDefaults, StoreError> {
    let (_, account) = identity_row(conn, identity)?;
    let (new, reply): (Option<String>, Option<String>) = conn
        .prepare_cached(
            "SELECT (SELECT s.uuid FROM signature s WHERE s.uuid = i.signature_new),
                    (SELECT s.uuid FROM signature s WHERE s.uuid = i.signature_reply)
             FROM identity i WHERE i.id = ?1",
        )?
        .query_row([identity.0], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(SignatureDefaults {
        identity,
        account: account_of(&account, identity)?,
        new: new.as_deref().and_then(SignatureId::parse),
        reply: reply.as_deref().and_then(SignatureId::parse),
    })
}

pub(crate) fn set_defaults(
    conn: &Connection,
    identity: IdentityId,
    new: Option<&SignatureId>,
    reply: Option<&SignatureId>,
) -> Result<SignatureDefaults, StoreError> {
    identity_row(conn, identity)?;
    for id in new.iter().chain(reply.iter()) {
        if signature(conn, id)?.is_none() {
            return Err(StoreError::NotFound(format!("signature {id}")));
        }
    }
    conn.prepare_cached(
        "UPDATE identity SET signature_new = ?2, signature_reply = ?3
         WHERE id = ?1 AND (signature_new IS NOT ?2 OR signature_reply IS NOT ?3)",
    )?
    .execute(params![
        identity.0,
        new.map(SignatureId::as_str),
        reply.map(SignatureId::as_str)
    ])?;
    defaults(conn, identity)
}

/// The stage 5 text signature of an identity: a signature with the identity's UUID (created,
/// changed, or with `None` removed), and both defaults pointing to it. Returns the account.
pub(crate) fn set_text_signature(
    conn: &Connection,
    identity: IdentityId,
    html: Option<&str>,
    text: Option<&str>,
    now: i64,
) -> Result<AccountId, StoreError> {
    let (uuid, account) = identity_row(conn, identity)?;
    let account = account_of(&account, identity)?;
    let id = SignatureId::parse(&uuid)
        .ok_or_else(|| StoreError::Config(format!("identity UUID {uuid:?}")))?;
    match text {
        Some(text) => {
            let name: String = conn
                .prepare_cached("SELECT email FROM identity WHERE id = ?1")?
                .query_row([identity.0], |r| r.get(0))?;
            let name = match signature(conn, &id)? {
                Some(existing) => existing.name,
                None => name,
            };
            put_signature(
                conn,
                &SignatureRow {
                    id: id.clone(),
                    name,
                    html: html.map(str::to_owned),
                    text: text.to_owned(),
                    images: Vec::new(),
                },
                now,
            )?;
            set_defaults(conn, identity, Some(&id), Some(&id))?;
        }
        None => {
            // Defaults that named this signature now mean "none" (see `delete_signature`);
            // defaults naming other signatures are cleared: the user wants none.
            delete_signature(conn, &id)?;
            conn.prepare_cached(
                "UPDATE identity SET
                   signature_new = CASE WHEN signature_new = ?2 THEN signature_new END,
                   signature_reply = CASE WHEN signature_reply = ?2 THEN signature_reply END
                 WHERE id = ?1
                   AND ((signature_new IS NOT NULL AND signature_new != ?2)
                     OR (signature_reply IS NOT NULL AND signature_reply != ?2))",
            )?
            .execute(params![identity.0, id.as_str()])?;
        }
    }
    Ok(account)
}

pub(crate) fn signature_for(
    conn: &Connection,
    chosen: Option<&SignatureId>,
    identity: Option<IdentityId>,
    kind: DraftKind,
) -> Result<Option<StoredSignature>, StoreError> {
    Ok(conn
        .prepare_cached(SIGNATURE_FOR)?
        .query_row(
            params![
                chosen.map(SignatureId::as_str),
                identity.map(|i| i.0),
                kind.as_str()
            ],
            signature_from_row,
        )
        .optional()?)
}

pub(crate) fn templates(conn: &Connection) -> Result<Vec<StoredTemplate>, StoreError> {
    Ok(conn
        .prepare_cached(&format!("{TEMPLATE_COLUMNS} ORDER BY position, id"))?
        .query_map([], template_from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn template(
    conn: &Connection,
    id: &TemplateId,
) -> Result<Option<StoredTemplate>, StoreError> {
    Ok(conn
        .prepare_cached(&format!("{TEMPLATE_COLUMNS} WHERE uuid = ?1"))?
        .query_row([id.as_str()], template_from_row)
        .optional()?)
}

pub(crate) fn put_template(
    conn: &Connection,
    row: &TemplateRow,
    now: i64,
) -> Result<StoredTemplate, StoreError> {
    if template(conn, &row.id)?.is_none() {
        let count: i64 = conn
            .prepare_cached("SELECT count(*) FROM template")?
            .query_row([], |r| r.get(0))?;
        if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_TEMPLATES {
            return Err(StoreError::InvalidArgument(format!(
                "at most {MAX_TEMPLATES} templates"
            )));
        }
    }
    conn.prepare_cached(UPSERT_TEMPLATE)?
        .execute(params![
            row.id.as_str(),
            row.name,
            row.subject,
            row.html,
            now,
            addresses_text(&row.to)?,
            addresses_text(&row.cc)?,
            addresses_text(&row.bcc)?,
        ])
        .map_err(StoreError::sqlite("store template"))?;
    template(conn, &row.id)?.ok_or_else(|| StoreError::NotFound(format!("template {}", row.id)))
}

pub(crate) fn delete_template(conn: &Connection, id: &TemplateId) -> Result<bool, StoreError> {
    Ok(conn
        .prepare_cached("DELETE FROM template WHERE uuid = ?1")?
        .execute([id.as_str()])?
        > 0)
}

// ---------------------------------------------------------------------------------------------
// API

impl Store {
    /// Every signature, in the user's order.
    pub async fn signatures(&self) -> Result<Vec<StoredSignature>, StoreError> {
        self.read(signatures).await
    }

    pub async fn signature(&self, id: SignatureId) -> Result<Option<StoredSignature>, StoreError> {
        self.read(move |conn| signature(conn, &id)).await
    }

    /// Creates or replaces a signature (content checked by the engine).
    pub async fn put_signature(&self, row: SignatureRow) -> Result<StoredSignature, StoreError> {
        self.write(move |tx| put_signature(tx, &row, now_millis()))
            .await
    }

    /// Deletes a signature; the defaults and draft choices that name it mean "none" from now
    /// on. `false` if there was none.
    pub async fn delete_signature(&self, id: SignatureId) -> Result<bool, StoreError> {
        self.write(move |tx| delete_signature(tx, &id)).await
    }

    /// The default signatures of an identity (only signatures that exist).
    pub async fn signature_defaults(
        &self,
        identity: IdentityId,
    ) -> Result<SignatureDefaults, StoreError> {
        self.read(move |conn| defaults(conn, identity)).await
    }

    /// Sets the default signatures of an identity; `NotFound` for a signature that does not
    /// exist.
    pub async fn set_signature_defaults(
        &self,
        identity: IdentityId,
        new: Option<SignatureId>,
        reply: Option<SignatureId>,
    ) -> Result<SignatureDefaults, StoreError> {
        self.write(move |tx| set_defaults(tx, identity, new.as_ref(), reply.as_ref()))
            .await
    }

    /// The stage 5 text signature (see [`set_text_signature`]): `html` is the editor HTML the
    /// engine derived from `text`.
    pub async fn set_text_signature(
        &self,
        identity: IdentityId,
        html: Option<String>,
        text: Option<String>,
    ) -> Result<AccountId, StoreError> {
        self.write(move |tx| {
            set_text_signature(tx, identity, html.as_deref(), text.as_deref(), now_millis())
        })
        .await
    }

    /// The signature a draft of `kind` from `identity` gets: `chosen` if it exists, else the
    /// identity's default for the kind (new mails: `signature_new`, else `signature_reply`).
    pub async fn signature_for(
        &self,
        chosen: Option<SignatureId>,
        identity: Option<IdentityId>,
        kind: DraftKind,
    ) -> Result<Option<StoredSignature>, StoreError> {
        self.read(move |conn| signature_for(conn, chosen.as_ref(), identity, kind))
            .await
    }

    /// Every template, in the user's order.
    pub async fn templates(&self) -> Result<Vec<StoredTemplate>, StoreError> {
        self.read(templates).await
    }

    pub async fn template(&self, id: TemplateId) -> Result<Option<StoredTemplate>, StoreError> {
        self.read(move |conn| template(conn, &id)).await
    }

    pub async fn put_template(&self, row: TemplateRow) -> Result<StoredTemplate, StoreError> {
        self.write(move |tx| put_template(tx, &row, now_millis()))
            .await
    }

    /// `false` if there was none.
    pub async fn delete_template(&self, id: TemplateId) -> Result<bool, StoreError> {
        self.write(move |tx| delete_template(tx, &id)).await
    }
}
