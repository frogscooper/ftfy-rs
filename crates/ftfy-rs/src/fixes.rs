//! The individual fixes that [`crate::fix_text`] can perform. Each one
//! mirrors the function of the same name in Python's `ftfy.fixes`.
//!
//! Text fixers return `Cow::Borrowed` when they leave the text alone, so the
//! common case of clean text doesn't allocate.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use regex::bytes::Regex as BytesRegex;
use regex::{Captures, Regex};

use crate::badness::{build, is_bad};
use crate::chardata::sloppy_1252_char;
use crate::tables::{
    HTML_ENTITIES, LIGATURES, UTF8_DETECTOR_LOOKBEHIND, UTF8_DETECTOR_PATTERN, WIDTH_MAP,
};
use crate::{fix_encoding, TextFixerConfig};

/// Copy `text`, letting `f` replace characters. `f` returns true if it
/// wrote a replacement for the character into the buffer (possibly nothing,
/// to delete it), or false to keep the character.
fn translate<'a>(text: &'a str, mut f: impl FnMut(char, &mut String) -> bool) -> Cow<'a, str> {
    let mut out: Option<String> = None;
    let mut replacement = String::new();
    // The text before `copied` is already in `out`.
    let mut copied = 0;
    for (i, ch) in text.char_indices() {
        if f(ch, &mut replacement) {
            let out = out.get_or_insert_with(|| String::with_capacity(text.len() + 8));
            out.push_str(&text[copied..i]);
            out.push_str(&replacement);
            replacement.clear();
            copied = i + ch.len_utf8();
        }
    }
    finish(text, out, copied)
}

/// The result of a fixer that copied the text before `copied` into `out`,
/// if it changed anything.
fn finish(text: &str, out: Option<String>, copied: usize) -> Cow<'_, str> {
    match out {
        Some(mut out) => {
            out.push_str(&text[copied..]);
            Cow::Owned(out)
        }
        None => Cow::Borrowed(text),
    }
}

static HTML_ENTITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&#?[0-9A-Za-z]{1,24};").unwrap());

/// Decode HTML entities and character references, including some nonstandard
/// ones written in all-caps, but only the unambiguous ones that end in
/// semicolons.
pub fn unescape_html(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    HTML_ENTITY_RE.replace_all(text, |caps: &Captures| {
        let entity = &caps[0];
        if let Ok(i) = HTML_ENTITIES.binary_search_by(|(name, _)| (*name).cmp(entity)) {
            HTML_ENTITIES[i].1.to_string()
        } else if entity.starts_with("&#") {
            unescape_numeric(entity).unwrap_or_else(|| entity.to_string())
        } else {
            entity.to_string()
        }
    })
}

/// Do what ftfy does with a numeric reference: run it through Python's
/// `html.unescape`, and keep the result only if the semicolon was consumed.
fn unescape_numeric(entity: &str) -> Option<String> {
    let body = &entity[2..entity.len() - 1];
    let (digits, radix) = match body.strip_prefix(['x', 'X']) {
        Some(hex) => (hex, 16),
        None => (body, 10),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let num = digits.chars().fold(0u32, |acc, c| {
        acc.saturating_mul(radix)
            .saturating_add(c.to_digit(radix).unwrap())
    });
    let replacement = html_charref(num);
    if replacement.contains(';') {
        None
    } else {
        Some(replacement)
    }
}

/// Python's `html._replace_charref` for a numeric reference.
fn html_charref(num: u32) -> String {
    match num {
        0x00 => "\u{FFFD}".to_string(),
        0x0D => "\r".to_string(),
        0x80..=0x9F => sloppy_1252_char(num as u8).to_string(),
        0xD800..=0xDFFF | 0x110000.. => "\u{FFFD}".to_string(),
        0x01..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F | 0xFDD0..=0xFDEF => String::new(),
        n if n & 0xFFFE == 0xFFFE => String::new(),
        n => char::from_u32(n).unwrap().to_string(),
    }
}

static ANSI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[\d;]*[a-zA-Z]").unwrap());

/// Strip out "ANSI" terminal escape sequences, such as those that produce
/// colored text on Unix.
pub fn remove_terminal_escapes(text: &str) -> Cow<'_, str> {
    if !text.contains('\x1b') {
        return Cow::Borrowed(text);
    }
    ANSI_RE.replace_all(text, "")
}

