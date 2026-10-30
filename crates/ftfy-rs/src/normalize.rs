//! Unicode normalization that can be made to agree with an older Unicode
//! version, such as the one in the running Python's `unicodedata`.
//!
//! Unicode's stability policy means characters never change how they
//! normalize once assigned. So the only way an older Unicode database
//! disagrees with ours is about characters it doesn't have yet: it treats
//! them as inert, never decomposing or combining with anything. Given that
//! set of characters (see [`set_unassigned`]), we normalize the text in runs
//! between them and pass them through untouched, which gives the older
//! version's result.

use std::borrow::Cow;
use std::sync::OnceLock;

use unicode_normalization::char::{
    canonical_combining_class, decompose_canonical, decompose_compatible,
};
use unicode_normalization::{
    is_nfc_quick, is_nfd_quick, is_nfkc_quick, is_nfkd_quick, IsNormalized, UnicodeNormalization,
};

use crate::Normalization;

static UNASSIGNED: OnceLock<Vec<char>> = OnceLock::new();

/// Characters whose normalization isn't trivial in our Unicode data: they
/// decompose, have a nonzero combining class, or appear in another
/// character's decomposition (so they may compose). Only these can be
/// treated differently by an older Unicode version.
pub fn candidates() -> Vec<char> {
    let mut special = vec![false; 0x110000];
    for ch in (0..=0x10FFFF).filter_map(char::from_u32) {
        let mut decomposes = false;
        decompose_compatible(ch, |d| decomposes |= d != ch);
        decompose_canonical(ch, |d| {
            if d != ch {
                special[d as usize] = true;
            }
        });
        if decomposes || canonical_combining_class(ch) != 0 {
            special[ch as usize] = true;
        }
    }
    (0..0x110000u32)
        .filter(|&cp| special[cp as usize])
        .filter_map(char::from_u32)
        .collect()
}

/// Declare the characters that should be treated as unassigned, i.e. passed
/// through normalization untouched. This can only be set once; returns false
/// if it was already set.
pub fn set_unassigned(mut chars: Vec<char>) -> bool {
    chars.sort_unstable();
    chars.dedup();
    UNASSIGNED.set(chars).is_ok()
}

/// Apply Unicode normalization, returning Borrowed if the text was already
/// normalized.
pub fn normalize(text: &str, form: Normalization) -> Cow<'_, str> {
    if text.is_ascii() {
        return Cow::Borrowed(text);
    }
    let unassigned = UNASSIGNED.get().map_or(&[][..], Vec::as_slice);
    let is_unassigned = |ch: &char| unassigned.binary_search(ch).is_ok();
    if unassigned.is_empty() || !text.chars().any(|ch| is_unassigned(&ch)) {
        return normalize_with_our_data(text, form);
    }

    let mut out = String::with_capacity(text.len());
    let mut run_start = 0;
    for (i, ch) in text.char_indices() {
        if is_unassigned(&ch) {
            out.push_str(&normalize_with_our_data(&text[run_start..i], form));
            out.push(ch);
            run_start = i + ch.len_utf8();
        }
    }
    out.push_str(&normalize_with_our_data(&text[run_start..], form));
    if out == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(out)
    }
}

fn normalize_with_our_data(text: &str, form: Normalization) -> Cow<'_, str> {
    if text.is_ascii() {
        return Cow::Borrowed(text);
    }
    let quick = match form {
        Normalization::Nfc => is_nfc_quick(text.chars()),
        Normalization::Nfd => is_nfd_quick(text.chars()),
        Normalization::Nfkc => is_nfkc_quick(text.chars()),
        Normalization::Nfkd => is_nfkd_quick(text.chars()),
    };
    if quick == IsNormalized::Yes {
        return Cow::Borrowed(text);
    }
    let normalized: String = match form {
        Normalization::Nfc => text.nfc().collect(),
        Normalization::Nfd => text.nfd().collect(),
        Normalization::Nfkc => text.nfkc().collect(),
        Normalization::Nfkd => text.nfkd().collect(),
    };
    if normalized == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(normalized)
    }
}
