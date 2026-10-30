//! Python bindings for ftfy-rs. The Python package `ftfy_rs` wraps these
//! functions with the same API as Python ftfy.

use std::borrow::Cow;

use ftfy_rs::fixes::{self, SURROGATE_ESCAPE_BASE};
use ftfy_rs::{ExplanationStep, Normalization, TextFixerConfig, UnescapeHtml};
use pyo3::exceptions::{PyKeyError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

/// Texts longer than this are fixed with the GIL released.
const RELEASE_GIL_LEN: usize = 4096;

/// A Python string as Rust text. Lone surrogates, which Rust strings can't
/// hold, are escaped as private-use characters (see SURROGATE_ESCAPE_BASE).
struct Text<'a> {
    text: Cow<'a, str>,
    escaped: bool,
}

fn extract_text<'a>(s: &'a Bound<'_, PyString>) -> PyResult<Text<'a>> {
    if let Ok(text) = s.to_str() {
        return Ok(Text { text: Cow::Borrowed(text), escaped: false });
    }
    let encoded = s.call_method1("encode", ("utf-8", "surrogatepass"))?;
    let bytes = encoded.downcast::<PyBytes>()?.as_bytes();
    Ok(Text { text: Cow::Owned(escape_surrogates(bytes)), escaped: true })
}

fn escape_surrogates(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 8);
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0xED && i + 2 < bytes.len() && bytes[i + 1] >= 0xA0 {
            out.push_str(std::str::from_utf8(&bytes[start..i]).expect("valid UTF-8"));
            let cp = (((bytes[i] & 0x0F) as u32) << 12)
                | (((bytes[i + 1] & 0x3F) as u32) << 6)
                | ((bytes[i + 2] & 0x3F) as u32);
            out.push(char::from_u32(SURROGATE_ESCAPE_BASE + cp - 0xD800).unwrap());
            i += 3;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push_str(std::str::from_utf8(&bytes[start..]).expect("valid UTF-8"));
    out
}

fn to_python<'py>(py: Python<'py>, text: &str, escaped: bool) -> PyResult<Bound<'py, PyAny>> {
    if !escaped || !text.chars().any(|ch| ch as u32 >= SURROGATE_ESCAPE_BASE) {
        return Ok(PyString::new(py, text).into_any());
    }
    let mut bytes = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let cp = ch as u32;
        if cp >= SURROGATE_ESCAPE_BASE {
            let surrogate = cp - SURROGATE_ESCAPE_BASE + 0xD800;
            bytes.extend([
                0xED,
                0x80 | ((surrogate >> 6) & 0x3F) as u8,
                0x80 | (surrogate & 0x3F) as u8,
            ]);
        } else {
            let mut buf = [0; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
    }
    PyBytes::new(py, &bytes).call_method1("decode", ("utf-8", "surrogatepass"))
}

/// Read a Python TextFixerConfig (a namedtuple) into a Rust config.
fn parse_config(config: Option<&Bound<'_, PyAny>>, explain: bool) -> PyResult<TextFixerConfig> {
    let Some(config) = config else {
        return Ok(TextFixerConfig { explain, ..Default::default() });
    };
    let field = |i: usize| config.get_item(i);
    let flag = |i: usize| field(i)?.is_truthy();

    let unescape = field(0)?;
    let unescape_html = if unescape.downcast::<PyString>().is_ok_and(|s| s.to_str().ok() == Some("auto")) {
        UnescapeHtml::Auto
    } else if unescape.is_truthy()? {
        UnescapeHtml::Always
    } else {
        UnescapeHtml::Never
    };
    let normalization = field(13)?;
    let normalization = if normalization.is_none() {
        None
    } else {
        let name: String = normalization.extract()?;
        Some(
            Normalization::from_name(&name)
                .ok_or_else(|| PyValueError::new_err("invalid normalization form"))?,
        )
    };
    Ok(TextFixerConfig {
        unescape_html,
        remove_terminal_escapes: flag(1)?,
        fix_encoding: flag(2)?,
        restore_byte_a0: flag(3)?,
        replace_lossy_sequences: flag(4)?,
        decode_inconsistent_utf8: flag(5)?,
        fix_c1_controls: flag(6)?,
        fix_latin_ligatures: flag(7)?,
        fix_character_width: flag(8)?,
        uncurl_quotes: flag(9)?,
        fix_line_breaks: flag(10)?,
        fix_surrogates: flag(11)?,
        remove_control_chars: flag(12)?,
        normalization,
        max_decode_length: field(14)?.extract()?,
        explain: flag(15)?,
        surrogates_escaped: false,
    })
}

