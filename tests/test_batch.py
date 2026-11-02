"""
fix_texts, the parallel batch API, must give the same results as calling
ftfy's fix_text on each text.
"""

import random

import ftfy
import pytest

import ftfy_rs
from test_parity import CASES, SEED, random_config_kwargs


def test_matches_ftfy_on_the_corpus():
    # Large enough to take the parallel path.
    assert ftfy_rs.fix_texts(CASES) == [ftfy.fix_text(text) for text in CASES]


def test_matches_ftfy_with_configs():
    rng = random.Random(SEED)
    for _ in range(20):
        kwargs = random_config_kwargs(rng)
        batch = rng.sample(CASES, 500)
        expected = [ftfy.fix_text(text, **kwargs) for text in batch]
        assert ftfy_rs.fix_texts(batch, **kwargs) == expected, kwargs
        config = ftfy_rs.TextFixerConfig(**kwargs)
        assert ftfy_rs.fix_texts(batch, config) == expected, kwargs


def test_small_and_empty_batches():
    assert ftfy_rs.fix_texts([]) == []
    assert ftfy_rs.fix_texts(["schÃ¶n"]) == ["schön"]
    assert ftfy_rs.fix_texts(iter(["&amp;", "x"]), unescape_html=False) == ["&amp;", "x"]


def test_texts_are_independent():
    # In fix_text, a '<' turns off HTML unescaping for the rest of that text,
    # but it must not leak into the other texts of a batch.
    texts = ["<b>&amp;</b>", "&amp;", "a\n<b>\n&amp;", "&amp;"] * 10_000
    assert ftfy_rs.fix_texts(texts) == [ftfy.fix_text(text) for text in texts]


def test_unchanged_texts_are_returned_as_is():
    text = "already fine"
    assert ftfy_rs.fix_texts([text])[0] is text


def test_surrogates():
    texts = ["💩", "lone \ud83d", "\udca9 x", "fine"] * 5000
    for kwargs in [{}, {"fix_surrogates": False}]:
        expected = [ftfy.fix_text(text, **kwargs) for text in texts]
        assert ftfy_rs.fix_texts(texts, **kwargs) == expected


def test_bad_items():
    with pytest.raises(UnicodeError, match="index 1"):
        ftfy_rs.fix_texts(["ok", b"bytes"])
    with pytest.raises(TypeError, match="NoneType at index 2"):
        ftfy_rs.fix_texts(["ok", "fine", None])
    with pytest.raises(TypeError):
        ftfy_rs.fix_texts(42)
