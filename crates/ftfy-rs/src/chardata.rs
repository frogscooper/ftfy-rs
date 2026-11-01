//! Character data and single-byte codecs: which encodings a string could have
//! come from, and encoding/decoding with them.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::tables::{CHARMAPS, NO_CHAR};

/// The single-byte encodings ftfy tries when fixing mojibake, in the order
/// it tries them. Indexes into this list are used as bit positions in the
/// masks returned by [`possible_encodings`].
pub const CHARMAP_ENCODINGS: [&str; 10] = [
    "latin-1",
    "sloppy-windows-1252",
    "sloppy-windows-1251",
    "sloppy-windows-1250",
    "sloppy-windows-1253",
    "sloppy-windows-1254",
    "sloppy-windows-1257",
    "iso-8859-2",
    "macroman",
    "cp437",
];

pub(crate) const LATIN_1: usize = 0;
pub(crate) const SLOPPY_WINDOWS_1252: usize = 1;
/// Strict windows-1252, which comes after the CHARMAP_ENCODINGS in the tables.
pub(crate) const WINDOWS_1252: usize = 10;

const ALL_ENCODINGS: u16 = (1 << CHARMAP_ENCODINGS.len()) - 1;

// Every character that any of the codecs can produce is below this, except
// for a handful that go in MASK_EXTRA.
const MASK_DIRECT_LEN: usize = 0x2600;

struct EncodingMasks {
    direct: Vec<u16>,
    extra: HashMap<char, u16>,
}

static MASKS: LazyLock<EncodingMasks> = LazyLock::new(|| {
    let mut direct = vec![0u16; MASK_DIRECT_LEN];
    let mut extra = HashMap::new();
    // ASCII encodes as itself everywhere, except that U+001A gets its bit
    // only from the codecs whose byte 0x1A decodes to it (the non-sloppy ones).
    direct[..0x80].fill(ALL_ENCODINGS);
    direct[0x1A] = 0;
    for (i, (_, table)) in CHARMAPS[..CHARMAP_ENCODINGS.len()].iter().enumerate() {
        for byte in (0x80..=0xFF).chain([0x1A]) {
            let cp = table[byte];
            if cp == NO_CHAR {
                continue;
            }
            if (cp as usize) < MASK_DIRECT_LEN {
                direct[cp as usize] |= 1 << i;
            } else {
                *extra.entry(char::from_u32(cp).unwrap()).or_insert(0) |= 1 << i;
            }
        }
    }
    EncodingMasks { direct, extra }
});

/// Returns a bitmask of the [`CHARMAP_ENCODINGS`] that could have produced
/// this text, i.e. those that can encode every character of it.
pub fn possible_encodings(text: &str) -> u16 {
    let masks = &*MASKS;
    let mut mask = ALL_ENCODINGS;
    for ch in text.chars() {
        let cp = ch as usize;
        mask &= if cp < MASK_DIRECT_LEN {
            masks.direct[cp]
        } else {
            masks.extra.get(&ch).copied().unwrap_or(0)
        };
        if mask == 0 {
            break;
        }
    }
    mask
}

/// Check whether `text` could be encoded in `encoding`, which may be "ascii"
/// or one of the [`CHARMAP_ENCODINGS`].
pub fn possible_encoding(text: &str, encoding: &str) -> Option<bool> {
    if encoding == "ascii" {
        return Some(text.is_ascii());
    }
    let index = CHARMAP_ENCODINGS.iter().position(|&e| e == encoding)?;
    Some(possible_encodings(text) & (1 << index) != 0)
}

pub(crate) fn has_bit(mask: u16, index: usize) -> bool {
    mask & (1 << index) != 0
}

const NO_BYTE: u16 = 0xFFFF;

/// A single-byte encoder: a direct lookup table for characters below
/// MASK_DIRECT_LEN, and a short sorted list for the rest.
struct Encoder {
    direct: Vec<u16>,
    extra: Vec<(char, u8)>,
}

impl Encoder {
    fn new(table: &[u32; 256]) -> Self {
        let mut direct = vec![NO_BYTE; MASK_DIRECT_LEN];
        let mut extra = Vec::new();
        for cp in 0..0x80 {
            if table[cp] == cp as u32 {
                direct[cp] = cp as u16;
            }
        }
        for (byte, &cp) in table.iter().enumerate() {
            if cp == NO_CHAR || cp < 0x80 {
                continue;
            }
            if (cp as usize) < MASK_DIRECT_LEN {
                direct[cp as usize] = byte as u16;
            } else {
                extra.push((char::from_u32(cp).unwrap(), byte as u8));
            }
        }
        extra.sort_unstable();
        Encoder { direct, extra }
    }