/// Run `f` on the text, releasing the GIL if the text is long.
fn run<T: Send>(py: Python<'_>, text: &str, f: impl FnOnce(&str) -> T + Send) -> T {
    if text.len() > RELEASE_GIL_LEN {
        py.allow_threads(|| f(text))
    } else {
        f(text)
    }
}

type Steps = Vec<(&'static str, &'static str)>;

fn steps_to_tuples(steps: Vec<ExplanationStep>) -> Steps {
    steps.into_iter().map(|s| (s.action, s.parameter)).collect()
}

#[pyfunction]
#[pyo3(signature = (text, config=None))]
fn fix_text<'py>(
    py: Python<'py>,
    text: &Bound<'py, PyString>,
    config: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let t = extract_text(text)?;
    let mut config = parse_config(config, false)?;
    config.surrogates_escaped = t.escaped;
    let fixed = run(py, &t.text, |s| ftfy_rs::fix_text(s, &config));
    to_python(py, &fixed, t.escaped && !config.fix_surrogates)
}

#[pyfunction]
#[pyo3(signature = (text, config=None))]
fn fix_text_segment<'py>(
    py: Python<'py>,
    text: &Bound<'py, PyString>,
    config: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let t = extract_text(text)?;
    let mut config = parse_config(config, false)?;
    config.surrogates_escaped = t.escaped;
    let fixed = run(py, &t.text, |s| ftfy_rs::fix_text_segment(s, &config));
    to_python(py, &fixed, t.escaped && !config.fix_surrogates)
}

#[pyfunction]
#[pyo3(signature = (text, config=None))]
fn fix_and_explain<'py>(
    py: Python<'py>,
    text: &Bound<'py, PyString>,
    config: Option<&Bound<'py, PyAny>>,
) -> PyResult<(Bound<'py, PyAny>, Option<Steps>)> {
    let t = extract_text(text)?;
    let mut config = parse_config(config, true)?;
    config.surrogates_escaped = t.escaped;
    let (fixed, steps) = run(py, &t.text, |s| ftfy_rs::fix_and_explain(s, &config));
    let restore = t.escaped && !config.fix_surrogates;
    Ok((to_python(py, &fixed, restore)?, steps.map(steps_to_tuples)))
}

#[pyfunction]
#[pyo3(signature = (text, config=None))]
fn fix_encoding_and_explain<'py>(
    py: Python<'py>,
    text: &Bound<'py, PyString>,
    config: Option<&Bound<'py, PyAny>>,
) -> PyResult<(Bound<'py, PyAny>, Steps)> {
    let t = extract_text(text)?;
    let config = parse_config(config, true)?;
    let (fixed, steps) = run(py, &t.text, |s| ftfy_rs::fix_encoding_and_explain(s, &config));
    Ok((to_python(py, &fixed, t.escaped)?, steps_to_tuples(steps)))
}

#[pyfunction]
#[pyo3(signature = (text, config=None))]
fn fix_encoding<'py>(
    py: Python<'py>,
    text: &Bound<'py, PyString>,
    config: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let t = extract_text(text)?;
    let config = parse_config(config, false)?;
    let fixed = run(py, &t.text, |s| ftfy_rs::fix_encoding(s, &config));
    to_python(py, &fixed, t.escaped)
}

/// Apply a str -> str fixer, returning the original object if it's unchanged.
fn apply_fixer<'py>(
    py: Python<'py>,
    text: &Bound<'py, PyString>,
    fixer: fn(&str) -> Cow<'_, str>,
) -> PyResult<Bound<'py, PyAny>> {
    let t = extract_text(text)?;
    match fixer(&t.text) {
        Cow::Borrowed(_) => Ok(text.clone().into_any()),
        Cow::Owned(fixed) => to_python(py, &fixed, t.escaped),
    }
}

macro_rules! text_fixers {
    ($($name:ident),* $(,)?) => {
        $(
            #[pyfunction]
            fn $name<'py>(py: Python<'py>, text: &Bound<'py, PyString>) -> PyResult<Bound<'py, PyAny>> {
                apply_fixer(py, text, fixes::$name)
            }
        )*
    };
}

