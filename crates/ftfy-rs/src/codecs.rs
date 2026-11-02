//! Decoding bytes with the encodings ftfy works with, including its "sloppy"
//! codecs and `utf-8-variants`, with errors described the way Python
//! describes them.
//!
//! ```
//! use ftfy_rs::codecs::Codec;
//!
//! let codec = Codec::lookup("sloppy-windows-1252").unwrap();
//! assert_eq!(codec.decode(b"\x80\x81").unwrap(), "\u{20AC}\u{81}");
//!
//! let strict = Codec::lookup("windows-1252").unwrap();
//! let err = strict.decode(b"ab\x9dc").unwrap_err();
//! assert_eq!(
//!     err.to_string(),
//!     "'charmap' codec can't decode byte 0x9d in position 2: character maps to <undefined>"
//! );
//! ```

use std::fmt;

use crate::chardata::{decode_utf8_variants, CHARMAP_ENCODINGS};
use crate::tables::{CHARMAPS, NO_CHAR};

/// A text encoding that can decode bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Utf8,
    /// UTF-8 plus CESU-8 surrogate pairs and Java's encoding of U+0000.
    Utf8Variants,
    /// UTF-16, little-endian unless the bytes start with a byte order mark.
    Utf16,
    /// A single-byte encoding; the index of its table.
    Charmap(usize),
}

impl Codec {
    /// Find a codec by name, accepting the names and common aliases Python
    /// accepts for it. Returns None for encodings this module doesn't have.
    pub fn lookup(name: &str) -> Option<Codec> {
        let name = normalize_name(name);
        let charmap = |canonical: &str| {
            CHARMAPS
                .iter()
                .position(|(n, _)| *n == canonical)
                .map(Codec::Charmap)
        };
        match name.as_str() {
            "utf_8" | "utf8" | "u8" | "utf" | "cp65001" => Some(Codec::Utf8),
            "utf_8_variants" | "utf8_variants" | "utf_8_variant" | "utf8_variant" | "utf_8_var"
            | "utf8_var" | "cesu_8" | "cesu8" | "java_utf_8" | "java_utf8" => {
                Some(Codec::Utf8Variants)
            }
            "utf_16" | "utf16" | "u16" => Some(Codec::Utf16),
            "latin_1" | "latin1" | "latin" | "l1" | "iso_8859_1" | "iso8859_1" | "8859"
            | "cp819" | "iso_ir_100" => charmap("latin-1"),
            "windows_1252" | "cp1252" => charmap("windows-1252"),
            "iso_8859_2" | "iso8859_2" | "latin2" | "l2" => charmap("iso-8859-2"),
            "macroman" | "mac_roman" | "macintosh" => charmap("macroman"),
            "cp437" | "437" | "ibm437" => charmap("cp437"),
            other => {
                // sloppy-windows-NNNN and its alias sloppy-cpNNNN
                let number = other
                    .strip_prefix("sloppy_windows_")
                    .or_else(|| other.strip_prefix("sloppy_cp"))?;
                charmap(&format!("sloppy-windows-{number}"))
            }
        }
    }

    /// The codec's canonical name, as ftfy writes it.
    pub fn name(self) -> &'static str {
        match self {
            Codec::Utf8 => "utf-8",
            Codec::Utf8Variants => "utf-8-variants",
            Codec::Utf16 => "utf-16",
            Codec::Charmap(index) => CHARMAPS[index].0,
        }
    }

    /// Decode bytes, failing where Python's codec would raise
    /// UnicodeDecodeError.
    pub fn decode(self, bytes: &[u8]) -> Result<String, DecodeError> {
        match self {
            Codec::Utf8 => std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|err| utf8_error(bytes, err)),
            Codec::Utf8Variants => decode_utf8_variants(bytes).ok_or_else(|| {
                // Report the first byte that isn't valid UTF-8, as the
                // standard UTF-8 decoder would.
                match std::str::from_utf8(bytes) {
                    Err(err) => utf8_error(bytes, err),
                    Ok(_) => unreachable!("valid UTF-8 is valid utf-8-variants"),
                }
            }),
            Codec::Utf16 => decode_utf16(bytes),
            Codec::Charmap(index) => {
                let table = &CHARMAPS[index].1;
                let mut out = String::with_capacity(bytes.len() * 2);
                for (i, &b) in bytes.iter().enumerate() {
                    match table[b as usize] {
                        NO_CHAR => {
                            return Err(DecodeError {
                                codec: "charmap",
                                start: i,
                                end: i + 1,
                                byte: b,
                                reason: "character maps to <undefined>",
                            })
                        }
                        cp => out.push(char::from_u32(cp).unwrap()),
                    }
                }
                Ok(out)
            }
        }
    }

    /// The codecs ftfy tries when fixing mojibake, plus strict windows-1252.
    pub fn charmaps() -> impl Iterator<Item = Codec> {
        (0..CHARMAP_ENCODINGS.len() + 1).map(Codec::Charmap)
    }
}

