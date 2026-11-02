//! The heuristic that detects likely mojibake.
//!
//! The regex itself is generated from Python ftfy's `BADNESS_RE`; see
//! `scripts/gen_tables.py`. So are the tables for a prefilter that rules out
//! most correct text without running the regex.

use std::sync::LazyLock;

use regex::{Regex, RegexBuilder};

use crate::tables::{BADNESS_CLASSES, BADNESS_FOLLOWS, BADNESS_PATTERN, BADNESS_SINGLE};

pub(crate) fn build(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .size_limit(1 << 26)
        .dfa_size_limit(1 << 26)
        .build()
        .expect("generated regex should compile")
}

static BADNESS_RE: LazyLock<Regex> = LazyLock::new(|| build(BADNESS_PATTERN));

/// The prefilter class of each character in the Basic Multilingual Plane,
/// where all the classes are. Class 0 is every character that's in no
/// necessary condition for a match, including all of ASCII.
static CLASSES: [u8; 0x10000] = {
    let mut table = [0u8; 0x10000];
    let mut i = 0;
    while i < BADNESS_CLASSES.len() {
        let (start, end, class) = BADNESS_CLASSES[i];
        assert!(start >= 0x80 && end < 0x10000 && (class as usize) < BADNESS_SINGLE.len());
        let mut cp = start;
        while cp <= end {
            table[cp as usize] = class;
            cp += 1;
        }
        i += 1;
    }
    table
};

#[inline]
pub(crate) fn class(cp: u32) -> u8 {
    if cp < 0x10000 {
        CLASSES[cp as usize]
    } else {
        0
    }
}

/// Whether a character of class `class`, right after one of class `prev`,
/// meets a necessary condition for a badness match. Every match contains
/// such a character.
#[inline]
pub(crate) fn is_trigger(prev: u8, class: u8) -> bool {
    BADNESS_SINGLE[class as usize] || BADNESS_FOLLOWS[prev as usize] >> class & 1 != 0
}

/// Whether the text meets any of the necessary conditions for a badness
/// match. Text that doesn't can't be bad, and that's much cheaper to check
/// than running the regex.
pub(crate) fn has_trigger(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut prev = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        // The text is valid UTF-8, and the classes are all two or three
        // byte sequences.
        let (cp, len) = match b {
            0x00..=0x7F => {
                prev = 0;
                i += 1;
                continue;
            }
            0xC0..=0xDF => (((b & 0x1F) as u32) << 6 | (bytes[i + 1] & 0x3F) as u32, 2),
            0xE0..=0xEF => (
                ((b & 0x0F) as u32) << 12
                    | ((bytes[i + 1] & 0x3F) as u32) << 6
                    | (bytes[i + 2] & 0x3F) as u32,
                3,
            ),
            _ => (0x10000, 4),
        };
        let class = class(cp);
        if is_trigger(prev, class) {
            return true;
        }
        prev = class;
        i += len;
    }
    false
}

/// Count the number of unlikely character sequences in the text. A badness
/// greater than 0 indicates that some of it seems to be mojibake.
pub fn badness(text: &str) -> usize {
    if !has_trigger(text) {
        return 0;
    }
    BADNESS_RE.find_iter(text).count()
}

/// Returns true if the text looks like it contains mojibake.
pub fn is_bad(text: &str) -> bool {
    has_trigger(text) && BADNESS_RE.is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_trigger_decodes_utf8() {
        for ch in (0..=0x10FFFF).filter_map(char::from_u32) {
            let s = format!("a{ch}b");
            let single = BADNESS_SINGLE[class(ch as u32) as usize];
            assert_eq!(has_trigger(&s), single, "U+{:04X}", ch as u32);
        }
    }

    /// Ordinary text in many languages shouldn't need the regex.
    #[test]
    fn correct_text_has_no_trigger() {
        let samples = [
            "She said it’s “encouraging” — really… • ok",
            "À la fin de la journée, « très » 50 €. naïve, Ça",
            "Комитет собрался во вторник. ВГРС Ёё «Привет»",
            "委员会周二开会，“令人鼓舞”。",
            "Αθήνα ° © ® ½ ¿Qué? ¡Sí! Straße",
        ];
        for text in samples {
            assert!(!has_trigger(text), "{text}");
        }
    }

    /// Every match of the regex on mojibake contains a trigger.
    #[test]
    fn matches_contain_a_trigger() {
        let samples = [
            "The cafÃ© is open",
            "Itâ€™s late",
            "ÐšÐ¾Ð¼Ð¸Ñ‚ÐµÑ‚",
            "Ã¡Ã©Ã\u{AD}Ã³Ãº",
            "â”œâ”€",
            "Ã\u{A0}",
        ];
        for text in samples {
            assert!(is_bad(text), "{text:?}");
            for m in BADNESS_RE.find_iter(text) {
                assert!(has_trigger(m.as_str()), "{:?} in {text:?}", m.as_str());
            }
        }
    }
}