text_fixers!(
    unescape_html,
    remove_terminal_escapes,
    uncurl_quotes,
    fix_latin_ligatures,
    fix_character_width,
    fix_line_breaks,
    remove_control_chars,
    decode_inconsistent_utf8,
    fix_c1_controls,
);

#[pyfunction]
fn fix_surrogates<'py>(py: Python<'py>, text: &Bound<'py, PyString>) -> PyResult<Bound<'py, PyAny>> {
    // Without escaped surrogates in the input there's nothing to fix, and
    // afterwards none are left, so characters in the escape range are real.
    let t = extract_text(text)?;
    match fixes::fix_escaped_surrogates(&t.text) {
        Cow::Borrowed(_) => Ok(text.clone().into_any()),
        Cow::Owned(fixed) => to_python(py, &fixed, false),
    }
}

#[pyfunction]
fn restore_byte_a0<'py>(py: Python<'py>, data: &[u8]) -> Bound<'py, PyBytes> {
    PyBytes::new(py, &fixes::restore_byte_a0(data))
}

#[pyfunction]
fn replace_lossy_sequences<'py>(py: Python<'py>, data: &[u8]) -> Bound<'py, PyBytes> {
    PyBytes::new(py, &fixes::replace_lossy_sequences(data))
}

#[pyfunction]
fn badness(text: &Bound<'_, PyString>) -> PyResult<usize> {
    Ok(ftfy_rs::badness(&extract_text(text)?.text))
}

#[pyfunction]
fn is_bad(text: &Bound<'_, PyString>) -> PyResult<bool> {
    Ok(ftfy_rs::is_bad(&extract_text(text)?.text))
}

#[pyfunction]
fn possible_encoding(text: &Bound<'_, PyString>, encoding: &str) -> PyResult<bool> {
    ftfy_rs::possible_encoding(&extract_text(text)?.text, encoding)
        .ok_or_else(|| PyKeyError::new_err(encoding.to_string()))
}

/// Characters whose normalization an older Unicode version could disagree
/// with, if it doesn't have them.
#[pyfunction]
fn _normalization_candidates() -> Vec<char> {
    ftfy_rs::normalize::candidates()
}

/// Tell normalization which characters the running Python's `unicodedata`
/// doesn't know, so it can leave them alone as Python does.
#[pyfunction]
fn _set_unassigned(chars: Vec<char>) -> bool {
    ftfy_rs::normalize::set_unassigned(chars)
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(fix_text, m)?)?;
    m.add_function(wrap_pyfunction!(fix_text_segment, m)?)?;
    m.add_function(wrap_pyfunction!(fix_and_explain, m)?)?;
    m.add_function(wrap_pyfunction!(fix_encoding_and_explain, m)?)?;
    m.add_function(wrap_pyfunction!(fix_encoding, m)?)?;
    m.add_function(wrap_pyfunction!(unescape_html, m)?)?;
    m.add_function(wrap_pyfunction!(remove_terminal_escapes, m)?)?;
    m.add_function(wrap_pyfunction!(uncurl_quotes, m)?)?;
    m.add_function(wrap_pyfunction!(fix_latin_ligatures, m)?)?;
    m.add_function(wrap_pyfunction!(fix_character_width, m)?)?;
    m.add_function(wrap_pyfunction!(fix_line_breaks, m)?)?;
    m.add_function(wrap_pyfunction!(fix_surrogates, m)?)?;
    m.add_function(wrap_pyfunction!(remove_control_chars, m)?)?;
    m.add_function(wrap_pyfunction!(decode_inconsistent_utf8, m)?)?;
    m.add_function(wrap_pyfunction!(fix_c1_controls, m)?)?;
    m.add_function(wrap_pyfunction!(restore_byte_a0, m)?)?;
    m.add_function(wrap_pyfunction!(replace_lossy_sequences, m)?)?;
    m.add_function(wrap_pyfunction!(badness, m)?)?;
    m.add_function(wrap_pyfunction!(is_bad, m)?)?;
    m.add_function(wrap_pyfunction!(possible_encoding, m)?)?;
    m.add_function(wrap_pyfunction!(_normalization_candidates, m)?)?;
    m.add_function(wrap_pyfunction!(_set_unassigned, m)?)?;
    Ok(())
}
