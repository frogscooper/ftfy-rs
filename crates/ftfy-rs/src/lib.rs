//! ftfy: fixes text for you. A faithful Rust port of Python's
//! [ftfy](https://github.com/rspeer/python-ftfy) 6.3.
//!
//! ```
//! use ftfy_rs::{fix_text, TextFixerConfig};
//!
//! let config = TextFixerConfig::default();
//! assert_eq!(fix_text("âœ” No problems", &config), "✔ No problems");
//! assert_eq!(fix_text("schÃ¶n", &config), "schön");
//! ```
//!
//! The goal is identical output to Python ftfy for every input, including
//! its explanations, so the behavior follows the Python implementation
//! step by step -- quirks included.

use std::borrow::Cow;

pub mod badness;
pub mod chardata;
pub mod fixes;
pub mod normalize;
mod scan;
mod tables;

pub use badness::{badness, is_bad};
pub use chardata::{decode_utf8_variants, possible_encoding, CHARMAP_ENCODINGS};
pub use normalize::normalize;

use chardata::{decode_charmap, encode_charmap, has_bit, possible_encodings, LATIN_1, WINDOWS_1252};

/// When to decode HTML entities such as `&amp;`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnescapeHtml {
    /// Decode them, unless the text contains a literal `<`, which suggests
    /// it's actual HTML whose entities should be preserved.
    Auto,
    Always,
    Never,
}

/// A Unicode normalization form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Normalization {
    Nfc,
    Nfd,
    Nfkc,
    Nfkd,
}

impl Normalization {
    pub fn name(self) -> &'static str {
        match self {
            Normalization::Nfc => "NFC",
            Normalization::Nfd => "NFD",
            Normalization::Nfkc => "NFKC",
            Normalization::Nfkd => "NFKD",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "NFC" => Some(Normalization::Nfc),
            "NFD" => Some(Normalization::Nfd),
            "NFKC" => Some(Normalization::Nfkc),
            "NFKD" => Some(Normalization::Nfkd),
            _ => None,
        }
    }
}

/// Configuration options for ftfy. See the documentation of Python ftfy's
/// `TextFixerConfig` for what each option does; the defaults are the same.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFixerConfig {
    pub unescape_html: UnescapeHtml,
    pub remove_terminal_escapes: bool,
    pub fix_encoding: bool,
    pub restore_byte_a0: bool,
    pub replace_lossy_sequences: bool,
    pub decode_inconsistent_utf8: bool,
    pub fix_c1_controls: bool,
    pub fix_latin_ligatures: bool,
    pub fix_character_width: bool,
    pub uncurl_quotes: bool,
    pub fix_line_breaks: bool,
    pub fix_surrogates: bool,
    pub remove_control_chars: bool,
    pub normalization: Option<Normalization>,
    pub max_decode_length: usize,
    pub explain: bool,
    /// Set by the Python bindings when the input contained lone surrogates,
    /// which travel through the pipeline as private-use characters. See
    /// [`fixes::SURROGATE_ESCAPE_BASE`].
    #[doc(hidden)]
    pub surrogates_escaped: bool,
}

impl Default for TextFixerConfig {
    fn default() -> Self {
        TextFixerConfig {
            unescape_html: UnescapeHtml::Auto,
            remove_terminal_escapes: true,
            fix_encoding: true,
            restore_byte_a0: true,
            replace_lossy_sequences: true,
            decode_inconsistent_utf8: true,
            fix_c1_controls: true,
            fix_latin_ligatures: true,
            fix_character_width: true,
            uncurl_quotes: true,
            fix_line_breaks: true,
            fix_surrogates: true,
            remove_control_chars: true,
            normalization: Some(Normalization::Nfc),
            max_decode_length: 1_000_000,
            explain: true,
            surrogates_escaped: false,
        }
    }
}

impl TextFixerConfig {
    /// The default configuration with explanations turned off, which is what
    /// Python ftfy uses when a function isn't given a config.
    pub fn no_explain() -> Self {
        TextFixerConfig {
            explain: false,
            ..Default::default()
        }
    }
}

/// One step of an explanation of how text was fixed. `action` is "encode",
/// "decode", "transcode", "apply" or "normalize", and `parameter` is the
/// encoding, function name or normalization form it applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExplanationStep {
    pub action: &'static str,
    pub parameter: &'static str,
}

const fn step(action: &'static str, parameter: &'static str) -> ExplanationStep {
    ExplanationStep { action, parameter }
}

/// Fix text in independent segments (lines, or chunks of at most
/// `max_decode_length` characters), returning the fixed text.
pub fn fix_text(text: &str, config: &TextFixerConfig) -> String {
    let mut config = Cow::Borrowed(config);
    let max_len = config.max_decode_length.max(1);
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while pos < text.len() {
        let rest = &text[pos..];
        let mut end = rest.find('\n').map_or(rest.len(), |i| i + 1);
        // Lengths are in codepoints, as in Python. A segment can only be too
        // long in codepoints if it's too long in bytes.
        if end > max_len {
            if let Some((i, _)) = rest[..end].char_indices().nth(max_len) {
                end = i;
            }
        }
        let segment = &rest[..end];
        // Like Python ftfy, once a segment contains '<', HTML unescaping
        // stays off for all the following segments too.
        if config.unescape_html == UnescapeHtml::Auto && segment.contains('<') {
            config.to_mut().unescape_html = UnescapeHtml::Never;
        }
        if is_trivially_clean(segment) {
            out.push_str(segment);
        } else {
            out.push_str(&fix_and_explain(segment, &config).0);
        }
        pos += end;
    }
    out
}

