//! The heuristic that detects likely mojibake.
//!
//! The regex itself is generated from Python ftfy's `BADNESS_RE`; see
//! `scripts/gen_tables.py`.

use std::sync::LazyLock;

use regex::{Regex, RegexBuilder};

use crate::tables::BADNESS_PATTERN;

pub(crate) fn build(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .size_limit(1 << 26)
        .dfa_size_limit(1 << 26)
        .build()
        .expect("generated regex should compile")
}

static BADNESS_RE: LazyLock<Regex> = LazyLock::new(|| build(BADNESS_PATTERN));

/// Count the number of unlikely character sequences in the text. A badness
/// greater than 0 indicates that some of it seems to be mojibake.
pub fn badness(text: &str) -> usize {
    BADNESS_RE.find_iter(text).count()
}

/// Returns true if the text looks like it contains mojibake.
pub fn is_bad(text: &str) -> bool {
    BADNESS_RE.is_match(text)
}
