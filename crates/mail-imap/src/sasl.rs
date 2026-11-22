//! SASL PLAIN (RFC 4616) for logins with non-ASCII credentials.
//!
//! IMAP `LOGIN` takes its arguments as strings, and async-imap sends them as quoted strings,
//! which RFC 3501 limits to 7-bit characters; what a server does with UTF-8 there is up to the
//! server. `AUTHENTICATE PLAIN` carries the credentials as UTF-8 by definition, and every
//! IMAP4rev1 server has to implement it (RFC 3501, section 6.2.2). The adapter therefore logs
//! in with LOGIN when user name and password are ASCII (the common case) and with
//! `AUTHENTICATE PLAIN` otherwise; see `ImapBackend::establish`.

use mail_model::Secret;

/// The PLAIN message `authzid NUL authcid NUL passwd` with an empty authorization id, base64.
pub(crate) fn plain_response(username: &str, password: &Secret<String>) -> Secret<String> {
    let mut message = Vec::with_capacity(username.len() + password.expose_secret().len() + 2);
    message.push(0);
    message.extend_from_slice(username.as_bytes());
    message.push(0);
    message.extend_from_slice(password.expose_secret().as_bytes());
    Secret::new(base64(&message))
}

/// async-imap's authenticator for `AUTHENTICATE PLAIN` without an initial response: answers
/// the server's (empty) challenge with the raw PLAIN message; async-imap encodes it.
pub(crate) struct PlainAuthenticator {
    message: Secret<Vec<u8>>,
}

impl PlainAuthenticator {
    pub(crate) fn new(username: &str, password: &Secret<String>) -> Self {
        let mut message = vec![0];
        message.extend_from_slice(username.as_bytes());
        message.push(0);
        message.extend_from_slice(password.expose_secret().as_bytes());
        Self {
            message: Secret::new(message),
        }
    }
}

impl async_imap::Authenticator for PlainAuthenticator {
    type Response = Vec<u8>;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        self.message.expose_secret().clone()
    }
}

/// Standard base64 with padding (RFC 4648, section 4).
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use async_imap::Authenticator;

    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        for (input, output) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), output);
        }
    }

    #[test]
    fn plain_message_is_utf8_with_empty_authzid() {
        let password = Secret::new("Grüße€".to_owned());
        // printf '\0alice\0Grüße€' | base64
        assert_eq!(
            plain_response("alice", &password).expose_secret(),
            "AGFsaWNlAEdyw7zDn2Xigqw="
        );
        let mut auth = PlainAuthenticator::new("alice", &password);
        assert_eq!(auth.process(b""), "\0alice\0Grüße€".as_bytes());
    }
}
