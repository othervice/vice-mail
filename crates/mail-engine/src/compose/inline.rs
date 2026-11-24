//! Stage 10 (decision 0023): inline images of a draft.
//!
//! An inline image is a draft file with a Content-ID: copied in like an attachment (from a path
//! the shell took from the system dialog or a drop on the window, never one the page made up),
//! checked to be a PNG, JPEG, GIF or WebP by its bytes (never SVG), and shown by the editor as
//! `<img src="cid:…">` with the data the core hands out ([`Compose::inline_image_data`]). It
//! goes out as a part of `multipart/related`, and only while the HTML still shows it. The size
//! the user drags to is the `width`/`height` of the `img` element.

use std::path::PathBuf;

use mail_model::{
    CoreError, DraftError, DraftFile, DraftId, DraftState, InlineImagesAdded, MAX_ATTACHMENT_BYTES,
    MAX_ATTACHMENTS, MAX_INLINE_IMAGE_BYTES,
};

use super::{Compose, blocking, files};
use crate::core::Connector;

/// Raster types an inline image may have.
pub(super) const TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// The right side of new Content-IDs: the sender's domain in ASCII, else `inline`.
pub(super) fn content_id_domain(email: &str) -> String {
    let domain = email
        .rsplit_once('@')
        .map(|(_, d)| d.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let ok = !domain.is_empty()
        && domain
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if ok { domain } else { "inline".to_owned() }
}

impl<C: Connector> Compose<C> {
    /// Copies image files into the draft as inline images.
    pub(crate) async fn add_inline_images(
        &self,
        id: DraftId,
        paths: Vec<PathBuf>,
    ) -> Result<InlineImagesAdded, CoreError> {
        let draft = self.stored(&id).await?;
        if draft.state != DraftState::Editing {
            return Err(CoreError::Draft(DraftError::Sending(id)));
        }
        if paths.is_empty() {
            // The dialog was cancelled.
            return Ok(InlineImagesAdded {
                draft: self.info(draft).await?,
                added: Vec::new(),
            });
        }
        if draft.attachments.len() + paths.len() > MAX_ATTACHMENTS {
            return Err(CoreError::InvalidArgument(format!(
                "at most {MAX_ATTACHMENTS} attachments and images"
            )));
        }
        let identities = self.inner.store.compose_identities(draft.account).await?;
        let domain = draft
            .identity
            .and_then(|i| identities.iter().find(|(identity, _)| identity.id == i))
            .or_else(|| identities.first())
            .map(|(identity, _)| content_id_domain(&identity.email))
            .unwrap_or_else(|| "inline".to_owned());
        let mut list = draft.attachments.clone();
        let mut used: u64 = list.iter().map(|a| a.size).sum();
        let mut next = list
            .iter()
            .filter_map(|a| a.id.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        let mut written: Vec<String> = Vec::new();
        let mut added_ids: Vec<String> = Vec::new();
        let cleanup = |written: &[String]| {
            for relative in written {
                files::remove_file(&self.inner.root, relative);
            }
        };
        for path in paths {
            next += 1;
            let file_id = next.to_string();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let limit = MAX_INLINE_IMAGE_BYTES.min(MAX_ATTACHMENT_BYTES.saturating_sub(used));
            let root = self.inner.root.clone();
            let draft_id = id.clone();
            let fid = file_id.clone();
            let copied =
                blocking(move || files::copy_in(&root, &draft_id, &fid, &path, limit)).await?;
            let (size, sha256) = match copied {
                Ok(done) => done,
                Err(err) => {
                    cleanup(&written);
                    return Err(match err {
                        CoreError::Draft(DraftError::AttachmentTooLarge { size, .. }) => {
                            CoreError::Draft(DraftError::AttachmentTooLarge {
                                size,
                                limit: MAX_INLINE_IMAGE_BYTES,
                            })
                        }
                        other => other,
                    });
                }
            };
            let relative = files::attachment_path(&id, &file_id);
            written.push(relative.clone());
            let root = self.inner.root.clone();
            let rel = relative.clone();
            let bytes = blocking(move || files::read(&root, &rel)).await?;
            let mime = match bytes.ok().and_then(|b| mail_content::sniff_image(&b)) {
                Some(kind) if TYPES.contains(&kind.mime()) => kind.mime(),
                _ => {
                    cleanup(&written);
                    return Err(CoreError::InvalidArgument(format!(
                        "{} is not a PNG, JPEG, GIF or WebP image",
                        mail_content::sanitize_filename(&name)
                    )));
                }
            };
            used = used.saturating_add(size);
            let content_id = format!("{:032x}@{domain}", super::random_u128());
            added_ids.push(file_id.clone());
            list.push(DraftFile {
                id: file_id,
                file_name: mail_content::sanitize_filename(&name),
                mime: mime.to_owned(),
                size,
                sha256,
                file: relative,
                content_id: Some(content_id),
            });
        }
        let draft = match self
            .inner
            .store
            .set_draft_attachments(id.clone(), list)
            .await
        {
            Ok(draft) => draft,
            Err(err) => {
                cleanup(&written);
                return Err(err.into());
            }
        };
        self.schedule(id.clone(), self.inner.config.draft_save_delay);
        self.drafts_changed(draft.account, vec![id], Vec::new());
        let added = draft
            .attachments
            .iter()
            .filter(|a| added_ids.contains(&a.id))
            .map(DraftFile::info)
            .collect();
        Ok(InlineImagesAdded {
            draft: self.info(draft).await?,
            added,
        })
    }

    /// An inline image of the draft as a `data:` URL for the editor.
    pub(crate) async fn inline_image_data(
        &self,
        id: DraftId,
        attachment: String,
    ) -> Result<String, CoreError> {
        let draft = self.stored(&id).await?;
        let file = draft
            .attachments
            .iter()
            .find(|a| a.id == attachment && a.content_id.is_some())
            .ok_or_else(|| CoreError::NotFound(format!("inline image {attachment}")))?;
        let root = self.inner.root.clone();
        let relative = file.file.clone();
        let bytes = blocking(move || files::read(&root, &relative)).await??;
        mail_content::image_data_uri(&bytes)
            .ok_or_else(|| CoreError::Internal("the stored image is not an image".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::content_id_domain;

    #[test]
    fn content_ids_name_only_the_sender_domain() {
        assert_eq!(content_id_domain("alice@Example.ORG"), "example.org");
        assert_eq!(content_id_domain("alice@bücher.example"), "inline");
        assert_eq!(content_id_domain("nobody"), "inline");
        assert!(mail_content::is_valid_content_id(&format!(
            "{:032x}@{}",
            7u128,
            content_id_domain("a@b.example")
        )));
    }
}
