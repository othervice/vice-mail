//! PKCE (RFC 7636) and the `state` value: random strings from the operating system's random
//! source (through the app's one crypto provider), and the S256 challenge.

use mail_model::{OAuthError, Secret};
use sha2::{Digest, Sha256};

/// 32 random bytes: 43 characters of base64url, the RFC's recommended verifier length.
const RANDOM_BYTES: usize = 32;

/// A fresh random string of 43 base64url characters (256 bits). Fails only without OS
/// randomness, in which case no sign-in may start (a guessable `state` or verifier would
/// defeat its purpose).
pub(crate) fn random_token() -> Result<Secret<String>, OAuthError> {
    let mut bytes = [0_u8; RANDOM_BYTES];
    mail_imap::tls::provider()
        .secure_random
        .fill(&mut bytes)
        .map_err(|_| {
            OAuthError::Failed("the system has no random numbers for the sign-in".into())
        })?;
    Ok(Secret::new(base64url(&bytes)))
}

/// The S256 code challenge: `BASE64URL(SHA256(ASCII(verifier)))`.
pub(crate) fn challenge(verifier: &Secret<String>) -> String {
    base64url(&Sha256::digest(verifier.expose_secret().as_bytes()))
}

/// Base64url without padding (RFC 4648, section 5), as PKCE asks.
pub(crate) fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            }
        }
    }
    out
}

/// Compares two secrets in time independent of where they differ.
pub(crate) fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0_u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches_rfc_7636_appendix_b() {
        let verifier = Secret::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_owned());
        assert_eq!(
            challenge(&verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_tokens_are_long_unique_and_url_safe() {
        let a = random_token().expect("random");
        let b = random_token().expect("random");
        assert_eq!(a.expose_secret().len(), 43);
        assert_ne!(a.expose_secret(), b.expose_secret());
        assert!(
            a.expose_secret()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
    }

    #[test]
    fn base64url_has_no_padding() {
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "abcd"));
    }
}