/// Guess how to decode bytes in an unknown encoding, like ftfy's
/// `guess_bytes`: UTF-16 if there's a byte order mark, then UTF-8 (or
/// utf-8-variants if it has the bytes those are for), then MacRoman if
/// there are CRs but no LFs, and finally sloppy-windows-1252.
///
/// As in ftfy, this fails only when the bytes start with a UTF-16 byte order
/// mark but aren't valid UTF-16.
pub fn guess(bytes: &[u8]) -> Result<(String, Codec), DecodeError> {
    if bytes.starts_with(b"\xFE\xFF") || bytes.starts_with(b"\xFF\xFE") {
        return Codec::Utf16.decode(bytes).map(|text| (text, Codec::Utf16));
    }
    let utf8 = if bytes.contains(&0xED) || bytes.contains(&0xC0) {
        Codec::Utf8Variants
    } else {
        Codec::Utf8
    };
    if let Ok(text) = utf8.decode(bytes) {
        return Ok((text, utf8));
    }
    let fallback = if bytes.contains(&b'\r') && !bytes.contains(&b'\n') {
        "macroman"
    } else {
        "sloppy-windows-1252"
    };
    let codec = Codec::lookup(fallback).expect("ftfy's own codecs are in the tables");
    let text = codec.decode(bytes).expect("these codecs decode any bytes");
    Ok((text, codec))
}

/// Python normalizes encoding names by lowercasing them and turning runs of
/// punctuation and spaces into underscores.
fn normalize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut pending_underscore = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' {
            if pending_underscore && !out.is_empty() {
                out.push('_');
            }
            pending_underscore = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_underscore = true;
        }
    }
    out
}

/// Bytes that couldn't be decoded, described like Python's
/// UnicodeDecodeError.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError {
    /// The codec name Python would report, such as "utf-8" or "charmap".
    pub codec: &'static str,
    pub start: usize,
    pub end: usize,
    /// The first byte that couldn't be decoded.
    pub byte: u8,
    pub reason: &'static str,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.end - self.start == 1 {
            write!(
                f,
                "'{}' codec can't decode byte 0x{:02x} in position {}: {}",
                self.codec, self.byte, self.start, self.reason
            )
        } else {
            write!(
                f,
                "'{}' codec can't decode bytes in position {}-{}: {}",
                self.codec,
                self.start,
                self.end - 1,
                self.reason
            )
        }
    }
}

impl std::error::Error for DecodeError {}

fn utf8_error(bytes: &[u8], err: std::str::Utf8Error) -> DecodeError {
    let start = err.valid_up_to();
    let byte = bytes[start];
    let (end, reason) = match err.error_len() {
        None => (bytes.len(), "unexpected end of data"),
        Some(len) => {
            let invalid_start = matches!(byte, 0x80..=0xC1 | 0xF5..=0xFF);
            let reason = if invalid_start {
                "invalid start byte"
            } else {
                "invalid continuation byte"
            };
            (start + len, reason)
        }
    };
    DecodeError {
        codec: "utf-8",
        start,
        end,
        byte,
        reason,
    }
}

