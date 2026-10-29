"""
Details about characters and the encodings that use them, as in
`ftfy.chardata`. Only the commonly used parts are provided.
"""

from ftfy_rs import _native

# These are the encodings we will try to fix in ftfy, in the
# order that they should be tried.
CHARMAP_ENCODINGS = [
    "latin-1",
    "sloppy-windows-1252",
    "sloppy-windows-1251",
    "sloppy-windows-1250",
    "sloppy-windows-1253",
    "sloppy-windows-1254",
    "sloppy-windows-1257",
    "iso-8859-2",
    "macroman",
    "cp437",
]


def possible_encoding(text: str, encoding: str) -> bool:
    """
    Given text and a single-byte encoding, check whether that text could have
    been decoded from that single-byte encoding.

    In other words, check whether it can be encoded in that encoding, possibly
    sloppily.
    """
    return _native.possible_encoding(text, encoding)
