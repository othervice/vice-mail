//! Signatures and templates of the composer (stage 10, decision 0023).
//!
//! - **Signatures** are one pool for all identities. Each identity has a default for new mails
//!   and one for replies and forwards; a draft may switch to any signature
//!   (`DraftUpdate::signatureId`). The HTML goes through the allowlist of outgoing mail
//!   (`mail_content::Signature::from_html`); images are small rasters the core read from files
//!   the user picked in the system dialog or dropped on the window, kept in the signature (base64)
//!   so they sync with it, and sent as `multipart/related` parts the HTML shows as `cid:`.
//! - **Templates**: subject and HTML with placeholders, filled for a draft with its first To
//!   recipient, its identity and today's date (`mail_content::fill_template`).
//! - The stage 5 text signature (`setIdentitySignature`) is a signature with the identity's
//!   UUID that both defaults name.

use std::path::PathBuf;

use mail_content::{InlineImage, LocalDate, Signature, Template, TemplateProblem, TemplateValues};
use mail_model::{
    Address, CoreError, CoreEvent, DraftError, DraftId, FilledTemplateInfo, IdentityId,
    IdentitySignatureDefaults, MAX_SIGNATURE_IMAGE_BYTES, MAX_SIGNATURE_IMAGES,
    MAX_SIGNATURE_IMAGES_BYTES, MAX_SNIPPET_NAME_CHARS, QuoteLanguage, SanitizeReport,
    SavedSignature, SavedTemplate, SignatureId, SignatureImageData, SignatureImageInfo,
    SignatureInfo, SignatureInput, TemplateFillProblem, TemplateId, TemplateInfo, TemplateInput,
};
use mail_store::{
    SignatureRow, Store, StoreError, StoredDraft, StoredSignature, StoredSignatureImage,
    StoredTemplate, TemplateRow,
};

use super::build::Sender;
use crate::AppCore;
use crate::core::Connector;

/// Raster types a signature image may have (never SVG: it can carry scripts and links).
const IMAGE_TYPES: [mail_content::ImageType; 4] = [
    mail_content::ImageType::Png,
    mail_content::ImageType::Jpeg,
    mail_content::ImageType::Gif,
    mail_content::ImageType::Webp,
];

/// The signature a draft uses, ready for the builder.
#[derive(Debug, Clone)]
pub(crate) struct DraftSignature {
    pub(crate) signature: Signature,
    /// Images its HTML shows.
    pub(crate) images: Vec<InlineImage>,
}

/// A stored signature as editor HTML and text (a stage 5 signature has only text).
fn signature_parts(stored: &StoredSignature) -> Signature {
    match &stored.html {
        Some(html) => Signature {
            html: html.clone(),
            text: stored.text.clone(),
        },
        None => Signature::from_text(&stored.text).unwrap_or_default(),
    }
}

fn draft_signature(stored: &StoredSignature) -> DraftSignature {
    let signature = signature_parts(stored);
    let shown = mail_content::inline_image_ids(&signature.html);
    let images = stored
        .images
        .iter()
        .filter(|image| shown.contains(&image.content_id))
        .filter_map(|image| {
            Some(InlineImage {
                content_id: image.content_id.clone(),
                file_name: image.file_name.clone(),
                mime: image.mime.clone(),
                bytes: base64_decode(&image.data_base64)?,
            })
        })
        .collect();
    DraftSignature { signature, images }
}

/// Adds the signature the draft uses to `sender`: the composer's choice if it still exists,
/// else the default of the sender's identity for the draft's kind; none when the draft leaves
/// the signature out.
pub(crate) async fn with_signature(
    store: &Store,
    draft: &StoredDraft,
    mut sender: Sender,
) -> Result<Sender, StoreError> {
    sender.signature = if draft.include_signature {
        store
            .signature_for(
                draft.signature.clone(),
                Some(sender.identity.id),
                draft.kind,
            )
            .await?
            .as_ref()
            .map(draft_signature)
    } else {
        None
    };
    Ok(sender)
}