/// Like [`translate`], for a fixer that only replaces characters whose UTF-8
/// starts with the byte `a` or `b`. Those can be found eight bytes at a time.
fn translate_by_lead_byte(
    text: &str,
    a: u8,
    b: u8,
    f: impl Fn(char) -> Option<&'static str>,
) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut i = 0;
    loop {
        i += crate::find_either(&bytes[i..], a, b);
        if i >= bytes.len() {
            break;
        }
        // `a` and `b` are lead bytes, so this is the start of a character.
        let ch = text[i..].chars().next().unwrap();
        if let Some(rep) = f(ch) {
            let out = out.get_or_insert_with(|| String::with_capacity(text.len()));
            out.push_str(&text[copied..i]);
            out.push_str(rep);
            copied = i + ch.len_utf8();
        }
        i += ch.len_utf8();
    }
    finish(text, out, copied)
}

/// Replace curly quotation marks with straight equivalents.
pub fn uncurl_quotes(text: &str) -> Cow<'_, str> {
    // U+02BC is CA BC in UTF-8, and U+2018 to U+201F start with E2.
    translate_by_lead_byte(text, 0xCA, 0xE2, |ch| match ch {
        '\u{2BC}' | '\u{2018}'..='\u{201B}' => Some("'"),
        '\u{201C}'..='\u{201F}' => Some("\""),
        _ => None,
    })
}

fn lookup(table: &'static [(u32, &'static str)], ch: char) -> Option<&'static str> {
    table
        .binary_search_by_key(&(ch as u32), |&(cp, _)| cp)
        .ok()
        .map(|i| table[i].1)
}

/// Replace single-character ligatures of Latin letters, such as 'ﬁ', with
/// the characters that they contain.
pub fn fix_latin_ligatures(text: &str) -> Cow<'_, str> {
    if text.is_ascii() {
        return Cow::Borrowed(text);
    }
    translate(text, |ch, out| {
        if (ch as u32) < 0x132 {
            return false;
        }
        match lookup(&LIGATURES, ch) {
            Some(rep) => {
                out.push_str(rep);
                true
            }
            None => false,
        }
    })
}

/// Replace halfwidth and fullwidth forms with their standard-width forms.
pub fn fix_character_width(text: &str) -> Cow<'_, str> {
    // Everything in the width map is U+3000 or in U+FF01 to U+FFEF, which
    // start with E3 and EF in UTF-8 (checked by a test).
    translate_by_lead_byte(text, 0xE3, 0xEF, |ch| lookup(&WIDTH_MAP, ch))
}

/// Convert all line breaks (CRLF, CR, U+2028, U+2029, U+85) to Unix style.
pub fn fix_line_breaks(text: &str) -> Cow<'_, str> {
    if !text.contains(['\r', '\u{2028}', '\u{2029}', '\u{85}']) {
        return Cow::Borrowed(text);
    }
    let mut pending_cr = false;
    translate(text, |ch, out| {
        let after_cr = std::mem::replace(&mut pending_cr, ch == '\r');
        match ch {
            '\n' if after_cr => true,
            '\r' | '\u{2028}' | '\u{2029}' | '\u{85}' => {
                out.push('\n');
                true
            }
            _ => false,
        }
    })
}

/// Lone UTF-16 surrogates can't exist in a Rust string. The Python bindings
/// carry them through the pipeline as these private-use characters in plane
/// 16 (U+10F800 + the surrogate's offset from U+D800), and turn them back
/// into surrogates on the way out.
pub const SURROGATE_ESCAPE_BASE: u32 = 0x10F800;

pub(crate) fn escaped_surrogate(ch: char) -> Option<u32> {
    let cp = ch as u32;
    (cp >= SURROGATE_ESCAPE_BASE).then(|| cp - SURROGATE_ESCAPE_BASE + 0xD800)
}

/// Replace escaped surrogates (see [`SURROGATE_ESCAPE_BASE`]) with the
/// characters they represent when properly paired, or with U+FFFD otherwise.
pub fn fix_escaped_surrogates(text: &str) -> Cow<'_, str> {
    if text.is_ascii() || !text.chars().any(|ch| escaped_surrogate(ch).is_some()) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match escaped_surrogate(ch) {
            None => out.push(ch),
            Some(hi @ 0xD800..=0xDBFF) => match chars.peek().copied().and_then(escaped_surrogate) {
                Some(lo @ 0xDC00..=0xDFFF) => {
                    chars.next();
                    let cp = 0x10000 + (hi - 0xD800) * 0x400 + (lo - 0xDC00);
                    out.push(char::from_u32(cp).unwrap());
                }
                _ => out.push('\u{FFFD}'),
            },
            Some(_) => out.push('\u{FFFD}'),
        }
    }
    Cow::Owned(out)
}

/// Remove control characters that have no displayed effect on text: most
/// ASCII controls, deprecated Arabic controls, interlinear annotation
/// characters, the object replacement character, and the byte order mark.
pub fn remove_control_chars(text: &str) -> Cow<'_, str> {
    translate(text, |ch, _| is_control_char(ch))
}

