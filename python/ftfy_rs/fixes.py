"""
The individual fixes that :func:`ftfy_rs.fix_text` can perform, with the same
names and behavior as the functions in `ftfy.fixes`.
"""

from __future__ import annotations

import codecs
import re
import warnings
from re import Match
from typing import Any

from ftfy_rs import _native

unescape_html = _native.unescape_html
remove_terminal_escapes = _native.remove_terminal_escapes
uncurl_quotes = _native.uncurl_quotes
fix_latin_ligatures = _native.fix_latin_ligatures
fix_character_width = _native.fix_character_width
fix_line_breaks = _native.fix_line_breaks
fix_surrogates = _native.fix_surrogates
remove_control_chars = _native.remove_control_chars
restore_byte_a0 = _native.restore_byte_a0
replace_lossy_sequences = _native.replace_lossy_sequences
decode_inconsistent_utf8 = _native.decode_inconsistent_utf8
fix_c1_controls = _native.fix_c1_controls


def remove_bom(text: str) -> str:
    r"""
    Remove a byte-order mark that was accidentally decoded as if it were part
    of the text.

    >>> print(remove_bom(chr(0xfeff) + "Where do you want to go today?"))
    Where do you want to go today?
    """
    return text.lstrip(chr(0xFEFF))


# Define a regex to match valid escape sequences in Python string literals.
ESCAPE_SEQUENCE_RE = re.compile(
    r"""
    ( \\U........      # 8-digit hex escapes
    | \\u....          # 4-digit hex escapes
    | \\x..            # 2-digit hex escapes
    | \\[0-7]{1,3}     # Octal escapes
    | \\N\{[^}]+\}     # Unicode characters by name
    | \\[\\'"abfnrtv]  # Single-character escapes
    )""",
    re.UNICODE | re.VERBOSE,
)


def decode_escapes(text: str) -> str:
    r"""
    Decode backslashed escape sequences, including \\x, \\u, and \\U character
    references, even in the presence of other Unicode.

        >>> factoid = '\\u20a1 is the currency symbol for the colón.'
        >>> print(decode_escapes(factoid))
        ₡ is the currency symbol for the colón.
    """

    def decode_match(match: Match[str]) -> str:
        "Given a regex match, decode the escape sequence it contains."
        return codecs.decode(match.group(0), "unicode-escape")

    return ESCAPE_SEQUENCE_RE.sub(decode_match, text)


def fix_encoding_and_explain(text: str) -> Any:
    """
    Deprecated copy of `ftfy_rs.fix_encoding_and_explain()`.
    """
    warnings.warn(
        "`fix_encoding_and_explain()` has moved to the main module of ftfy.",
        DeprecationWarning,
        stacklevel=2,
    )
    import ftfy_rs

    return ftfy_rs.fix_encoding_and_explain(text)


def fix_encoding(text: str) -> str:
    """
    Deprecated copy of `ftfy_rs.fix_encoding()`.
    """
    warnings.warn(
        "`fix_encoding()` has moved to the main module of ftfy.",
        DeprecationWarning,
        stacklevel=2,
    )
    import ftfy_rs

    return ftfy_rs.fix_encoding(text)


def apply_plan(text: str, plan: list[tuple[str, str]]) -> str:
    """
    Deprecated copy of `ftfy_rs.apply_plan()`.
    """
    warnings.warn(
        "`apply_plan()` has moved to the main module of ftfy.",
        DeprecationWarning,
        stacklevel=2,
    )
    import ftfy_rs

    return ftfy_rs.apply_plan(text, plan)
