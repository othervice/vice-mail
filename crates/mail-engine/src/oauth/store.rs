//! The refresh token in the system keychain, under the account's keychain reference
//! (`oauth:<provider>:<account>`). Only the refresh token is kept; access tokens live in
//! memory and are fetched again after a restart.
//!
//! The entry is JSON: `{"v":1,"refreshToken":"<first piece>","pieces":n}`. Windows Credential
//! Manager keeps at most 2,560 bytes per entry (1,280 UTF-16 characters, how keyring stores
//! text), and providers do not promise a length for refresh tokens, so a long token is split:
//! the first piece in the entry itself, the others in `<reference>#1`, `<reference>#2`, … (at
//! most [`MAX_PIECES`] entries in all). Deleting the account's entry deletes the pieces too
//! (`Credentials::delete`).

use mail_model::{CoreError, OAuthError, Secret};
use serde::{Deserialize, Serialize};

use crate::credentials::{CredentialRef, Credentials};

/// Characters per piece; with the JSON around it well below Windows' 1,280.
const PIECE_CHARS: usize = 1000;
/// Longest refresh token stored: 8 pieces of 1,000 characters.
pub(crate) const MAX_PIECES: usize = 8;

#[derive(Serialize, Deserialize)]
struct Entry {
    v: u32,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    pieces: usize,
}

/// Splits `text` into pieces of at most [`PIECE_CHARS`] characters (on character bounds).
fn split(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(PIECE_CHARS)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

/// Stores the refresh token of `key`, replacing an earlier one. Pieces go first, the entry
/// itself last, so an interrupted write leaves the earlier entry readable or nothing.
pub(crate) async fn save(
    credentials: &Credentials,
    key: &CredentialRef,
    refresh_token: &Secret<String>,
) -> Result<(), CoreError> {
    let pieces = split(refresh_token.expose_secret());
    if pieces.is_empty() || pieces.len() > MAX_PIECES {
        return Err(OAuthError::Failed(format!(
            "the provider's refresh token is {} characters long; at most {} can be stored",
            refresh_token.expose_secret().chars().count(),
            PIECE_CHARS * MAX_PIECES
        ))
        .into());
    }
    for (n, piece) in pieces.iter().enumerate().skip(1) {
        credentials
            .set_raw(&key.piece(n), Secret::new(piece.clone()))
            .await?;
    }
    let first = pieces.first().cloned().unwrap_or_default();
    let entry = Entry {
        v: 1,
        refresh_token: first,
        pieces: pieces.len(),
    };
    let json = serde_json::to_string(&entry)
        .map_err(|err| CoreError::Internal(format!("sign-in entry: {err}")))?;
    credentials.set_raw(key, Secret::new(json)).await?;
    // Pieces of a longer token from before.
    for n in pieces.len()..MAX_PIECES {
        if let Err(err) = credentials.delete_raw(&key.piece(n)).await {
            tracing::debug!(error = %err, "an old piece of a sign-in could not be removed");
        }
    }
    Ok(())
}

/// The stored refresh token of `key`, `None` if this device has none (an account taken over
/// from the settings sync, or one whose sign-in was removed).
pub(crate) async fn load(
    credentials: &Credentials,
    key: &CredentialRef,
) -> Result<Option<Secret<String>>, CoreError> {
    let Some(text) = credentials.get_raw(key).await? else {
        return Ok(None);
    };
    let unreadable = || {
        CoreError::from(OAuthError::SignInRequired(
            "the stored sign-in is unreadable".into(),
        ))
    };
    let entry: Entry = serde_json::from_str(text.expose_secret()).map_err(|_| unreadable())?;
    if entry.v != 1 || entry.pieces == 0 || entry.pieces > MAX_PIECES {
        return Err(unreadable());
    }
    let mut token = entry.refresh_token;
    for n in 1..entry.pieces {
        let piece = credentials
            .get_raw(&key.piece(n))
            .await?
            .ok_or_else(unreadable)?;
        token.push_str(piece.expose_secret());
    }
    Ok(Some(Secret::new(token)))
}

#[cfg(test)]
mod tests {
    use mail_model::{AccountId, OAuthProvider};

    use super::*;
    use crate::credentials::MemorySecretStore;

    #[tokio::test]
    async fn long_tokens_are_stored_in_pieces_and_deleted_with_the_entry() {
        let store = std::sync::Arc::new(MemorySecretStore::new());
        let credentials = Credentials::from_arc(store.clone());
        let key = CredentialRef::for_oauth_account(
            OAuthProvider::Microsoft,
            AccountId::from_uuid(uuid::Uuid::new_v4()),
        );

        let short = Secret::new("M.R3_short".to_owned());
        save(&credentials, &key, &short).await.expect("save");
        assert_eq!(store.len(), 1);
        let loaded = load(&credentials, &key).await.expect("load").expect("some");
        assert_eq!(loaded.expose_secret(), "M.R3_short");

        // 2,500 characters: three entries, each below Windows' limit.
        let long = Secret::new("x".repeat(1200) + &"y".repeat(1300));
        save(&credentials, &key, &long).await.expect("save long");
        assert_eq!(store.len(), 3);
        assert!(store.longest() < 1280, "{}", store.longest());
        let loaded = load(&credentials, &key).await.expect("load").expect("some");
        assert_eq!(loaded.expose_secret(), long.expose_secret());

        // Shorter again: the old pieces go.
        save(&credentials, &key, &short).await.expect("save");
        assert_eq!(store.len(), 1);

        save(&credentials, &key, &long).await.expect("save long");
        assert!(credentials.delete(&key).await.expect("delete"));
        assert!(store.is_empty());

        let huge = Secret::new("z".repeat(PIECE_CHARS * MAX_PIECES + 1));
        assert!(save(&credentials, &key, &huge).await.is_err());
        assert!(store.is_empty());

        // A password consumer never gets the sign-in entry.
        save(&credentials, &key, &short).await.expect("save");
        assert!(credentials.get(&key).await.is_err());
    }
}
