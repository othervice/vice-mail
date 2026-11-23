//! SASL PLAIN (RFC 4616) for logins with non-ASCII credentials, and since stage 14 the two
//! bearer-token mechanisms for accounts that sign in with Microsoft or Google (decision 0042):
//! `XOAUTH2` (what both providers document) and `OAUTHBEARER` (RFC 7628).
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

/// A SASL mechanism that carries an OAuth 2.0 access token instead of a password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bearer {
    /// Google's and Microsoft's `XOAUTH2`: `user=<user>^Aauth=Bearer <token>^A^A`.
    XOauth2,
    /// RFC 7628: GS2 header with the user, then `host`, `port` and `auth` key-value pairs.
    OAuthBearer,
}

impl Bearer {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::XOauth2 => "XOAUTH2",
            Self::OAuthBearer => "OAUTHBEARER",
        }
    }

    /// The client's answer to the server's error challenge (a base64 JSON status), after which
    /// the server ends the exchange with a failure: empty for XOAUTH2, `^A` for OAUTHBEARER
    /// (RFC 7628, section 3.2.3).
    pub(crate) const fn error_ack(self) -> &'static [u8] {
        match self {
            Self::XOauth2 => b"",
            Self::OAuthBearer => b"\x01",
        }
    }

    /// The initial client response with the token. `host` and `port` only go into
    /// OAUTHBEARER, which defines them.
    pub(crate) fn message(
        self,
        username: &str,
        host: &str,
        port: u16,
        token: &Secret<String>,
    ) -> Secret<Vec<u8>> {
        let token = token.expose_secret();
        let text = match self {
            Self::XOauth2 => format!("user={username}\x01auth=Bearer {token}\x01\x01"),
            Self::OAuthBearer => {
                // RFC 5801: `,` and `=` in the authorization identity are escaped.
                let user = username.replace('=', "=3D").replace(',', "=2C");
                format!("n,a={user},\x01host={host}\x01port={port}\x01auth=Bearer {token}\x01\x01")
            }
        };
        Secret::new(text.into_bytes())
    }
}

/// async-imap's authenticator for the bearer mechanisms: the first continuation gets the token
/// (unless it already went on the command line with SASL-IR), any later one is the server's
/// error challenge and gets [`Bearer::error_ack`], so the server answers with its NO.
pub(crate) struct BearerAuthenticator {
    mechanism: Bearer,
    message: Secret<Vec<u8>>,
    sent: bool,
}

impl BearerAuthenticator {
    pub(crate) fn new(mechanism: Bearer, message: Secret<Vec<u8>>, sent: bool) -> Self {
        Self {
            mechanism,
            message,
            sent,
        }
    }
}

impl async_imap::Authenticator for BearerAuthenticator {
    type Response = Vec<u8>;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        if self.sent {
            self.mechanism.error_ack().to_vec()
        } else {
            self.sent = true;
            self.message.expose_secret().clone()
        }
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

    #[test]
    fn bearer_messages_follow_their_specifications() {
        let token = Secret::new("ya29.token".to_owned());
        let x = Bearer::XOauth2.message("alice@example.org", "imap.example.org", 993, &token);
        assert_eq!(
            x.expose_secret().as_slice(),
            b"user=alice@example.org\x01auth=Bearer ya29.token\x01\x01"
        );
        // RFC 7628, section 4.1 (with the user's `,` and `=` escaped as RFC 5801 says).
        let o = Bearer::OAuthBearer.message("a,b=c@example.org", "server.example.com", 143, &token);
        assert_eq!(
            o.expose_secret().as_slice(),
            b"n,a=a=2Cb=3Dc@example.org,\x01host=server.example.com\x01port=143\x01auth=Bearer ya29.token\x01\x01"
        );
        // The first continuation gets the token, the error challenge the acknowledgement.
        let mut auth = BearerAuthenticator::new(Bearer::OAuthBearer, o.clone(), false);
        assert_eq!(auth.process(b""), o.expose_secret().clone());
        assert_eq!(auth.process(b"{\"status\":\"invalid_token\"}"), b"\x01");
        let mut auth = BearerAuthenticator::new(Bearer::XOauth2, x, true);
        assert_eq!(auth.process(b"{\"status\":\"401\"}"), b"");
    }
}