// ---------------------------------------------------------------------------------------------
// Base64 (the images of a signature are stored and synchronised as text)

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let symbol = |n: u32| char::from(BASE64[(n & 63) as usize]);
    for chunk in bytes.chunks(3) {
        let a = u32::from(chunk[0]);
        let b = chunk.get(1).copied().map_or(0, u32::from);
        let c = chunk.get(2).copied().map_or(0, u32::from);
        let n = a << 16 | b << 8 | c;
        out.push(symbol(n >> 18));
        out.push(symbol(n >> 12));
        out.push(if chunk.len() > 1 { symbol(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { symbol(n) } else { '=' });
    }
    out
}

/// Strict decoder (standard alphabet, padding only at the end); `None` for anything else.
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let mut n: u32 = 0;
        let mut pad = 0;
        for (i, &b) in chunk.iter().enumerate() {
            let value = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' if last && i >= 2 => {
                    pad += 1;
                    0
                }
                _ => return None,
            };
            if pad > 0 && b != b'=' {
                return None;
            }
            n = n << 6 | u32::from(value);
        }
        let [_, x, y, z] = n.to_be_bytes();
        out.push(x);
        if pad < 2 {
            out.push(y);
        }
        if pad < 1 {
            out.push(z);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// Checks and conversions

fn clean_name(name: &str, what: &str) -> Result<String, CoreError> {
    let name: String = name.trim().to_owned();
    if name.is_empty() || name.chars().count() > MAX_SNIPPET_NAME_CHARS {
        return Err(CoreError::InvalidArgument(format!(
            "a {what} name has 1 to {MAX_SNIPPET_NAME_CHARS} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(CoreError::InvalidArgument(format!(
            "a {what} name has no control characters"
        )));
    }
    Ok(name)
}

fn report(notes: mail_content::SanitizeNotes) -> SanitizeReport {
    SanitizeReport {
        images_removed: u32::try_from(notes.images_removed).unwrap_or(u32::MAX),
        formatting_simplified: notes.formatting_simplified,
    }
}

fn signature_info(stored: &StoredSignature) -> SignatureInfo {
    let parts = signature_parts(stored);
    SignatureInfo {
        id: stored.id.clone(),
        name: stored.name.clone(),
        html: parts.html,
        text: parts.text,
        images: stored
            .images
            .iter()
            .map(|image| SignatureImageInfo {
                content_id: image.content_id.clone(),
                file_name: image.file_name.clone(),
                mime: image.mime.clone(),
                size: u32::try_from(image.data_base64.len() / 4 * 3).unwrap_or(u32::MAX),
            })
            .collect(),
        updated_at: stored.updated_at,
    }
}

fn template_info(stored: &StoredTemplate) -> TemplateInfo {
    let template = Template {
        subject: stored.subject.clone(),
        html: stored.html.clone(),
    };
    let (known, unknown) = template.placeholders();
    TemplateInfo {
        id: stored.id.clone(),
        name: stored.name.clone(),
        subject: stored.subject.clone(),
        html: stored.html.clone(),
        placeholders: known.iter().map(|p| p.name().to_owned()).collect(),
        unknown_placeholders: unknown,
        updated_at: stored.updated_at,
    }
}

fn defaults_info(defaults: mail_store::SignatureDefaults) -> IdentitySignatureDefaults {
    IdentitySignatureDefaults {
        identity_id: defaults.identity,
        new_signature: defaults.new,
        reply_signature: defaults.reply,
    }
}

fn image_bytes(image: &StoredSignatureImage) -> u64 {
    u64::try_from(image.data_base64.len() / 4 * 3).unwrap_or(u64::MAX)
}

/// Reads an image file for a signature: at most [`MAX_SIGNATURE_IMAGE_BYTES`], a raster type
/// recognised from the bytes (the extension is not trusted).
fn read_image(path: &std::path::Path) -> Result<(String, &'static str, Vec<u8>), CoreError> {
    let io = |err: std::io::Error| CoreError::InvalidArgument(format!("image unreadable: {err}"));
    let size = std::fs::metadata(path).map_err(io)?.len();
    if size > MAX_SIGNATURE_IMAGE_BYTES {
        return Err(CoreError::Draft(DraftError::AttachmentTooLarge {
            size,
            limit: MAX_SIGNATURE_IMAGE_BYTES,
        }));
    }
    let bytes = std::fs::read(path).map_err(io)?;
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if size > MAX_SIGNATURE_IMAGE_BYTES {
        return Err(CoreError::Draft(DraftError::AttachmentTooLarge {
            size,
            limit: MAX_SIGNATURE_IMAGE_BYTES,
        }));
    }
    let kind = mail_content::sniff_image(&bytes)
        .filter(|kind| IMAGE_TYPES.contains(kind))
        .ok_or_else(|| {
            CoreError::InvalidArgument(
                "a signature image is a PNG, JPEG, GIF or WebP picture".into(),
            )
        })?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok((mail_content::sanitize_filename(&name), kind.mime(), bytes))
}

fn new_content_id() -> String {
    format!("sig-{:032x}@signature", uuid::Uuid::new_v4().as_u128())
}

fn local_date() -> LocalDate {
    let today = jiff::ZonedDateTime::now().date();
    LocalDate {
        year: i32::from(today.year()),
        month: u8::try_from(today.month()).unwrap_or(1),
        day: u8::try_from(today.day()).unwrap_or(1),
    }
}

fn content_language(language: QuoteLanguage) -> mail_content::QuoteLanguage {
    match language {
        QuoteLanguage::En => mail_content::QuoteLanguage::English,
        QuoteLanguage::De => mail_content::QuoteLanguage::German,
    }
}

// ---------------------------------------------------------------------------------------------
// Commands

impl<C: Connector> AppCore<C> {
    fn snippet_store(&self) -> Result<&Store, CoreError> {
        self.store_ref()
    }

    async fn require_signature(&self, id: &SignatureId) -> Result<StoredSignature, CoreError> {
        self.snippet_store()?
            .signature(id.clone())
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("signature {id}")))
    }

    /// Every signature of the pool, in the user's order.
    pub async fn list_signatures(&self) -> Result<Vec<SignatureInfo>, CoreError> {
        Ok(self
            .snippet_store()?
            .signatures()
            .await?
            .iter()
            .map(signature_info)
            .collect())
    }

    /// Creates (`id` null) or changes a signature. The HTML is sanitised with the allowlist of
    /// outgoing mail; it may show only the signature's own images (`cid:`), and images it no
    /// longer shows are removed.
    pub async fn save_signature(&self, input: SignatureInput) -> Result<SavedSignature, CoreError> {
        let store = self.snippet_store()?;
        let name = clean_name(&input.name, "signature")?;
        let existing = match &input.id {
            Some(id) => Some(self.require_signature(id).await?),
            None => None,
        };
        let (signature, notes) = Signature::from_html(&input.html)
            .map_err(|err| CoreError::InvalidArgument(format!("signature: {err}")))?;
        let own: Vec<StoredSignatureImage> = existing
            .as_ref()
            .map(|s| s.images.clone())
            .unwrap_or_default();
        let shown = mail_content::inline_image_ids(&signature.html);
        if let Some(unknown) = shown
            .iter()
            .find(|cid| !own.iter().any(|image| &image.content_id == *cid))
        {
            return Err(CoreError::InvalidArgument(format!(
                "the signature shows an image it does not have: cid:{unknown}"
            )));
        }
        let images: Vec<StoredSignatureImage> = own
            .into_iter()
            .filter(|image| shown.contains(&image.content_id))
            .collect();
        let id = input
            .id
            .unwrap_or_else(|| SignatureId::from_random(uuid::Uuid::new_v4().as_u128()));
        let stored = store
            .put_signature(SignatureRow {
                id,
                name,
                html: Some(signature.html),
                text: signature.text,
                images,
            })
            .await?;
        self.emit_event(CoreEvent::SignaturesChanged);
        tracing::info!(images = stored.images.len(), "signature saved");
        Ok(SavedSignature {
            signature: signature_info(&stored),
            notes: report(notes),
        })
    }

    /// Deletes a signature; defaults and drafts that used it get none.
    pub async fn delete_signature(&self, id: SignatureId) -> Result<(), CoreError> {
        if !self.snippet_store()?.delete_signature(id.clone()).await? {
            return Err(CoreError::NotFound(format!("signature {id}")));
        }
        self.emit_event(CoreEvent::SignaturesChanged);
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(())
    }

    /// The default signatures of an identity.
    pub async fn get_identity_signature_defaults(
        &self,
        identity: IdentityId,
    ) -> Result<IdentitySignatureDefaults, CoreError> {
        Ok(defaults_info(
            self.snippet_store()?.signature_defaults(identity).await?,
        ))
    }

    /// Sets the default signatures of an identity (null = none).
    pub async fn set_identity_signature_defaults(
        &self,
        identity: IdentityId,
        new_signature: Option<SignatureId>,
        reply_signature: Option<SignatureId>,
    ) -> Result<IdentitySignatureDefaults, CoreError> {
        let defaults = self
            .snippet_store()?
            .set_signature_defaults(identity, new_signature, reply_signature)
            .await?;
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(defaults_info(defaults))
    }

    /// Adds images to a signature from files (paths from the system dialog or a drop on the
    /// window, never from the page). Returns the signature; the new images' content ids are the
    /// last ones of `images`. They stay only if the next save shows them.
    pub async fn add_signature_images(
        &self,
        id: SignatureId,
        paths: Vec<PathBuf>,
    ) -> Result<SignatureInfo, CoreError> {
        let stored = self.require_signature(&id).await?;
        if stored.images.len() + paths.len() > MAX_SIGNATURE_IMAGES {
            return Err(CoreError::InvalidArgument(format!(
                "a signature has at most {MAX_SIGNATURE_IMAGES} images"
            )));
        }
        let read = tokio::task::spawn_blocking(move || {
            paths
                .iter()
                .map(|path| read_image(path))
                .collect::<Result<Vec<_>, _>>()
        })
        .await
        .map_err(|err| CoreError::Internal(format!("reading images failed: {err}")))??;
        let mut images = stored.images.clone();
        let mut total: u64 = images.iter().map(image_bytes).sum();
        let mut next = images
            .iter()
            .filter_map(|image| image.id.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        for (file_name, mime, bytes) in read {
            total = total.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
            if total > MAX_SIGNATURE_IMAGES_BYTES {
                return Err(CoreError::Draft(DraftError::AttachmentTooLarge {
                    size: total,
                    limit: MAX_SIGNATURE_IMAGES_BYTES,
                }));
            }
            next += 1;
            images.push(StoredSignatureImage {
                id: next.to_string(),
                content_id: new_content_id(),
                file_name,
                mime: mime.to_owned(),
                data_base64: base64_encode(&bytes),
            });
        }
        let saved = self
            .snippet_store()?
            .put_signature(SignatureRow {
                id: stored.id.clone(),
                name: stored.name.clone(),
                html: stored.html.clone(),
                text: stored.text.clone(),
                images,
            })
            .await?;
        self.emit_event(CoreEvent::SignaturesChanged);
        Ok(signature_info(&saved))
    }

    /// Removes an image (and every `<img>` of the HTML that shows it is left to the next save:
    /// the editor removes it, or the save refuses an unknown `cid:`).
    pub async fn remove_signature_image(
        &self,
        id: SignatureId,
        content_id: String,
    ) -> Result<SignatureInfo, CoreError> {
        let stored = self.require_signature(&id).await?;
        if !stored.images.iter().any(|i| i.content_id == content_id) {
            return Err(CoreError::NotFound(format!("image {content_id}")));
        }
        let images = stored
            .images
            .iter()
            .filter(|i| i.content_id != content_id)
            .cloned()
            .collect();
        let saved = self
            .snippet_store()?
            .put_signature(SignatureRow {
                id: stored.id.clone(),
                name: stored.name.clone(),
                html: stored.html.clone(),
                text: stored.text.clone(),
                images,
            })
            .await?;
        self.emit_event(CoreEvent::SignaturesChanged);
        Ok(signature_info(&saved))
    }

    /// The images of a signature as `data:` URLs for the editor.
    pub async fn signature_image_data(
        &self,
        id: SignatureId,
    ) -> Result<Vec<SignatureImageData>, CoreError> {
        let stored = self.require_signature(&id).await?;
        Ok(stored
            .images
            .iter()
            .map(|image| SignatureImageData {
                content_id: image.content_id.clone(),
                data_url: format!("data:{};base64,{}", image.mime, image.data_base64),
            })
            .collect())
    }

    /// Every template, in the user's order.
    pub async fn list_templates(&self) -> Result<Vec<TemplateInfo>, CoreError> {
        Ok(self
            .snippet_store()?
            .templates()
            .await?
            .iter()
            .map(template_info)
            .collect())
    }

    /// Creates (`id` null) or changes a template; the HTML is sanitised like a signature's and
    /// may not show images.
    pub async fn save_template(&self, input: TemplateInput) -> Result<SavedTemplate, CoreError> {
        let store = self.snippet_store()?;
        let name = clean_name(&input.name, "template")?;
        if let Some(id) = &input.id
            && store.template(id.clone()).await?.is_none()
        {
            return Err(CoreError::NotFound(format!("template {id}")));
        }
        let (template, notes) = Template::from_html(&input.subject, &input.html)
            .map_err(|err| CoreError::InvalidArgument(format!("template: {err}")))?;
        if !mail_content::inline_image_ids(&template.html).is_empty() {
            return Err(CoreError::InvalidArgument(
                "a template has no images; add them in the composer".into(),
            ));
        }
        let id = input
            .id
            .unwrap_or_else(|| TemplateId::from_random(uuid::Uuid::new_v4().as_u128()));
        let stored = store
            .put_template(TemplateRow {
                id,
                name,
                subject: template.subject,
                html: template.html,
            })
            .await?;
        self.emit_event(CoreEvent::TemplatesChanged);
        Ok(SavedTemplate {
            template: template_info(&stored),
            notes: report(notes),
        })
    }

    pub async fn delete_template(&self, id: TemplateId) -> Result<(), CoreError> {
        if !self.snippet_store()?.delete_template(id.clone()).await? {
            return Err(CoreError::NotFound(format!("template {id}")));
        }
        self.emit_event(CoreEvent::TemplatesChanged);
        Ok(())
    }

    /// The template filled for the draft: its first To recipient, its identity and today's date
    /// in the system's time zone. The draft is not changed.
    pub async fn fill_template(
        &self,
        template: TemplateId,
        draft: DraftId,
        language: QuoteLanguage,
    ) -> Result<FilledTemplateInfo, CoreError> {
        let store = self.snippet_store()?;
        let stored = store
            .template(template.clone())
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("template {template}")))?;
        let draft = store
            .draft(draft.clone())
            .await?
            .ok_or(CoreError::Draft(DraftError::NotFound(draft)))?;
        let sender: Option<Address> = match draft.identity {
            Some(identity) => store
                .compose_identities(draft.account)
                .await?
                .into_iter()
                .find(|(i, _)| i.id == identity)
                .map(|(i, _)| Address {
                    name: Some(i.name).filter(|n| !n.trim().is_empty()),
                    email: i.email,
                }),
            None => None,
        };
        let filled = mail_content::fill_template(
            &Template {
                subject: stored.subject.clone(),
                html: stored.html.clone(),
            },
            &TemplateValues {
                recipient: draft.to.first(),
                sender: sender.as_ref(),
                date: Some(local_date()),
                language: content_language(language),
            },
        );
        Ok(FilledTemplateInfo {
            subject: filled.subject,
            html: filled.html,
            text: filled.text,
            problems: filled
                .problems
                .into_iter()
                .map(|problem| match problem {
                    TemplateProblem::Unknown(name) => TemplateFillProblem::Unknown { name },
                    TemplateProblem::Missing(placeholder) => TemplateFillProblem::Missing {
                        placeholder: placeholder.name().to_owned(),
                    },
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_and_is_strict() {
        for len in 0..40usize {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let text = base64_encode(&bytes);
            assert_eq!(base64_decode(&text).as_deref(), Some(bytes.as_slice()));
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_decode("TWE="), Some(b"Ma".to_vec()));
        for bad in ["TWE", "TW=u", "T*Fu", "TWE=TWFu", "===="] {
            assert_eq!(base64_decode(bad), None, "{bad}");
        }
    }

    use std::sync::Arc;
    use std::time::Duration;

    use mail_model::{
        AccountSettings, Draft, DraftUpdate, NewDraft, Secret, Security, ServerSettings,
    };
    use tempfile::TempDir;

    use crate::credentials::{Credentials, MemorySecretStore};
    use crate::sync::SyncConfig;
    use crate::sync::fake::{FakeConnector, FakeServer, PASSWORD};
    use crate::{BuildInfo, CoreConfig, CoreServices};

    /// A tiny file that is a PNG by its first bytes.
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01tiny";

    async fn core(dir: &TempDir) -> (AppCore<FakeConnector>, mail_model::AccountId) {
        let server = FakeServer::new();
        let mut services = CoreServices::with_connector(
            Credentials::new(Arc::new(MemorySecretStore::new())),
            FakeConnector(Arc::clone(&server)),
        );
        services.sync = SyncConfig {
            poll_interval: Duration::from_secs(600),
            settings_sync_auto: false,
            ..SyncConfig::default()
        };
        let core = AppCore::start(
            CoreConfig {
                data_dir: dir.path().join("data"),
                build: BuildInfo {
                    version: "0".into(),
                    commit: None,
                    build_date: None,
                },
            },
            services,
        )
        .await;
        let server = |host: &str, port| ServerSettings {
            host: host.into(),
            port,
            security: Security::Tls,
            username: "alice".into(),
        };
        let account = core
            .add_account(
                AccountSettings {
                    email: "alice@fake.test".into(),
                    display_name: "Alice".into(),
                    imap: server("imap.fake.test", 993),
                    smtp: server("smtp.fake.test", 465),
                },
                Secret::new(PASSWORD.into()),
            )
            .await
            .expect("account")
            .id;
        (core, account)
    }

    fn update(draft: &Draft, signature: Option<SignatureId>, include: bool) -> DraftUpdate {
        DraftUpdate {
            identity_id: draft.identity_id,
            to: draft.to.clone(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Hello".into(),
            format: mail_model::ComposeFormat::Html,
            body_html: "<p>Hi Bob</p>".into(),
            body_text: String::new(),
            include_quote: true,
            quote_position: mail_model::QuotePosition::Above,
            include_signature: include,
            signature_id: signature,
        }
    }

    /// The message the draft sends now, as text.
    async fn built(core: &AppCore<FakeConnector>, draft: &Draft) -> String {
        let store = core.store_for_tests();
        let stored = store
            .draft(draft.id.clone())
            .await
            .expect("read")
            .expect("draft");
        let identities = store.compose_identities(stored.account).await.expect("ids");
        let sender = super::super::build::sender_of(&stored, &identities).expect("sender");
        let sender = with_signature(store, &stored, sender)
            .await
            .expect("signature");
        let root = super::super::files::compose_dir(store);
        let files = super::super::build::files_of(&root, &stored).expect("files");
        let message = super::super::build::outgoing(&stored, &sender, files, 1_764_000_000);
        let built = mail_content::build_message(&message).expect("build");
        // Quoted-printable undone as far as the checks need it.
        String::from_utf8_lossy(&built.for_sending)
            .replace("=\r\n", "")
            .replace("=3D", "=")
            .replace("=20", " ")
    }

    #[tokio::test]
    async fn a_draft_sends_the_default_of_its_kind_or_the_chosen_signature_with_its_images() {
        let dir = TempDir::new().expect("dir");
        let (core, account) = core(&dir).await;
        let identity = core.list_compose_identities(account).await.expect("ids")[0].id;

        // A signature with a logo for new mails, a plain one for replies.
        let logo = core
            .save_signature(SignatureInput {
                id: None,
                name: "Logo".into(),
                html: "<p>Alice Example</p>".into(),
            })
            .await
            .expect("logo")
            .signature;
        let file = dir.path().join("logo.png");
        std::fs::write(&file, PNG).expect("png");
        let with_image = core
            .add_signature_images(logo.id.clone(), vec![file])
            .await
            .expect("image");
        let cid = with_image.images[0].content_id.clone();
        assert!(mail_content::is_valid_content_id(&cid), "{cid}");
        let saved = core
            .save_signature(SignatureInput {
                id: Some(logo.id.clone()),
                name: "Logo".into(),
                html: format!(
                    "<p>Alice Example<br><img src=\"cid:{cid}\" width=\"80\"></p>\
                     <p><a href=\"https://example.org\">example.org</a></p>"
                ),
            })
            .await
            .expect("with image");
        assert_eq!(saved.signature.images.len(), 1);
        let data = core
            .signature_image_data(logo.id.clone())
            .await
            .expect("data");
        assert!(
            data[0]
                .data_url
                .starts_with("data:image/png;base64,iVBORw0KGgo")
        );
        let plain = core
            .save_signature(SignatureInput {
                id: None,
                name: "Short".into(),
                html: "<p>-- </p><p>A.</p>".into(),
            })
            .await
            .expect("plain")
            .signature;
        assert_eq!(plain.text, "A.", "a typed separator is dropped");
        core.set_identity_signature_defaults(
            identity,
            Some(logo.id.clone()),
            Some(plain.id.clone()),
        )
        .await
        .expect("defaults");

        // A new mail: the default for new mails, its image as a related part.
        let draft = core
            .create_draft(NewDraft::New {
                account_id: account,
                identity_id: None,
                to: vec![Address {
                    name: Some("Bob".into()),
                    email: "bob@example.org".into(),
                }],
            })
            .await
            .expect("draft");
        let draft = core
            .save_draft(draft.id.clone(), update(&draft, None, true))
            .await
            .expect("save");
        let text = built(&core, &draft).await;
        assert!(text.contains("multipart/related"), "{text}");
        assert!(text.contains(&format!("Content-ID: <{cid}>")), "{text}");
        assert!(text.contains(&format!("cid:{cid}")), "{text}");
        assert!(text.contains("-- \r\nAlice Example"), "{text}");

        // Switched in the composer: the plain one, no related part.
        let draft = core
            .save_draft(
                draft.id.clone(),
                update(&draft, Some(plain.id.clone()), true),
            )
            .await
            .expect("switch");
        assert_eq!(draft.signature_id, Some(plain.id.clone()));
        let text = built(&core, &draft).await;
        assert!(!text.contains("multipart/related"), "{text}");
        assert!(text.contains("-- \r\nA."), "{text}");

        // Left out: none. Deleting the chosen one falls back to the default.
        let draft = core
            .save_draft(
                draft.id.clone(),
                update(&draft, Some(plain.id.clone()), false),
            )
            .await
            .expect("without");
        assert!(!built(&core, &draft).await.contains("-- \r\n"));
        core.delete_signature(plain.id.clone())
            .await
            .expect("delete");
        let draft = core
            .save_draft(draft.id.clone(), update(&draft, None, true))
            .await
            .expect("default again");
        assert!(built(&core, &draft).await.contains("Alice Example"));

        // A save may not show an image the signature does not have.
        let refused = core
            .save_signature(SignatureInput {
                id: Some(logo.id.clone()),
                name: "Logo".into(),
                html: "<p><img src=\"cid:other@x\"></p>".into(),
            })
            .await;
        assert!(
            matches!(refused, Err(CoreError::InvalidArgument(_))),
            "{refused:?}"
        );
        // An SVG (or anything not a raster) is no signature image.
        let svg = dir.path().join("logo.svg");
        std::fs::write(&svg, b"<svg xmlns='http://www.w3.org/2000/svg'/>").expect("svg");
        assert!(matches!(
            core.add_signature_images(logo.id.clone(), vec![svg]).await,
            Err(CoreError::InvalidArgument(_))
        ));
        let big = dir.path().join("big.png");
        let mut bytes = PNG.to_vec();
        bytes.resize(70 * 1024, 0);
        std::fs::write(&big, bytes).expect("big");
        assert!(matches!(
            core.add_signature_images(logo.id.clone(), vec![big]).await,
            Err(CoreError::Draft(DraftError::AttachmentTooLarge { .. }))
        ));
        core.shutdown().await;
    }

    #[tokio::test]
    async fn templates_are_saved_and_filled_for_a_draft() {
        let dir = TempDir::new().expect("dir");
        let (core, account) = core(&dir).await;
        let saved = core
            .save_template(TemplateInput {
                id: None,
                name: "Thanks".into(),
                subject: "Thanks, {{first_name}}".into(),
                html: "<p>Hallo {{first_name}}, <script>x()</script>{{greeting}}</p>\
                       <p>{{my_email}}</p>"
                    .into(),
            })
            .await
            .expect("template");
        assert_eq!(saved.template.placeholders, ["first_name", "my_email"]);
        assert_eq!(saved.template.unknown_placeholders, ["greeting"]);
        assert!(!saved.template.html.contains("script"));
        let draft = core
            .create_draft(NewDraft::New {
                account_id: account,
                identity_id: None,
                to: vec![Address {
                    name: Some("Dr. Bob Builder".into()),
                    email: "bob@example.org".into(),
                }],
            })
            .await
            .expect("draft");
        let filled = core
            .fill_template(
                saved.template.id.clone(),
                draft.id.clone(),
                QuoteLanguage::De,
            )
            .await
            .expect("filled");
        assert_eq!(filled.subject, "Thanks, Bob");
        assert!(filled.html.contains("Hallo Bob,"), "{}", filled.html);
        assert!(filled.html.contains("alice@fake.test"), "{}", filled.html);
        assert_eq!(
            filled.problems,
            [TemplateFillProblem::Unknown {
                name: "greeting".into()
            }]
        );
        assert_eq!(core.list_templates().await.expect("list").len(), 1);
        core.delete_template(saved.template.id.clone())
            .await
            .expect("delete");
        assert!(matches!(
            core.delete_template(saved.template.id).await,
            Err(CoreError::NotFound(_))
        ));
        assert!(matches!(
            core.save_template(TemplateInput {
                id: None,
                name: " ".into(),
                subject: String::new(),
                html: String::new(),
            })
            .await,
            Err(CoreError::InvalidArgument(_))
        ));
        core.shutdown().await;
    }
}
