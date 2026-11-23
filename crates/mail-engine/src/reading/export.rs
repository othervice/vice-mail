//! Stage 9: the whole message for `.eml` export, a checked page for the browser (unsubscribe
//! pages), and files handed to the system (calendar invitations).

use std::path::PathBuf;
use std::sync::Arc;

use mail_model::{CoreError, MailConnector, MessageId};

use super::fetch::{self, Fetched};
use super::links::Renderings;
use super::{Reading, blocking, files};

/// Largest message fetched for an export (the server's own limits are usually lower).
const MAX_EXPORT_BYTES: u32 = 512 * 1024 * 1024;

impl<C: MailConnector> Reading<C> {
    /// The message exactly as the server has it: the stored body if it is complete, else
    /// fetched whole over the reading connection (not stored).
    pub(crate) async fn raw_message(&self, message: MessageId) -> Result<Vec<u8>, CoreError> {
        if let Some(info) = self.store.body_info(message).await?
            && info.complete
            && let Some(raw) = self.store.load_message(message).await?
        {
            return Ok(raw);
        }
        let summary = self
            .store
            .message_summary(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))?;
        let account = self
            .store
            .account(summary.account_id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {}", summary.account_id)))?;
        match fetch::fetch_body(
            &self.connections,
            &self.store,
            &account,
            message,
            summary.size,
            MAX_EXPORT_BYTES,
        )
        .await?
        {
            Fetched::Whole(raw) => Ok(raw),
            Fetched::Sections(..) => Err(CoreError::InvalidArgument(format!(
                "message {message} is larger than {MAX_EXPORT_BYTES} bytes"
            ))),
            Fetched::NotOnServer => Err(CoreError::NotFound(format!(
                "message {message} is no longer on the server"
            ))),
        }
    }

    /// Hands an `http`/`https` URL the core checked to the system browser.
    pub(crate) async fn open_web_page(&self, url: String) -> Result<(), CoreError> {
        let check = mail_content::check_link("", &url);
        let open = check
            .open_url
            .filter(|_| matches!(check.scheme.as_str(), "http" | "https"));
        let Some(open) = open else {
            return Err(CoreError::InvalidArgument(
                "only http and https pages open in the browser".into(),
            ));
        };
        let opener = Arc::clone(&self.opener);
        blocking(move || opener.open_url(&open)).await??;
        Ok(())
    }

    /// Writes `bytes` as a read-only file `name` into the app's directory for opened files and
    /// hands it to the system's default app (a calendar for `.ics`).
    pub(crate) async fn open_file_copy(
        &self,
        name: String,
        bytes: Vec<u8>,
    ) -> Result<PathBuf, CoreError> {
        let dir = self.open_dir.clone();
        let folder = Renderings::new_token().as_str().to_owned();
        let name = mail_content::sanitize_filename(&name);
        let path =
            blocking(move || files::write_for_opening(&dir, &folder, &name, &bytes)).await??;
        let opener = Arc::clone(&self.opener);
        let opened = path.clone();
        blocking(move || opener.open_path(&opened)).await??;
        Ok(path)
    }

    /// Writes `bytes` to `path`, which the user chose in the system dialog (atomically).
    pub(crate) async fn save_bytes_to(
        &self,
        path: PathBuf,
        bytes: Vec<u8>,
    ) -> Result<PathBuf, CoreError> {
        blocking(move || files::save_atomically(&path, &bytes)).await?
    }
}
