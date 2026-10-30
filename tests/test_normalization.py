"""
Normalization must match this Python's `unicodedata`, even though the Rust
side may have newer Unicode data.
"""

import sys
import unicodedata

import pytest

import ftfy_rs
from ftfy_rs import _native

ONLY_NORMALIZE = dict(
    fix_encoding=False,
    unescape_html=False,
    remove_terminal_escapes=False,
    fix_c1_controls=False,
    fix_latin_ligatures=False,
    fix_character_width=False,
    uncurl_quotes=False,
    fix_line_breaks=False,
    fix_surrogates=False,
    remove_control_chars=False,
)
FORMS = ["NFC", "NFD", "NFKC", "NFKD"]


def normalize_rs(text, form):
    return ftfy_rs.fix_text_segment(text, normalization=form, **ONLY_NORMALIZE)


@pytest.mark.parametrize("form", FORMS)
def test_every_codepoint(form):
    # Spaces separate the characters, so each one normalizes on its own.
    chars = [
        chr(cp)
        for cp in range(sys.maxunicode + 1)
        if not 0xD800 <= cp <= 0xDFFF and cp >= 0x80 and chr(cp) != " "
    ]
    for start in range(0, len(chars), 4096):
        text = " ".join(chars[start : start + 4096])
        if normalize_rs(text, form) != unicodedata.normalize(form, text):
            for ch in chars[start : start + 4096]:
                assert normalize_rs(ch, form) == unicodedata.normalize(form, ch), (
                    f"U+{ord(ch):04X}"
                )


@pytest.mark.parametrize("form", FORMS)
def test_characters_this_python_lacks(form):
    unassigned = [
        ch for ch in _native._normalization_candidates() if unicodedata.category(ch) == "Cn"
    ]
    for ch in unassigned:
        for text in [
            "e" + ch + "́",
            "́" + ch,
            ch + "̈",
            "ạ" + ch + "̇",
            "ᄀ" + ch + "ᅡ",
        ]:
            assert normalize_rs(text, form) == unicodedata.normalize(form, text), (
                f"U+{ord(ch):04X}",
                ascii(text),
            )