/// Text that no fixer would change, whatever the config: printable ASCII
/// plus tab, newline and form feed, without any '&'.
fn is_trivially_clean(text: &str) -> bool {
    text.bytes()
        .all(|b| (0x20..0x7F).contains(&b) && b != b'&' || matches!(b, b'\t' | b'\n' | 0x0C))
}

/// Fix text as a single segment, discarding the explanation.
pub fn fix_text_segment(text: &str, config: &TextFixerConfig) -> String {
    fix_and_explain(text, config).0
}

/// Fix text as a single segment, returning the fixed text and, if
/// `config.explain` is set, a list of the steps that fixed it.
pub fn fix_and_explain(
    text: &str,
    config: &TextFixerConfig,
) -> (String, Option<Vec<ExplanationStep>>) {
    let mut config = Cow::Borrowed(config);
    if config.unescape_html == UnescapeHtml::Auto && text.contains('<') {
        config.to_mut().unescape_html = UnescapeHtml::Never;
    }
    let mut steps = config.explain.then(Vec::new);
    if is_trivially_clean(text) {
        return (text.to_string(), steps);
    }

    let mut current = text.to_string();
    loop {
        let mut text = Cow::Borrowed(current.as_str());
        // Which fixers could change the text; rescanned whenever it changes.
        let mut bits = scan::scan(&text);

        if config.unescape_html != UnescapeHtml::Never && bits & scan::AMPERSAND != 0 {
            try_fix(&mut text, &mut bits, fixes::unescape_html, "unescape_html", &mut steps);
        }

        if config.fix_encoding && bits & scan::NON_ASCII != 0 {
            let fixed = match &mut steps {
                // Python ftfy calls fix_encoding() here without passing on
                // the config, so the encoding sub-options only take effect
                // when explanations are on.
                None => into_changed(fix_encoding_cow(&text, &TextFixerConfig::no_explain())),
                Some(steps) => {
                    let (fixed, encoding_steps) = fix_encoding_and_explain_cow(&text, &config);
                    steps.extend(encoding_steps);
                    into_changed(fixed)
                }
            };
            if let Some(fixed) = fixed {
                bits = scan::scan(&fixed);
                text = Cow::Owned(fixed);
            }
        }

        let fixers: [(bool, u16, Fixer, &str); 8] = [
            (config.fix_c1_controls, scan::C1_CONTROL, fixes::fix_c1_controls, "fix_c1_controls"),
            (config.fix_latin_ligatures, scan::LIGATURE, fixes::fix_latin_ligatures, "fix_latin_ligatures"),
            (config.fix_character_width, scan::WIDTH, fixes::fix_character_width, "fix_character_width"),
            (config.uncurl_quotes, scan::CURLY_QUOTE, fixes::uncurl_quotes, "uncurl_quotes"),
            (config.fix_line_breaks, scan::LINE_BREAK, fixes::fix_line_breaks, "fix_line_breaks"),
            (
                config.fix_surrogates && config.surrogates_escaped,
                scan::ESCAPED_SURROGATE,
                fixes::fix_escaped_surrogates,
                "fix_surrogates",
            ),
            (
                config.remove_terminal_escapes,
                scan::ESCAPE,
                fixes::remove_terminal_escapes,
                "remove_terminal_escapes",
            ),
            (config.remove_control_chars, scan::CONTROL, fixes::remove_control_chars, "remove_control_chars"),
        ];
        for (enabled, bit, fixer, name) in fixers {
            if enabled && bits & bit != 0 {
                try_fix(&mut text, &mut bits, fixer, name, &mut steps);
            }
        }

        if let Some(form) = config.normalization {
            let already_nfc = form == Normalization::Nfc && bits & scan::ABOVE_U0300 == 0;
            if bits & scan::NON_ASCII != 0 && !already_nfc {
                if let Some(fixed) = into_changed(normalize(&text, form)) {
                    if let Some(steps) = &mut steps {
                        steps.push(step("normalize", form.name()));
                    }
                    text = Cow::Owned(fixed);
                }
            }
        }

        let next = match text {
            Cow::Borrowed(_) => None,
            Cow::Owned(next) => Some(next),
        };
        match next {
            Some(next) if next != current => current = next,
            _ => return (current, steps),
        }
    }
}

type Fixer = fn(&str) -> Cow<'_, str>;

/// The new text if a fixer allocated one, or None if it returned its input.
fn into_changed(text: Cow<'_, str>) -> Option<String> {
    match text {
        Cow::Borrowed(_) => None,
        Cow::Owned(text) => Some(text),
    }
}

