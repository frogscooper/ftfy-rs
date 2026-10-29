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

static ENCODERS: LazyLock<Vec<HashMap<char, u8>>> = LazyLock::new(|| {
    CHARMAPS
        .iter()
        .map(|(_, table)| {
            let mut map = HashMap::new();
            for (byte, &cp) in table.iter().enumerate() {
                if cp != NO_CHAR && cp >= 0x80 {
                    map.insert(char::from_u32(cp).unwrap(), byte as u8);
                }
            }
            map
        })
        .collect()
});

/// Encode text with the single-byte codec at `index` in the tables.
pub(crate) fn encode_charmap(text: &str, index: usize) -> Option<Vec<u8>> {
    let table = &CHARMAPS[index].1;
    let encoder = &ENCODERS[index];
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let cp = ch as u32;
        if cp < 0x80 && table[cp as usize] == cp {
            out.push(cp as u8);
        } else {
            out.push(*encoder.get(&ch)?);
        }
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
            0xC0 if bytes.get(i + 1) == Some(&0x80) => {
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

fn is_cesu8_pair(b: &[u8]) -> bool {
    b.len() >= 6
        && b[0] == 0xED
        && (0xA0..=0xAF).contains(&b[1])
        && (0x80..=0xBF).contains(&b[2])
        && b[3] == 0xED
        && (0xB0..=0xBF).contains(&b[4])
        && (0x80..=0xBF).contains(&b[5])
}
