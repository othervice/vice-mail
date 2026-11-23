//! Signatures and templates of the composer (stage 10, decision 0023): the types that cross
//! the bridge. Signatures are one pool for all identities; every identity has a default for new
//! mails and one for replies and forwards, and a draft can switch to any signature.

use std::fmt;

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::IdentityId;

/// Most signatures in the pool.
pub const MAX_SIGNATURES: usize = 200;
/// Most templates.
pub const MAX_TEMPLATES: usize = 500;
/// Longest name of a signature or template, in characters.
pub const MAX_SNIPPET_NAME_CHARS: usize = 100;
/// Most images of one signature.
pub const MAX_SIGNATURE_IMAGES: usize = 8;
/// Largest image of a signature (the file as read).
pub const MAX_SIGNATURE_IMAGE_BYTES: u64 = 64 * 1024;
/// Largest total of a signature's images; with the HTML it fits a sync record (256 KiB).
pub const MAX_SIGNATURE_IMAGES_BYTES: u64 = 128 * 1024;

/// A lower-case hyphenated UUID, shared by [`SignatureId`] and [`TemplateId`].
fn parse_uuid(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    shaped.then(|| text.to_ascii_lowercase())
}

fn uuid_from_random(bits: u128) -> String {
    let bits = (bits & !(0xf << 76) & !(0x3 << 62)) | (0x4 << 76) | (0x2 << 62);
    let hex = format!("{bits:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

macro_rules! uuid_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
            specta::Type,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// `None` unless `text` is a hyphenated UUID (any case; kept in lower case).
            pub fn parse(text: &str) -> Option<Self> {
                parse_uuid(text).map(Self)
            }

            /// From 128 random bits (the engine draws them), as a UUID v4.
            pub fn from_random(bits: u128) -> Self {
                Self(uuid_from_random(bits))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

uuid_id!(
    /// Id of a signature: a UUID, the same on every device (the sync record id). A signature
    /// taken over from a stage 5 text signature has the UUID of its identity.
    SignatureId
);
uuid_id!(
    /// Id of a template: a UUID, the same on every device.
    TemplateId
);

/// An image a signature shows (`<img src="cid:CONTENT_ID">` in its HTML).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SignatureImageInfo {
    /// The `Content-ID` of the image part, without angle brackets; the HTML refers to it as
    /// `cid:<contentId>`.
    pub content_id: String,
    pub file_name: String,
    /// `image/png`, `image/jpeg`, `image/gif` or `image/webp` (recognised from the bytes).
    pub mime: String,
    pub size: u32,
}

/// A signature of the pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInfo {
    pub id: SignatureId,
    pub name: String,
    /// Allowlisted editor HTML; images as `cid:` (see [`SignatureImageData`] for showing them).
    pub html: String,
    /// The plain text (without the `-- ` separator, added when the message is put together).
    pub text: String,
    pub images: Vec<SignatureImageInfo>,
    /// Unix milliseconds.
    #[specta(type = Number)]
    pub updated_at: i64,
}

/// A signature to create (`id` null) or change. The HTML is sanitised with the allowlist of
/// outgoing mail; `cid:` images must name images of this signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInput {
    pub id: Option<SignatureId>,
    pub name: String,
    pub html: String,
}

/// What sanitising removed or simplified, for a hint below the editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SanitizeReport {
    /// Images dropped (not one of the signature's own `cid:` images: external, `data:`, SVG).
    pub images_removed: u32,
    /// Colours, fonts, sizes, alignment, tables or headings were simplified.
    pub formatting_simplified: bool,
}

/// A saved signature and what sanitising changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedSignature {
    pub signature: SignatureInfo,
    pub notes: SanitizeReport,
}

/// One image of a signature as a `data:` URL, for the signature editor (the editor shows it in
/// place of `cid:<contentId>` and writes `cid:<contentId>` back when saving).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SignatureImageData {
    pub content_id: String,
    pub data_url: String,
}

/// The default signatures of an identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IdentitySignatureDefaults {
    pub identity_id: IdentityId,
    /// For new mails; null = none.
    pub new_signature: Option<SignatureId>,
    /// For replies and forwards; null = none.
    pub reply_signature: Option<SignatureId>,
}

/// A template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInfo {
    pub id: TemplateId,
    pub name: String,
    /// May hold placeholders such as `{{first_name}}`.
    pub subject: String,
    /// Allowlisted editor HTML with placeholders.
    pub html: String,
    /// Known placeholders the template uses (`first_name`, `date`, …) and unknown ones.
    pub placeholders: Vec<String>,
    pub unknown_placeholders: Vec<String>,
    #[specta(type = Number)]
    pub updated_at: i64,
}

/// A template to create (`id` null) or change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInput {
    pub id: Option<TemplateId>,
    pub name: String,
    pub subject: String,
    pub html: String,
}

/// A saved template and what sanitising changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedTemplate {
    pub template: TemplateInfo,
    pub notes: SanitizeReport,
}

/// Something to point out after filling a template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TemplateFillProblem {
    /// `{{name}}` is no known placeholder; it stays as written.
    Unknown { name: String },
    /// No value (for example a recipient without a name) and no fallback: left empty.
    Missing { placeholder: String },
}

/// A template filled for a draft: the composer puts `html` where the cursor is and takes the
/// subject if its own is empty (the draft itself is not changed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FilledTemplateInfo {
    pub subject: String,
    /// Editor HTML.
    pub html: String,
    pub text: String,
    pub problems: Vec<TemplateFillProblem>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn ids_and_wire_format() {
        let id = SignatureId::from_random(u128::MAX);
        assert_eq!(id.as_str(), "ffffffff-ffff-4fff-bfff-ffffffffffff");
        assert_eq!(
            TemplateId::parse("6F9619FF-8B86-4D11-B42D-00C04FC964FF").map(|t| t.0),
            Some("6f9619ff-8b86-4d11-b42d-00c04fc964ff".to_owned())
        );
        assert!(SignatureId::parse("../x").is_none());
        assert_eq!(
            serde_json::to_value(TemplateFillProblem::Missing {
                placeholder: "first_name".into()
            })
            .expect("json"),
            json!({ "kind": "missing", "placeholder": "first_name" })
        );
        let input: SignatureInput =
            serde_json::from_value(json!({ "id": null, "name": "Work", "html": "<p>A</p>" }))
                .expect("input");
        assert_eq!(input.name, "Work");
    }
}