/// Apply a fixer, recording it and rescanning the text if it changed.
fn try_fix(
    text: &mut Cow<'_, str>,
    bits: &mut u16,
    fixer: Fixer,
    name: &'static str,
    steps: &mut Option<Vec<ExplanationStep>>,
) {
    let Some(fixed) = into_changed(fixer(text)) else {
        return;
    };
    if fixed == **text {
        return;
    }
    if let Some(steps) = steps {
        steps.push(step("apply", name));
    }
    *bits = scan::scan(&fixed);
    *text = Cow::Owned(fixed);
}

/// Apply just the encoding-fixing steps of ftfy, discarding the explanation.
pub fn fix_encoding(text: &str, config: &TextFixerConfig) -> String {
    fix_encoding_cow(text, config).into_owned()
}

fn fix_encoding_cow<'a>(text: &'a str, config: &TextFixerConfig) -> Cow<'a, str> {
    fix_encoding_and_explain_cow(text, config).0
}

/// Detect mojibake and fix it, returning the fixed text and a list of the
/// steps that fixed it.
pub fn fix_encoding_and_explain(
    text: &str,
    config: &TextFixerConfig,
) -> (String, Vec<ExplanationStep>) {
    let (fixed, plan) = fix_encoding_and_explain_cow(text, config);
    (fixed.into_owned(), plan)
}

fn fix_encoding_and_explain_cow<'a>(
    text: &'a str,
    config: &TextFixerConfig,
) -> (Cow<'a, str>, Vec<ExplanationStep>) {
    let mut plan_so_far = Vec::new();
    if !config.fix_encoding {
        return (Cow::Borrowed(text), plan_so_far);
    }
    let mut current = Cow::Borrowed(text);
    loop {
        let (fixed, plan) = fix_encoding_one_step(&current, config);
        plan_so_far.extend(plan);
        match fixed {
            Some(fixed) if fixed != *current => current = Cow::Owned(fixed),
            _ => return (current, plan_so_far),
        }
    }
}

/// One step of fixing the encoding of text. Returns None for the text if
/// it's unchanged.
fn fix_encoding_one_step(
    text: &str,
    config: &TextFixerConfig,
) -> (Option<String>, Vec<ExplanationStep>) {
    // ASCII text, and text that doesn't look like mojibake, stays unchanged.
    if text.is_empty() || text.is_ascii() || !is_bad(text) {
        return (None, vec![]);
    }

    // Suppose the text was supposed to be UTF-8, but it was decoded using a
    // single-byte encoding instead.
    let possible = possible_encodings(text);
    for (i, &encoding) in CHARMAP_ENCODINGS.iter().enumerate() {
        if !has_bit(possible, i) {
            continue;
        }
        let mut bytes = encode_charmap(text, i).expect("encodability was checked");
        let mut steps = vec![step("encode", encoding)];

        // Put back byte A0 where it was turned into a space. Not in MacRoman,
        // where it would match an en dash followed by a space.
        if config.restore_byte_a0
            && encoding != "macroman"
            && fixes::ALTERED_UTF8_RE.is_match(&bytes)
        {
            let replaced = fixes::restore_byte_a0(&bytes);
            if replaced != bytes {
                steps.push(step("transcode", "restore_byte_a0"));
                bytes = replaced;
            }
        }

        if config.replace_lossy_sequences && encoding.starts_with("sloppy") {
            if let Cow::Owned(replaced) = fixes::replace_lossy_sequences_cow(&bytes) {
                if replaced != bytes {
                    steps.push(step("transcode", "replace_lossy_sequences"));
                    bytes = replaced;
                }
            }
        }

        let (decoding, decoded) = if bytes.contains(&0xED) || bytes.contains(&0xC0) {
            ("utf-8-variants", decode_utf8_variants(&bytes))
        } else {
            ("utf-8", String::from_utf8(bytes).ok())
        };
        if let Some(fixed) = decoded {
            steps.push(step("decode", decoding));
            return (Some(fixed), steps);
        }
    }

    // Look for a-hat-euro sequences that remain, and fix them in isolation.
    if config.decode_inconsistent_utf8 && fixes::has_utf8_mojibake(text) {
        if let Cow::Owned(fixed) = fixes::decode_inconsistent_utf8(text) {
            if fixed != text {
                return (Some(fixed), vec![step("apply", "decode_inconsistent_utf8")]);
            }
        }
    }

    // Latin-1 that was intended to be read as Windows-1252. (Python ftfy
    // first checks whether "windows-1252" is among the possible encodings,
    // but it never is, since only the sloppy version is in the list.)
    if has_bit(possible, LATIN_1) {
        let bytes = encode_charmap(text, LATIN_1).expect("encodability was checked");
        if let Some(fixed) = decode_charmap(&bytes, WINDOWS_1252) {
            if fixed != text {
                let steps = vec![step("encode", "latin-1"), step("decode", "windows-1252")];
                return (Some(fixed), steps);
            }
        }
    }

    // Fix individual C1 control characters.
    if config.fix_c1_controls && fixes::has_c1_controls(text) {
        let fixed = fixes::fix_c1_controls(text).into_owned();
        return (Some(fixed), vec![step("transcode", "fix_c1_controls")]);
    }

    (None, vec![])
}
