//! Signatures and templates (stage 10, decision 0023). Each command forwards to [`AppCore`];
//! the image dialog and the check of dropped paths are done here, because they need the
//! window (like attachments in `compose.rs`).

use mail_engine::AppCore;
use mail_model::{
    CoreError, DraftId, FilledTemplateInfo, IdentityId, IdentitySignatureDefaults, IpcError,
    QuoteLanguage, SavedSignature, SavedTemplate, SignatureId, SignatureImageData, SignatureInfo,
    SignatureInput, TemplateId, TemplateInfo, TemplateInput,
};
use tauri::{Runtime, State, WebviewWindow};

use super::to_ipc;
use crate::drops::DropRegistry;

/// Every signature of the pool, in the user's order.
#[tauri::command]
#[specta::specta]
pub async fn list_signatures(core: State<'_, AppCore>) -> Result<Vec<SignatureInfo>, IpcError> {
    core.list_signatures()
        .await
        .map_err(to_ipc("list_signatures"))
}

/// Creates (`id` null) or changes a signature; the HTML is sanitised and may show only the
/// signature's own `cid:` images.
#[tauri::command]
#[specta::specta]
pub async fn save_signature(
    core: State<'_, AppCore>,
    signature: SignatureInput,
) -> Result<SavedSignature, IpcError> {
    core.save_signature(signature)
        .await
        .map_err(to_ipc("save_signature"))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_signature(
    core: State<'_, AppCore>,
    signature_id: SignatureId,
) -> Result<(), IpcError> {
    core.delete_signature(signature_id)
        .await
        .map_err(to_ipc("delete_signature"))
}

#[tauri::command]
#[specta::specta]
pub async fn get_identity_signature_defaults(
    core: State<'_, AppCore>,
    identity_id: IdentityId,
) -> Result<IdentitySignatureDefaults, IpcError> {
    core.get_identity_signature_defaults(identity_id)
        .await
        .map_err(to_ipc("get_identity_signature_defaults"))
}

/// The default signatures of an identity for new mails and for replies and forwards.
#[tauri::command]
#[specta::specta]
pub async fn set_identity_signature_defaults(
    core: State<'_, AppCore>,
    identity_id: IdentityId,
    new_signature: Option<SignatureId>,
    reply_signature: Option<SignatureId>,
) -> Result<IdentitySignatureDefaults, IpcError> {
    core.set_identity_signature_defaults(identity_id, new_signature, reply_signature)
        .await
        .map_err(to_ipc("set_identity_signature_defaults"))
}

/// Lets the user pick images in the system dialog and adds them to the signature; cancelling
/// changes nothing. The new images are the last ones of `images`; insert them as
/// `<img src="cid:CONTENT_ID">` and save the signature.
#[tauri::command]
#[specta::specta]
pub async fn pick_signature_images<R: Runtime>(
    window: WebviewWindow<R>,
    core: State<'_, AppCore>,
    signature_id: SignatureId,
) -> Result<SignatureInfo, IpcError> {
    let paths = ask_image_paths(&window)
        .await
        .map_err(to_ipc("pick_signature_images"))?;
    if paths.is_empty() {
        return core
            .list_signatures()
            .await
            .map_err(to_ipc("pick_signature_images"))?
            .into_iter()
            .find(|s| s.id == signature_id)
            .ok_or_else(|| {
                to_ipc("pick_signature_images")(CoreError::NotFound(format!(
                    "signature {signature_id}"
                )))
            });
    }
    core.add_signature_images(signature_id, paths)
        .await
        .map_err(to_ipc("pick_signature_images"))
}

/// Adds images dropped on the window: only paths of a drop the shell saw itself, shortly
/// before (`invalid_argument` otherwise; see `drops.rs`).
#[tauri::command]
#[specta::specta]
pub async fn add_dropped_signature_images(
    core: State<'_, AppCore>,
    drops: State<'_, DropRegistry>,
    signature_id: SignatureId,
    paths: Vec<String>,
) -> Result<SignatureInfo, IpcError> {
    let paths = drops.take(&paths).map_err(|path| {
        to_ipc("add_dropped_signature_images")(CoreError::InvalidArgument(format!(
            "{path} was not dropped on the window"
        )))
    })?;
    core.add_signature_images(signature_id, paths)
        .await
        .map_err(to_ipc("add_dropped_signature_images"))
}

#[tauri::command]
#[specta::specta]
pub async fn remove_signature_image(
    core: State<'_, AppCore>,
    signature_id: SignatureId,
    content_id: String,
) -> Result<SignatureInfo, IpcError> {
    core.remove_signature_image(signature_id, content_id)
        .await
        .map_err(to_ipc("remove_signature_image"))
}

/// The signature's images as `data:` URLs, to show them in the editor in place of `cid:`.
#[tauri::command]
#[specta::specta]
pub async fn signature_image_data(
    core: State<'_, AppCore>,
    signature_id: SignatureId,
) -> Result<Vec<SignatureImageData>, IpcError> {
    core.signature_image_data(signature_id)
        .await
        .map_err(to_ipc("signature_image_data"))
}

#[tauri::command]
#[specta::specta]
pub async fn list_templates(core: State<'_, AppCore>) -> Result<Vec<TemplateInfo>, IpcError> {
    core.list_templates()
        .await
        .map_err(to_ipc("list_templates"))
}

#[tauri::command]
#[specta::specta]
pub async fn save_template(
    core: State<'_, AppCore>,
    template: TemplateInput,
) -> Result<SavedTemplate, IpcError> {
    core.save_template(template)
        .await
        .map_err(to_ipc("save_template"))
}

#[tauri::command]
#[specta::specta]
pub async fn delete_template(
    core: State<'_, AppCore>,
    template_id: TemplateId,
) -> Result<(), IpcError> {
    core.delete_template(template_id)
        .await
        .map_err(to_ipc("delete_template"))
}

/// The template filled for the draft (first To recipient, identity, today's date); the draft
/// itself is not changed.
#[tauri::command]
#[specta::specta]
pub async fn fill_template(
    core: State<'_, AppCore>,
    template_id: TemplateId,
    draft_id: DraftId,
    language: QuoteLanguage,
) -> Result<FilledTemplateInfo, IpcError> {
    core.fill_template(template_id, draft_id, language)
        .await
        .map_err(to_ipc("fill_template"))
}

/// The system's open dialog for images, as a sheet of the window. Empty when the user cancels.
#[cfg(any(target_os = "macos", windows))]
async fn ask_image_paths<R: Runtime>(
    window: &WebviewWindow<R>,
) -> Result<Vec<std::path::PathBuf>, CoreError> {
    Ok(rfd::AsyncFileDialog::new()
        .set_parent(window)
        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
        .pick_files()
        .await
        .map(|files| files.iter().map(|f| f.path().to_path_buf()).collect())
        .unwrap_or_default())
}

#[cfg(not(any(target_os = "macos", windows)))]
async fn ask_image_paths<R: Runtime>(
    _window: &WebviewWindow<R>,
) -> Result<Vec<std::path::PathBuf>, CoreError> {
    Err(CoreError::Internal(
        "adding images needs the system dialog of macOS or Windows".into(),
    ))
}
