//! Modified UTF-7 for IMAP mailbox names (RFC 3501, section 5.1.3).
//!
//! async-imap passes mailbox names through unchanged in both directions, so the client has to
//! encode and decode itself. The only crate for it (`utf7-imap`) has had no release since 2022.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";

#[derive(Debug, PartialEq, Eq)]
pub struct DecodeError(pub String);

pub fn decode(wire: &str) -> Result<String, DecodeError> {
    let mut out = String::with_capacity(wire.len());
    let mut rest = wire;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let end = after
            .find('-')
            .ok_or_else(|| DecodeError(format!("unterminated shift in {wire:?}")))?;
        let b64 = &after[..end];
        if b64.is_empty() {
            out.push('&');
        } else {
            let bytes = decode_b64(b64).ok_or_else(|| DecodeError(format!("bad base64 in {wire:?}")))?;
            if bytes.len() % 2 != 0 {
                return Err(DecodeError(format!("odd UTF-16 length in {wire:?}")));
            }
            let units = bytes.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]]));
            for ch in char::decode_utf16(units) {
                out.push(ch.map_err(|_| DecodeError(format!("bad UTF-16 in {wire:?}")))?);
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

pub fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 8);
    let mut pending: Vec<u16> = Vec::new();
    let flush = |pending: &mut Vec<u16>, out: &mut String| {
        if pending.is_empty() {
            return;
        }
        let bytes: Vec<u8> = pending.iter().flat_map(|u| u.to_be_bytes()).collect();
        out.push('&');
        out.push_str(&encode_b64(&bytes));
        out.push('-');
        pending.clear();
    };
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

fn decode_b64(s: &str) -> Option<Vec<u8>> {
    let mut acc: u32 = 0;
    let mut bits = 0;
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for b in s.bytes() {
        let v = ALPHABET.iter().position(|&a| a == b)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn encode_b64(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &b in bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            out.push(ALPHABET[((acc >> bits) & 63) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (6 - bits)) & 63) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_examples() {
        assert_eq!(decode("Projects/&ANw-berpr&APw-fung").unwrap(), "Projects/Überprüfung");
        assert_eq!(encode("Projects/Überprüfung"), "Projects/&ANw-berpr&APw-fung");
        assert_eq!(
            decode("~peter/mail/&U,BTFw-/&ZeVnLIqe-").unwrap(),
            "~peter/mail/台北/日本語"
        );
        assert_eq!(encode("~peter/mail/台北/日本語"), "~peter/mail/&U,BTFw-/&ZeVnLIqe-");
        assert_eq!(decode("A&-B").unwrap(), "A&B");
        assert_eq!(encode("A&B"), "A&-B");
        assert_eq!(encode("Emoji 😀"), "Emoji &2D3eAA-");
        assert_eq!(decode("Emoji &2D3eAA-").unwrap(), "Emoji 😀");
    }

    #[test]
    fn broken_input_is_an_error_not_a_panic() {
        assert!(decode("&ANw").is_err());
        assert!(decode("&A!w-").is_err());
        assert!(decode("&AN-").is_err());
        assert!(decode("&2D0-").is_err()); // lone surrogate
    }
}