// The same set as the generated CONTROL_CHARS table, as a fast match.
fn is_control_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x00..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F | 0x206A..=0x206F | 0xFEFF | 0xFFF9..=0xFFFC
    )
}

#[cfg(test)]
#[test]
fn control_chars_match_table() {
    use crate::tables::CONTROL_CHARS;
    let from_match: Vec<u32> = (0..=0x10FFFF)
        .filter_map(char::from_u32)
        .filter(|&ch| is_control_char(ch))
        .map(|ch| ch as u32)
        .collect();
    assert_eq!(from_match, CONTROL_CHARS);
}

#[cfg(test)]
#[test]
fn width_map_lead_bytes() {
    for &(cp, _) in WIDTH_MAP.iter() {
        let lead = char::from_u32(cp).unwrap().to_string().as_bytes()[0];
        assert!(lead == 0xE3 || lead == 0xEF, "U+{cp:04X}");
    }
}

/// Remove a byte-order mark that was accidentally decoded as if it were part
/// of the text.
pub fn remove_bom(text: &str) -> &str {
    text.trim_start_matches('\u{FEFF}')
}

/// Treat C1 control characters as their Windows-1252 equivalents, like Web
/// browsers do.
pub fn fix_c1_controls(text: &str) -> Cow<'_, str> {
    if text.is_ascii() {
        return Cow::Borrowed(text);
    }
    translate(text, |ch, out| {
        if ('\u{80}'..='\u{9F}').contains(&ch) {
            out.push(sloppy_1252_char(ch as u8));
            true
        } else {
            false
        }
    })
}

/// Returns true if the text contains C1 control characters.
pub(crate) fn has_c1_controls(text: &str) -> bool {
    !text.is_ascii() && text.chars().any(|ch| ('\u{80}'..='\u{9F}').contains(&ch))
}

pub(crate) static ALTERED_UTF8_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(concat!(
        r"(?-u)[\xC2\xC3\xC5\xCE\xD0\xD9]\x20",
        r"|[\xE2\xE3]\x20[\x80-\x84\x86-\x9F\xA1-\xBF]",
        r"|[\xE0-\xE3][\x80-\x84\x86-\x9F\xA1-\xBF]\x20",
        r"|\xF0\x20[\x80-\xBF][\x80-\xBF]",
        r"|\xF0[\x80-\xBF]\x20[\x80-\xBF]",
        r"|\xF0[\x80-\xBF][\x80-\xBF]\x20",
    ))
    .unwrap()
});

/// Put back byte A0 where a process converted it to a space, in sequences
/// that would otherwise convincingly decode as UTF-8.
pub fn restore_byte_a0(bytes: &[u8]) -> Vec<u8> {
    // First, the A_GRAVE_WORD_RE exception: "\xc3 " not followed by
    // " ", "quele", "quela", "quilo" or "s " becomes "\xc3\xa0 ".
    let mut fixed = Vec::with_capacity(bytes.len() + 8);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0xC3 && bytes.get(i + 1) == Some(&b' ') {
            let rest = &bytes[i + 2..];
            let excluded = [&b" "[..], b"quele", b"quela", b"quilo", b"s "]
                .iter()
                .any(|prefix| rest.starts_with(prefix));
            if !excluded {
                fixed.extend_from_slice(b"\xC3\xA0 ");
                i += 2;
                continue;
            }
        }
        fixed.push(bytes[i]);
        i += 1;
    }

    ALTERED_UTF8_RE
        .replace_all(&fixed, |caps: &regex::bytes::Captures| {
            caps[0]
                .iter()
                .map(|&b| if b == b' ' { 0xA0 } else { b })
                .collect::<Vec<u8>>()
        })
        .into_owned()
}

static LOSSY_UTF8_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
    BytesRegex::new(concat!(
        r"(?-u)[\xC2-\xDF]\x1A",
        r"|[\xC2-\xC3]\?",
        r"|\xED[\xA0-\xAF][\x1A?]\xED[\xB0-\xBF][\x1A?\x80-\xBF]",
        r"|\xED[\xA0-\xAF][\x1A?\x80-\xBF]\xED[\xB0-\xBF][\x1A?]",
        r"|[\xE0-\xEF][\x1A?][\x1A\x80-\xBF]",
        r"|[\xE0-\xEF][\x1A\x80-\xBF][\x1A?]",
        r"|[\xF0-\xF4][\x1A?][\x1A\x80-\xBF][\x1A\x80-\xBF]",
        r"|[\xF0-\xF4][\x1A\x80-\xBF][\x1A?][\x1A\x80-\xBF]",
        r"|[\xF0-\xF4][\x1A\x80-\xBF][\x1A\x80-\xBF][\x1A?]",
        r"|\x1A",
    ))
    .unwrap()
});