    fn encode(&self, ch: char) -> Option<u8> {
        let cp = ch as usize;
        if cp < MASK_DIRECT_LEN {
            let byte = self.direct[cp];
            (byte != NO_BYTE).then_some(byte as u8)
        } else {
            let i = self.extra.binary_search_by_key(&ch, |&(c, _)| c).ok()?;
            Some(self.extra[i].1)
        }
    }
}

static ENCODERS: LazyLock<Vec<Encoder>> = LazyLock::new(|| {
    CHARMAPS
        .iter()
        .map(|(_, table)| Encoder::new(table))
        .collect()
});

/// Encode text with the single-byte codec at `index` in the tables.
pub(crate) fn encode_charmap(text: &str, index: usize) -> Option<Vec<u8>> {
    let encoder = &ENCODERS[index];
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        out.push(encoder.encode(ch)?);
    }
    Some(out)
}

/// Decode bytes with the single-byte codec at `index` in the tables.
pub(crate) fn decode_charmap(bytes: &[u8], index: usize) -> Option<String> {
    let table = &CHARMAPS[index].1;
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from_u32(table[b as usize])?);
    }
    Some(out)
}

/// Decode a single byte with sloppy-windows-1252.
pub(crate) fn sloppy_1252_char(byte: u8) -> char {
    char::from_u32(CHARMAPS[SLOPPY_WINDOWS_1252].1[byte as usize]).unwrap()
}

/// Decode "utf-8-variants": standard UTF-8, plus CESU-8 surrogate pairs and
/// Java's two-byte encoding of U+0000. Returns None where Python would raise
/// UnicodeDecodeError.
pub fn decode_utf8_variants(bytes: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(bytes.len());
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            0xC0 if bytes.get(i + 1) == Some(&0x80) || &bytes[i..] == b"\xC0\n" => {
                out.push_str(std::str::from_utf8(&bytes[start..i]).ok()?);
                out.push('\0');
                i += 2;
                start = i;
            }
            0xED if is_cesu8_pair(&bytes[i..]) => {
                out.push_str(std::str::from_utf8(&bytes[start..i]).ok()?);
                let b = &bytes[i..i + 6];
                let cp = (((b[1] & 0x0F) as u32) << 16)
                    + (((b[2] & 0x3F) as u32) << 10)
                    + (((b[4] & 0x0F) as u32) << 6)
                    + ((b[5] & 0x3F) as u32)
                    + 0x10000;
                out.push(char::from_u32(cp)?);
                i += 6;
                start = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(std::str::from_utf8(&bytes[start..]).ok()?);
    Some(out)
}

// Python ftfy finds these sequences with regexes that allow `$` in place of
// each byte after the first, meant for input that's cut off. But in Python,
// `$` also matches before a newline at the very end, so when the input ends
// in the first 5 bytes of a CESU-8 pair plus "\n", or in "\xC0\n", the
// newline is taken as the last byte of the sequence. We do the same.
fn is_cesu8_pair(b: &[u8]) -> bool {
    b.len() >= 6
        && b[0] == 0xED
        && (0xA0..=0xAF).contains(&b[1])
        && (0x80..=0xBF).contains(&b[2])
        && b[3] == 0xED
        && (0xB0..=0xBF).contains(&b[4])
        && ((0x80..=0xBF).contains(&b[5]) || b.len() == 6 && b[5] == b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_variants_trailing_newline_quirk() {
        // As in Python ftfy, a final newline is eaten as part of the sequence.
        let decode = |b: &[u8]| decode_utf8_variants(b);
        assert_eq!(
            decode(b"\xed\xa0\xbd\xed\xb8\n").as_deref(),
            Some("\u{1F60A}")
        );
        assert_eq!(decode(b"null\xc0\n").as_deref(), Some("null\0"));
        // But only at the very end.
        assert_eq!(decode(b"\xed\xa0\xbd\xed\xb8\nx"), None);
        assert_eq!(decode(b"\xc0\nx"), None);
    }

    #[test]
    fn charmaps_never_map_two_bytes_to_one_char() {
        // The encoders rely on this: otherwise which byte a character
        // encodes to would depend on table order.
        for (name, table) in CHARMAPS.iter() {
            let mut seen = std::collections::HashSet::new();
            for &cp in table.iter().filter(|&&cp| cp != NO_CHAR) {
                assert!(seen.insert(cp), "{name} maps two bytes to U+{cp:04X}");
            }
        }
    }

    #[test]
    fn encoders_round_trip() {
        for (index, (name, table)) in CHARMAPS.iter().enumerate() {
            for (byte, &cp) in table.iter().enumerate() {
                if let Some(ch) = char::from_u32(cp) {
                    let encoded = encode_charmap(ch.encode_utf8(&mut [0; 4]), index);
                    assert_eq!(encoded, Some(vec![byte as u8]), "{name} byte {byte:02X}");
                }
            }
        }
    }
}
