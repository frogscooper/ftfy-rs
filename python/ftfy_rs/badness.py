"""
The heuristic that detects likely mojibake, as in `ftfy.badness`.
"""

import warnings

from ftfy_rs import _native

badness = _native.badness
is_bad = _native.is_bad


def sequence_weirdness(text: str) -> int:
    """
    The name of the heuristic used in ftfy 2.x through 5.x, redirected to
    :func:`badness`.
    """
    warnings.warn(
        "`sequence_weirdness()` is an old heuristic, and the current "
        "closest equivalent is `ftfy.badness.badness()`"
    )
    return badness(text)