fn decode_utf16(bytes: &[u8]) -> Result<String, DecodeError> {
    let (big_endian, offset) = match bytes {
        [0xFE, 0xFF, ..] => (true, 2),
        [0xFF, 0xFE, ..] => (false, 2),
        _ => (false, 0),
    };
    let codec = if big_endian { "utf-16-be" } else { "utf-16-le" };
    let unit = |i: usize| {
        let pair = [bytes[i], bytes[i + 1]];
        if big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        }
    };
    let error = |start: usize, end: usize, reason| DecodeError {
        codec,
        start,
        end,
        byte: bytes[start],
        reason,
    };

    let mut out = String::with_capacity(bytes.len() / 2);
    let mut i = offset;
    while i + 1 < bytes.len() {
        let u = unit(i);
        match u {
            0xD800..=0xDBFF => {
                if i + 3 >= bytes.len() {
                    return Err(error(i, bytes.len(), "unexpected end of data"));
                }
                let lo = unit(i + 2);
                if !(0xDC00..=0xDFFF).contains(&lo) {
                    return Err(error(i, i + 2, "illegal UTF-16 surrogate"));
                }
                let cp = 0x10000 + ((u as u32 - 0xD800) << 10) + (lo as u32 - 0xDC00);
                out.push(char::from_u32(cp).unwrap());
                i += 4;
            }
            0xDC00..=0xDFFF => return Err(error(i, i + 2, "illegal encoding")),
            _ => {
                out.push(char::from_u32(u as u32).unwrap());
                i += 2;
            }
        }
    }
    if i < bytes.len() {
        return Err(error(i, i + 1, "truncated data"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_err(name: &str, bytes: &[u8]) -> String {
        Codec::lookup(name)
            .unwrap()
            .decode(bytes)
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn errors_read_like_python() {
        // Expected messages copied from CPython 3.13.
        let cases: [(&str, &[u8], &str); 11] = [
            ("utf-8", b"abc\xe2\x82", "'utf-8' codec can't decode bytes in position 3-4: unexpected end of data"),
            ("utf-8", b"\xe2(\xa1", "'utf-8' codec can't decode byte 0xe2 in position 0: invalid continuation byte"),
            ("utf-8", b"\xf0\x9f\x98x", "'utf-8' codec can't decode bytes in position 0-2: invalid continuation byte"),
            ("utf-8", b"\xffab", "'utf-8' codec can't decode byte 0xff in position 0: invalid start byte"),
            ("utf-8", b"\xed\xa0\x80", "'utf-8' codec can't decode byte 0xed in position 0: invalid continuation byte"),
            ("utf-16", b"a\x00b", "'utf-16-le' codec can't decode byte 0x62 in position 2: truncated data"),
            ("utf-16", b"\x00\xdc", "'utf-16-le' codec can't decode bytes in position 0-1: illegal encoding"),
            ("utf-16", b"\x00\xd8a\x00", "'utf-16-le' codec can't decode bytes in position 0-1: illegal UTF-16 surrogate"),
            ("utf-16", b"a\x00\x00\xd8", "'utf-16-le' codec can't decode bytes in position 2-3: unexpected end of data"),
            ("utf-16", b"\xfe\xff\x00a\x00", "'utf-16-be' codec can't decode byte 0x00 in position 4: truncated data"),
            ("windows-1252", b"ab\x9dc", "'charmap' codec can't decode byte 0x9d in position 2: character maps to <undefined>"),
        ];
        for (name, bytes, expected) in cases {
            assert_eq!(decode_err(name, bytes), expected, "{name} {bytes:?}");
        }
    }

    #[test]
    fn lookup_accepts_python_style_names() {
        assert_eq!(Codec::lookup("UTF-8"), Some(Codec::Utf8));
        assert_eq!(Codec::lookup("utf8_var"), Some(Codec::Utf8Variants));
        assert_eq!(
            Codec::lookup("Sloppy Windows-1252").map(Codec::name),
            Some("sloppy-windows-1252")
        );
        assert_eq!(
            Codec::lookup("sloppy-cp1250").map(Codec::name),
            Some("sloppy-windows-1250")
        );
        assert_eq!(Codec::lookup("Latin-1").map(Codec::name), Some("latin-1"));
        assert_eq!(
            Codec::lookup("mac_roman").map(Codec::name),
            Some("macroman")
        );
        // Not one of ftfy's tables.
        assert_eq!(Codec::lookup("sloppy-windows-1255"), None);
        assert_eq!(Codec::lookup("shift_jis"), None);
    }
}
