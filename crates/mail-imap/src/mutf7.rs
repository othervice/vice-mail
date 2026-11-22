//! Modified UTF-7 for IMAP mailbox names (RFC 3501, section 5.1.3).
//!
//! async-imap passes mailbox names through unchanged in both directions, so the adapter encodes
//! and decodes itself. The only crate for it (`utf7-imap`) has had no release since 2022.
//! `UTF8=ACCEPT` stays off in version 1 (decision 0002), so names always arrive in this form.

/// Base64 alphabet of modified UTF-7: `,` instead of `/`, no padding.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";

/// A wire name that is not valid modified UTF-7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

/// Decodes a mailbox name as sent by the server (`Projects/&ANw-berpr&APw-fung` becomes
/// `Projects/Überprüfung`).
pub fn decode(wire: &str) -> Result<String, DecodeError> {
    let mut out = String::with_capacity(wire.len());
    let mut rest = wire;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let end = after
            .find('-')
            .ok_or_else(|| DecodeError(format!("unterminated shift in {wire:?}")))?;
        let encoded = &after[..end];
        if encoded.is_empty() {
            out.push('&');
        } else {
            let bytes = decode_base64(encoded)
                .ok_or_else(|| DecodeError(format!("invalid base64 in {wire:?}")))?;
            if bytes.len() % 2 != 0 {
                return Err(DecodeError(format!("odd UTF-16 length in {wire:?}")));
            }
            let units = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_be_bytes(*pair));
            for ch in char::decode_utf16(units) {
                out.push(ch.map_err(|_| DecodeError(format!("invalid UTF-16 in {wire:?}")))?);
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Encodes a mailbox name for the wire (inverse of [`decode`]).
pub fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 8);
    let mut pending: Vec<u16> = Vec::new();
    for ch in name.chars() {
        if (' '..='~').contains(&ch) {
            flush(&mut pending, &mut out);
            if ch == '&' {
                out.push_str("&-");
            } else {
                out.push(ch);
            }
        } else {
            let mut buf = [0u16; 2];
            pending.extend_from_slice(ch.encode_utf16(&mut buf));
        }
    }
    flush(&mut pending, &mut out);
    out
}

fn flush(pending: &mut Vec<u16>, out: &mut String) {
    if pending.is_empty() {
        return;
    }
    let bytes: Vec<u8> = pending.iter().flat_map(|u| u.to_be_bytes()).collect();
    out.push('&');
    out.push_str(&encode_base64(&bytes));
    out.push('-');
    pending.clear();
}

fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for b in s.bytes() {
        let value = ALPHABET.iter().position(|&a| a == b)?;
        acc = (acc << 6) | u32::try_from(value).ok()?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn encode_base64(bytes: &[u8]) -> String {
    let symbol = |index: u32| char::from(ALPHABET[(index & 63) as usize]);
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 2);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            out.push(symbol(acc >> bits));
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(symbol(acc << (6 - bits)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_and_seed_examples_round_trip() {
        let cases = [
            ("Projects/&ANw-berpr&APw-fung", "Projects/Überprüfung"),
            ("~peter/mail/&U,BTFw-/&ZeVnLIqe-", "~peter/mail/台北/日本語"),
            ("A&-B", "A&B"),
            ("Emoji &2D3eAA-", "Emoji 😀"),
            ("Entw&APw-rfe", "Entwürfe"),
            ("Gel&APY-schte Elemente", "Gelöschte Elemente"),
            ("INBOX", "INBOX"),
            ("", ""),
        ];
        for (wire, name) in cases {
            assert_eq!(decode(wire).as_deref(), Ok(name), "{wire}");
            assert_eq!(encode(name), wire, "{name}");
        }
    }

    #[test]
    fn every_char_round_trips() {
        let all: String = ('\u{1}'..='\u{2fff}')
            .chain(['\u{1f600}', '\u{10ffff}'])
            .collect();
        assert_eq!(decode(&encode(&all)).as_deref(), Ok(all.as_str()));
    }

    #[test]
    fn broken_input_is_an_error_not_a_panic() {
        for wire in [
            "&ANw", "&A!w-", "&AN-", "&2D0-", "&-&", "a&ZeVn&-", "&////-",
        ] {
            assert!(decode(wire).is_err(), "{wire}");
        }
        assert_eq!(decode("tail-&-").as_deref(), Ok("tail-&"));
    }
}
