//! Images in sanitised HTML. Only raster formats recognised by their magic bytes are shown, and
//! only as base64 `data:` URIs built here, so every image URL in the output has one strict form.
//! SVG is never accepted: it is a document format, not a picture.

use std::sync::Arc;

/// A transparent 1×1 GIF. Stands in for blocked external images, so the layout keeps its size.
pub(crate) const PLACEHOLDER_PIXEL: &str =
    "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";

/// Image bytes handed to the renderer (inline parts, or external images the core fetched).
/// Shared, so options can be cloned cheaply.
pub type ImageBytes = Arc<[u8]>;

/// Raster formats a mail may show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageType {
    Png,
    Jpeg,
    Gif,
    Webp,
    Bmp,
    Avif,
    Ico,
}

impl ImageType {
    /// MIME type used in the `data:` URI.
    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
            Self::Bmp => "image/bmp",
            Self::Avif => "image/avif",
            Self::Ico => "image/x-icon",
        }
    }
}

/// Recognises an image by its first bytes. The declared MIME type of a part is never trusted.
pub fn sniff_image(bytes: &[u8]) -> Option<ImageType> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageType::Png)
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        Some(ImageType::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(ImageType::Gif)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some(ImageType::Webp)
    } else if bytes.len() >= 12
        && bytes.get(4..8) == Some(b"ftyp")
        && matches!(bytes.get(8..12), Some(b"avif" | b"avis"))
    {
        Some(ImageType::Avif)
    } else if bytes.starts_with(b"BM") && bytes.len() >= 26 {
        Some(ImageType::Bmp)
    } else if bytes.starts_with(&[0, 0, 1, 0]) && bytes.len() >= 22 {
        Some(ImageType::Ico)
    } else {
        None
    }
}

/// `data:<mime>;base64,<payload>` for bytes that are a known image, else `None`.
pub(crate) fn image_data_uri(bytes: &[u8]) -> Option<String> {
    let kind = sniff_image(bytes)?;
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4 + 32);
    out.push_str("data:");
    out.push_str(kind.mime());
    out.push_str(";base64,");
    base64_encode_into(bytes, &mut out);
    Some(out)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn base64_encode_into(bytes: &[u8], out: &mut String) {
    let symbol = |index: u32| char::from(BASE64[(index & 63) as usize]);
    let (chunks, rest) = bytes.as_chunks::<3>();
    for &[a, b, c] in chunks {
        let n = u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c);
        out.extend([symbol(n >> 18), symbol(n >> 12), symbol(n >> 6), symbol(n)]);
    }
    match *rest {
        [a] => {
            let n = u32::from(a) << 16;
            out.extend([symbol(n >> 18), symbol(n >> 12), '=', '=']);
        }
        [a, b] => {
            let n = u32::from(a) << 16 | u32::from(b) << 8;
            out.extend([symbol(n >> 18), symbol(n >> 12), symbol(n >> 6), '=']);
        }
        _ => {}
    }
}

/// Lenient base64 decoder for `data:` URIs written by mail authors: skips whitespace and stops
/// at padding. Returns `None` on any other byte outside the alphabet.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%'
            && let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).and_then(|b| char::from(*b).to_digit(16)),
                bytes.get(i + 2).and_then(|b| char::from(*b).to_digit(16)),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
            continue;
        }
        out.push(byte);
        i += 1;
    }
    out
}

/// Decodes a `data:` URL (any declared type; the bytes are sniffed later). `None` when the URL
/// is not a data URL, is malformed, or its payload would exceed `max_bytes`.
pub(crate) fn decode_data_url(url: &str, max_bytes: usize) -> Option<Vec<u8>> {
    let rest = url
        .get(..5)
        .filter(|scheme| scheme.eq_ignore_ascii_case("data:"))
        .and_then(|_| url.get(5..))?;
    let (meta, payload) = rest.split_once(',')?;
    // Base64 needs 4 characters for 3 bytes; anything far larger is refused before decoding.
    if payload.len() > max_bytes.saturating_mul(4) / 3 + 1024 {
        return None;
    }
    let is_base64 = meta
        .rsplit(';')
        .next()
        .is_some_and(|last| last.trim().eq_ignore_ascii_case("base64"));
    let bytes = if is_base64 {
        base64_decode(payload)?
    } else {
        percent_decode(payload)
    };
    (bytes.len() <= max_bytes).then_some(bytes)
}

