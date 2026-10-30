//! A single pass over the text that finds which fixers could possibly change
//! it, so the pipeline can skip the rest without scanning the text again for
//! each one.
//!
//! A clear bit guarantees the fixer would leave the text unchanged; a set bit
//! only means it has to run.

pub const AMPERSAND: u16 = 1 << 0;
pub const ESCAPE: u16 = 1 << 1;
pub const C1_CONTROL: u16 = 1 << 2;
pub const LIGATURE: u16 = 1 << 3;
pub const WIDTH: u16 = 1 << 4;
pub const CURLY_QUOTE: u16 = 1 << 5;
pub const LINE_BREAK: u16 = 1 << 6;
pub const CONTROL: u16 = 1 << 7;
pub const ESCAPED_SURROGATE: u16 = 1 << 8;
pub const NON_ASCII: u16 = 1 << 9;
/// Something at or above U+0300. Text without such characters is already
/// in NFC: nothing below it decomposes or combines.
pub const ABOVE_U0300: u16 = 1 << 10;

const fn ascii_bits(b: u8) -> u16 {
    match b {
        b'&' => AMPERSAND,
        0x1B => ESCAPE | CONTROL,
        b'\r' => LINE_BREAK,
        0x00..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F => CONTROL,
        _ => 0,
    }
}

static ASCII_BITS: [u16; 128] = {
    let mut table = [0; 128];
    let mut i = 0;
    while i < 128 {
        table[i] = ascii_bits(i as u8);
        i += 1;
    }
    table
};

fn non_ascii_bits(ch: char) -> u16 {
    let cp = ch as u32;
    let bits = NON_ASCII
        | match cp {
            0x85 => C1_CONTROL | LINE_BREAK,
            0x80..=0x9F => C1_CONTROL,
            0x132..=0x1F3 => LIGATURE,
            0x2BC | 0x2018..=0x201F => CURLY_QUOTE,
            0x2028 | 0x2029 => LINE_BREAK,
            0x206A..=0x206F | 0xFEFF | 0xFFF9..=0xFFFC => CONTROL,
            0x3000 | 0xFF01..=0xFFEF => WIDTH,
            0xFB00..=0xFB06 => LIGATURE,
            0x10F800.. => ESCAPED_SURROGATE,
            _ => 0,
        };
    if cp >= 0x300 {
        bits | ABOVE_U0300
    } else {
        bits
    }
}

pub fn scan(text: &str) -> u16 {
    let bytes = text.as_bytes();
    let mut bits = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            bits |= ASCII_BITS[b as usize];
            i += 1;
        } else {
            // `i` is on a char boundary, since we only step over whole chars.
            let ch = text[i..].chars().next().unwrap();
            bits |= non_ascii_bits(ch);
            i += ch.len_utf8();
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixes;
    use crate::Normalization;

    /// Every character whose bit is clear must be left alone by the fixer.
    #[test]
    fn clear_bits_mean_no_change() {
        type Fixer = fn(&str) -> std::borrow::Cow<'_, str>;
        let fixers: [(u16, Fixer); 9] = [
            (AMPERSAND, fixes::unescape_html),
            (ESCAPE, fixes::remove_terminal_escapes),
            (C1_CONTROL, fixes::fix_c1_controls),
            (LIGATURE, fixes::fix_latin_ligatures),
            (WIDTH, fixes::fix_character_width),
            (CURLY_QUOTE, fixes::uncurl_quotes),
            (LINE_BREAK, fixes::fix_line_breaks),
            (CONTROL, fixes::remove_control_chars),
            (ESCAPED_SURROGATE, fixes::fix_escaped_surrogates),
        ];
        for ch in (0..=0x10FFFF).filter_map(char::from_u32) {
            let s = ch.to_string();
            let bits = scan(&s);
            for (bit, fixer) in fixers {
                if bits & bit == 0 {
                    assert_eq!(fixer(&s), s, "U+{:04X}", ch as u32);
                }
            }
            if bits & ABOVE_U0300 == 0 {
                assert_eq!(crate::normalize(&s, Normalization::Nfc), s, "U+{:04X}", ch as u32);
            }
            assert_eq!(bits & NON_ASCII != 0, !ch.is_ascii());
        }
    }
}
