"""
ftfy_rs: a fast, drop-in replacement for ftfy, implemented in Rust.

    >>> import ftfy_rs as ftfy
    >>> ftfy.fix_text('âœ” No problems')
    '✔ No problems'

The API matches ftfy 6.3: the same functions, the same TextFixerConfig, and
the same results, including explanations.
"""

from __future__ import annotations

import unicodedata
import warnings
from typing import TYPE_CHECKING, Any, BinaryIO, Callable, Literal, NamedTuple, TextIO

from ftfy_rs import _native, bad_codecs, chardata, fixes
from ftfy_rs.badness import is_bad
from ftfy_rs.formatting import display_ljust

if TYPE_CHECKING:
    from collections.abc import Iterator

__version__ = "0.1.0"

# The version of Python ftfy whose behavior this matches.
__ftfy_version__ = "6.3.1"

bad_codecs.ok()


class ExplanationStep(NamedTuple):
    """
    A step in an ExplainedText, explaining how to decode text.

    The possible actions are:

    - "encode": take in a string and encode it as bytes, with the given encoding
    - "decode": take in bytes and decode them as a string, with the given encoding
    - "transcode": convert bytes to bytes with a particular named function
    - "apply": convert str to str with a particular named function

    The `parameter` is the name of the encoding or function to use. If it's a
    function, it must appear in the FIXERS dictionary.
    """

    action: str
    parameter: str

    def __repr__(self) -> str:
        return repr(tuple(self))


class ExplainedText(NamedTuple):
    """
    The return type from functions that provide an "explanation" of which
    steps they applied to fix the text, such as :func:`fix_and_explain()`.

    When the 'explain' option is disabled, the `explanation` will be None.
    """

    text: str
    explanation: list[ExplanationStep] | None


# Functions that can be applied using `apply_plan`.
FIXERS: dict[str, Callable] = {  # type: ignore[type-arg]
    "unescape_html": fixes.unescape_html,
    "remove_terminal_escapes": fixes.remove_terminal_escapes,
    "restore_byte_a0": fixes.restore_byte_a0,
    "replace_lossy_sequences": fixes.replace_lossy_sequences,
    "decode_inconsistent_utf8": fixes.decode_inconsistent_utf8,
    "fix_c1_controls": fixes.fix_c1_controls,
    "fix_latin_ligatures": fixes.fix_latin_ligatures,
    "fix_character_width": fixes.fix_character_width,
    "uncurl_quotes": fixes.uncurl_quotes,
    "fix_line_breaks": fixes.fix_line_breaks,
    "fix_surrogates": fixes.fix_surrogates,
    "remove_control_chars": fixes.remove_control_chars,
}


class TextFixerConfig(NamedTuple):
    r"""
    A TextFixerConfig object stores configuration options for ftfy.

    It's a namedtuple with defaults, so you can instantiate it by providing
    the values to change from their defaults as keyword arguments. The
    options are the same as in ftfy; see its documentation for details.
    """

    unescape_html: str | bool = "auto"
    remove_terminal_escapes: bool = True
    fix_encoding: bool = True
    restore_byte_a0: bool = True
    replace_lossy_sequences: bool = True
    decode_inconsistent_utf8: bool = True
    fix_c1_controls: bool = True
    fix_latin_ligatures: bool = True
    fix_character_width: bool = True
    uncurl_quotes: bool = True
    fix_line_breaks: bool = True
    fix_surrogates: bool = True
    remove_control_chars: bool = True
    normalization: Literal["NFC", "NFD", "NFKC", "NFKD"] | None = "NFC"
    max_decode_length: int = 1000000
    explain: bool = True


def _config_from_kwargs(
    config: TextFixerConfig | None, kwargs: dict[str, Any], explain: bool
) -> TextFixerConfig | None:
    """
    Combine a config and keyword arguments. Returns None when both are
    absent, which tells the native code to use the defaults.
    """
    if not kwargs:
        return config
    if config is None:
        config = TextFixerConfig(explain=explain)
    if "fix_entities" in kwargs:
        warnings.warn(
            "`fix_entities` has been renamed to `unescape_html`",
            DeprecationWarning,
            stacklevel=3,
        )
        kwargs = kwargs.copy()
        kwargs["unescape_html"] = kwargs["fix_entities"]
        del kwargs["fix_entities"]
    return config._replace(**kwargs)