/// Replace UTF-8 sequences where information was lost in a "sloppy" codec
/// (marked by byte 1A, or sometimes '?') with the UTF-8 encoding of U+FFFD.
pub fn replace_lossy_sequences(bytes: &[u8]) -> Vec<u8> {
    replace_lossy_sequences_cow(bytes).into_owned()
}

pub(crate) fn replace_lossy_sequences_cow(bytes: &[u8]) -> Cow<'_, [u8]> {
    LOSSY_UTF8_RE.replace_all(bytes, &b"\xEF\xBF\xBD"[..])
}

static UTF8_DETECTOR_RE: LazyLock<Regex> = LazyLock::new(|| build(UTF8_DETECTOR_PATTERN));
static UTF8_DETECTOR_LOOKBEHIND_RE: LazyLock<Regex> =
    LazyLock::new(|| build(&format!(r"\A(?:{UTF8_DETECTOR_LOOKBEHIND})\z")));

// The lookbehind class tested ahead of time for every character below this,
// which covers all of its members.
const LOOKBEHIND_TABLE_LEN: usize = 0x2600;
static UTF8_DETECTOR_LOOKBEHIND_TABLE: LazyLock<Vec<bool>> = LazyLock::new(|| {
    (0..LOOKBEHIND_TABLE_LEN as u32)
        .map(|cp| char::from_u32(cp).is_some_and(lookbehind_regex_matches))
        .collect()
});

fn lookbehind_regex_matches(ch: char) -> bool {
    UTF8_DETECTOR_LOOKBEHIND_RE.is_match(ch.encode_utf8(&mut [0; 4]))
}

fn blocks_utf8_detector(ch: char) -> bool {
    match UTF8_DETECTOR_LOOKBEHIND_TABLE.get(ch as usize) {
        Some(&blocked) => blocked,
        None => lookbehind_regex_matches(ch),
    }
}

/// Find matches of Python's UTF8_DETECTOR_RE, including its negative
/// lookbehind, which the regex crate can't express.
fn utf8_detector_matches(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut pos = 0;
    std::iter::from_fn(move || {
        while pos <= text.len() {
            let m = UTF8_DETECTOR_RE.find_at(text, pos)?;
            let start = m.start();
            let prev = text[..start].chars().next_back();
            if prev.is_some_and(blocks_utf8_detector) {
                pos = start + text[start..].chars().next().map_or(1, char::len_utf8);
                continue;
            }
            pos = m.end();
            return Some((start, m.end()));
        }
        None
    })
}

#[doc(hidden)]
pub fn has_utf8_mojibake(text: &str) -> bool {
    utf8_detector_matches(text).next().is_some()
}

/// The same few mojibake sequences (like "â€™") tend to recur throughout a
/// text, so the fixes for short ones are remembered per thread.
const MEMO_MAX_LEN: usize = 64;
const MEMO_MAX_ENTRIES: usize = 4096;

thread_local! {
    static EMBEDDED_MOJIBAKE_MEMO: RefCell<HashMap<Box<str>, Option<Rc<str>>>> =
        RefCell::new(HashMap::new());
}

/// Fix one sequence found by the UTF-8 detector, or None to leave it alone.
fn fix_embedded_mojibake(substr: &str) -> Option<Rc<str>> {
    let compute =
        || is_bad(substr).then(|| Rc::from(fix_encoding(substr, &TextFixerConfig::no_explain())));
    if substr.len() > MEMO_MAX_LEN {
        return compute();
    }
    EMBEDDED_MOJIBAKE_MEMO.with(|memo| {
        if let Some(fixed) = memo.borrow().get(substr) {
            return fixed.clone();
        }
        let fixed = compute();
        let mut memo = memo.borrow_mut();
        if memo.len() >= MEMO_MAX_ENTRIES {
            memo.clear();
        }
        memo.insert(substr.into(), fixed.clone());
        fixed
    })
}

/// Fix sequences that distinctly look like UTF-8 mojibake, even when there's
/// no consistent way to reinterpret the whole string in a new encoding.
pub fn decode_inconsistent_utf8(text: &str) -> Cow<'_, str> {
    let mut out: Option<String> = None;
    let mut last = 0;
    for (start, end) in utf8_detector_matches(text) {
        let substr = &text[start..end];
        // Require the match to be shorter, so that this doesn't recurse forever
        if substr.len() < text.len() {
            if let Some(fixed) = fix_embedded_mojibake(substr) {
                let out = out.get_or_insert_with(|| String::with_capacity(text.len()));
                out.push_str(&text[last..start]);
                out.push_str(&fixed);
                last = end;
            }
        }
    }
    match out {
        None => Cow::Borrowed(text),
        Some(mut out) => {
            out.push_str(&text[last..]);
            Cow::Owned(out)
        }
    }
}