/// One candidate of a `srcset` attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SrcsetCandidate<'a> {
    pub(crate) url: &'a str,
    /// `2x`, `640w`, or empty. Anything else is dropped.
    pub(crate) descriptor: &'a str,
}

/// Splits a `srcset` value as the HTML standard does: a URL is a run of non-whitespace (commas
/// inside it are allowed, trailing ones end it), followed by descriptors up to the next comma
/// outside parentheses.
pub(crate) fn parse_srcset(value: &str) -> Vec<SrcsetCandidate<'_>> {
    let bytes = value.as_bytes();
    let is_space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c');
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (is_space(bytes[i]) || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && !is_space(bytes[i]) {
            i += 1;
        }
        let mut url_end = i;
        let mut descriptor = "";
        if bytes[start..url_end].ends_with(b",") {
            while url_end > start && bytes[url_end - 1] == b',' {
                url_end -= 1;
            }
        } else {
            let desc_start = i;
            let mut depth = 0usize;
            while i < bytes.len() {
                match bytes[i] {
                    b'(' => depth += 1,
                    b')' => depth = depth.saturating_sub(1),
                    b',' if depth == 0 => break,
                    _ => {}
                }
                i += 1;
            }
            descriptor = value.get(desc_start..i).unwrap_or("").trim();
        }
        if let Some(url) = value.get(start..url_end).filter(|url| !url.is_empty()) {
            let descriptor = if is_plain_descriptor(descriptor) {
                descriptor
            } else {
                ""
            };
            out.push(SrcsetCandidate { url, descriptor });
        }
    }
    out
}

/// `1.5x`, `640w`: a number and one of `x`, `w`, `h`.
fn is_plain_descriptor(descriptor: &str) -> bool {
    let Some(number) = descriptor
        .strip_suffix('x')
        .or_else(|| descriptor.strip_suffix('w'))
        .or_else(|| descriptor.strip_suffix('h'))
    else {
        return false;
    };
    !number.is_empty()
        && number.len() <= 12
        && number.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            let mut out = String::new();
            base64_encode_into(input.as_bytes(), &mut out);
            assert_eq!(out, expected);
            assert_eq!(base64_decode(expected).unwrap(), input.as_bytes());
        }
        assert_eq!(base64_decode("Zm9v\r\nYmFy").unwrap(), b"foobar");
        assert!(base64_decode("Zm9v<script>").is_none());
    }

    #[test]
    fn sniffing_recognises_rasters_and_rejects_the_rest() {
        let pixel = decode_data_url(PLACEHOLDER_PIXEL, 1024).unwrap();
        assert_eq!(sniff_image(&pixel), Some(ImageType::Gif));
        assert_eq!(image_data_uri(&pixel).as_deref(), Some(PLACEHOLDER_PIXEL));
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\nrest"), Some(ImageType::Png));
        assert_eq!(sniff_image(b"\xFF\xD8\xFF\xE0"), Some(ImageType::Jpeg));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImageType::Webp));
        assert_eq!(
            sniff_image(b"\0\0\0\x1cftypavif\0\0"),
            Some(ImageType::Avif)
        );
        assert_eq!(
            sniff_image(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
            None
        );
        assert_eq!(sniff_image(b"<html><script>"), None);
        assert_eq!(sniff_image(b""), None);
    }

    #[test]
    fn data_urls_decode_with_limits() {
        assert_eq!(decode_data_url("data:,a%20b", 10).unwrap(), b"a b");
        assert_eq!(
            decode_data_url("DATA:image/png;BASE64,Zm9v", 10).unwrap(),
            b"foo"
        );
        assert!(decode_data_url("data:;base64,Zm9vYmFy", 3).is_none());
        assert!(decode_data_url("http://x/", 10).is_none());
        assert!(decode_data_url("data:no-comma", 10).is_none());
    }

    #[test]
    fn srcset_follows_the_html_algorithm() {
        let c =
            parse_srcset("a.png 1x, b.png 2x,c.png, data:image/png;base64,AAAA 640w, d.png (x,y)");
        let got: Vec<_> = c.iter().map(|c| (c.url, c.descriptor)).collect();
        assert_eq!(
            got,
            [
                ("a.png", "1x"),
                ("b.png", "2x"),
                ("c.png", ""),
                ("data:image/png;base64,AAAA", "640w"),
                ("d.png", ""),
            ]
        );
        assert!(parse_srcset(" , ,").is_empty());
    }
}