BYTES_ERROR_TEXT = """Hey wait, this isn't Unicode.

ftfy is designed to fix problems with text. Treating bytes like they're
interchangeable with Unicode text is usually something that introduces
problems with text.

You should first decode these bytes from the encoding you think they're in.
If you're not sure what encoding they're in:

- First, try to find out. 'utf-8' is a good assumption.
- If the encoding is simply unknowable, try running your bytes through
  ftfy.guess_bytes. As the name implies, this may not always be accurate.

For more information on the distinction between bytes and text, read the
Python Unicode HOWTO:

    http://docs.python.org/3/howto/unicode.html
"""


def _check_text(text: Any) -> None:
    if isinstance(text, bytes):
        raise UnicodeError(BYTES_ERROR_TEXT)


def _explained(text: str, steps: list[tuple[str, str]] | None) -> ExplainedText:
    if steps is None:
        return ExplainedText(text, None)
    return ExplainedText(text, [ExplanationStep(*step) for step in steps])


def fix_text(text: str, config: TextFixerConfig | None = None, **kwargs: Any) -> str:
    r"""
    Given Unicode text as input, fix inconsistencies and glitches in it,
    such as mojibake (text that was decoded in the wrong encoding).

        >>> fix_text('âœ” No problems')
        '✔ No problems'

        >>> print(fix_text("&macr;\\_(ã\x83\x84)_/&macr;"))
        ¯\_(ツ)_/¯

        >>> fix_text('Broken text&hellip; it&#x2019;s ﬂubberiﬁc!')
        "Broken text... it's flubberific!"

        >>> fix_text('ＬＯＵＤ　ＮＯＩＳＥＳ')
        'LOUD NOISES'

    This function fixes text in independent segments, which are usually lines
    of text, or arbitrarily broken up every 1 million codepoints (configurable
    with `config.max_decode_length`) if there aren't enough line breaks.
    """
    config = _config_from_kwargs(config, kwargs, explain=False)
    _check_text(text)
    return _native.fix_text(text, config)


def fix_and_explain(
    text: str, config: TextFixerConfig | None = None, **kwargs: Any
) -> ExplainedText:
    """
    Fix text as a single segment, returning the fixed text and an explanation
    of what was fixed.

    The explanation is a list of steps that can be applied with
    :func:`apply_plan`, or if config.explain is False, it will be None.
    """
    _check_text(text)
    config = _config_from_kwargs(config, kwargs, explain=True)
    return _explained(*_native.fix_and_explain(text, config))


def fix_encoding_and_explain(
    text: str, config: TextFixerConfig | None = None, **kwargs: Any
) -> ExplainedText:
    """
    Apply the steps of ftfy that detect mojibake and fix it. Returns the fixed
    text and a list explaining what was fixed.

        >>> fix_encoding_and_explain("sÃ³")
        ExplainedText(text='só', explanation=[('encode', 'latin-1'), ('decode', 'utf-8')])
    """
    _check_text(text)
    config = _config_from_kwargs(config, kwargs, explain=True)
    return _explained(*_native.fix_encoding_and_explain(text, config))


def fix_encoding(text: str, config: TextFixerConfig | None = None, **kwargs: Any) -> str:
    """
    Apply just the encoding-fixing steps of ftfy to this text. Returns the
    fixed text, discarding the explanation.

        >>> fix_encoding("Ã³")
        'ó'
        >>> fix_encoding("&ATILDE;&SUP3;")
        '&ATILDE;&SUP3;'
    """
    _check_text(text)
    config = _config_from_kwargs(config, kwargs, explain=False)
    return _native.fix_encoding(text, config)


# Some alternate names for the main functions
ftfy = fix_text


def fix_text_segment(text: str, config: TextFixerConfig | None = None, **kwargs: Any) -> str:
    """
    Fix text as a single segment, with a consistent sequence of steps that
    are applied to fix the text. Discard the explanation.
    """
    _check_text(text)
    config = _config_from_kwargs(config, kwargs, explain=False)
    return _native.fix_text_segment(text, config)


def fix_file(
    input_file: TextIO | BinaryIO,
    encoding: str | None = None,
    config: TextFixerConfig | None = None,
    **kwargs: Any,
) -> Iterator[str]:
    """
    Fix text that is found in a file.

    If the file is being read as Unicode text, use that. If it's being read as
    bytes, then we hope an encoding was supplied. If not, unfortunately, we
    have to guess what encoding it is. See the `guess_bytes` function for how
    this is done.

    The output is a stream of fixed lines of text.
    """
    if config is None:
        config = TextFixerConfig()
    config = _config_from_kwargs(config, kwargs, explain=True)
    assert config is not None

    for line in input_file:
        if isinstance(line, bytes):
            if encoding is None:
                line, encoding = guess_bytes(line)
            else:
                line = line.decode(encoding)
        if config.unescape_html == "auto" and "<" in line:
            config = config._replace(unescape_html=False)

        fixed_line, _explan = fix_and_explain(line, config)
        yield fixed_line


def guess_bytes(bstring: bytes) -> tuple[str, str]:
    """
    NOTE: Using `guess_bytes` is not the recommended way of using ftfy. ftfy
    is not designed to be an encoding detector.

    In the unfortunate situation that you have some bytes in an unknown
    encoding, ftfy can guess a reasonable strategy for decoding them, by trying
    UTF-16 with a byte order mark, UTF-8, "utf-8-variants", MacRoman, and
    "sloppy-windows-1252", in that order.
    """
    if isinstance(bstring, str):
        msg = (
            "This string was already decoded as Unicode. You should pass "
            "bytes to guess_bytes, not Unicode."
        )
        raise UnicodeError(msg)

    if bstring.startswith((b"\xfe\xff", b"\xff\xfe")):
        return bstring.decode("utf-16"), "utf-16"

    byteset = set(bstring)
    try:
        if 0xED in byteset or 0xC0 in byteset:
            return bstring.decode("utf-8-variants"), "utf-8-variants"
        else:
            return bstring.decode("utf-8"), "utf-8"
    except UnicodeDecodeError:
        pass

    if 0x0D in byteset and 0x0A not in byteset:
        # Files that contain CR and not LF are likely to be MacRoman.
        return bstring.decode("macroman"), "macroman"

    return bstring.decode("sloppy-windows-1252"), "sloppy-windows-1252"


def apply_plan(text: str, plan: list[tuple[str, str]]) -> str:
    """
    Apply a plan for fixing the encoding of text.

    The plan is a list of tuples of the form (operation, arg), where
    `operation` is 'encode', 'decode', 'transcode' or 'apply'.

        >>> mojibake = "schÃ¶n"
        >>> text, plan = fix_and_explain(mojibake)
        >>> apply_plan(mojibake, plan)
        'schön'
    """
    obj = text
    for operation, encoding in plan:
        if operation == "encode":
            obj = obj.encode(encoding)  # type: ignore
        elif operation == "decode":
            obj = obj.decode(encoding)  # type: ignore
        elif operation in ("transcode", "apply"):
            if encoding in FIXERS:
                obj = FIXERS[encoding](obj)
            else:
                msg = f"Unknown function to apply: {encoding}"
                raise ValueError(msg)
        else:
            msg = f"Unknown plan step: {operation}"
            raise ValueError(msg)

    return obj


def explain_unicode(text: str) -> None:
    """
    A utility method that's useful for debugging mysterious Unicode.

    It breaks down a string, showing you for each codepoint its number in
    hexadecimal, its glyph, its category in the Unicode standard, and its name
    in the Unicode standard.
    """
    for char in text:
        if char.isprintable():
            display = char
        else:
            display = char.encode("unicode-escape").decode("ascii")
        print(
            "U+{code:04X}  {display} [{category}] {name}".format(
                display=display_ljust(display, 7),
                code=ord(char),
                category=unicodedata.category(char),
                name=unicodedata.name(char, "<unknown>"),
            )
        )


__all__ = [
    "ExplainedText",
    "ExplanationStep",
    "FIXERS",
    "TextFixerConfig",
    "apply_plan",
    "explain_unicode",
    "fix_and_explain",
    "fix_encoding",
    "fix_encoding_and_explain",
    "fix_file",
    "fix_text",
    "fix_text_segment",
    "ftfy",
    "guess_bytes",
    "is_bad",
]
