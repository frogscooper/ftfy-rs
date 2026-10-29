"""
Differential tests: ftfy_rs must give exactly the same results as Python ftfy.

Inputs are generated from a fixed seed: real-world-ish text run through every
kind of damage ftfy knows how to fix (mojibake in each supported codec,
double mojibake, lost bytes, HTML entities, surrogates, line breaks...), plus
random strings drawn from the characters that ftfy's heuristics care about.
Set FTFY_PARITY_CASES to change how many random cases run.
"""

from __future__ import annotations

import json
import os
import random
from pathlib import Path

import ftfy
import ftfy.badness
import ftfy.chardata
import ftfy.fixes
import pytest

import ftfy_rs
import ftfy_rs.badness
import ftfy_rs.chardata
import ftfy_rs.fixes

SEED = 20261029
N_CASES = int(os.environ.get("FTFY_PARITY_CASES", "20000"))
TEST_CASE_DIR = Path(__file__).parent / "upstream" / "test-cases"

CLEAN_SAMPLES = [
    "Renée Fleming sang “Ave Maria” — à la française.",
    "Noël Coward’s café served crème brûlée for 5 €.",
    "¿Qué pasa? ¡Olé! Señor Muñoz está aquí.",
    "Zażółć gęślą jaźń. Příliš žluťoučký kůň úpěl ďábelské ódy.",
    "Съешь же ещё этих мягких французских булок, да выпей чаю.",
    "Ξεσκεπάζω την ψυχοφθόρα βδελυγμία.",
    "Pijamalı hasta yağız şoföre çabucak güvendi.",
    "Įlinkdama fechtuotojo špaga sublykčiojusi pragręžė apvalų arbūzą.",
    "日本語のテキスト、カタカナとひらがな。",
    "한국어 텍스트입니다.",
    "Emoji time 😀🎉👍🏽 and flags 🇺🇸🏴󠁧󠁢󠁷󠁬󠁳󠁿",
    "Math: x² + y³ ≤ ½ × π ≈ 1.57 ± 0.01 µm",
    "Prices: £12, ¥300, ₹45, ¢99 — that’s “all” folks…",
    "Ελληνικά και English μαζί • bullet • points",
    "Arabic: مرحبا بالعالم and Hebrew: שלום עולם",
    "Devanagari: नमस्ते दुनिया, Tamil: வணக்கம்",
    "Ænema Ørsted Åland Þórr ðæt ß",
    "ﬁne ﬂour ﬀ ﬃ ĳ ǆ",
    "ＦＵＬＬＷＩＤＴＨ　ｔｅｘｔ ｶﾀｶﾅ",
    "à la carte, às vezes, àquele, voilà le travail",
    " non-breaking spaces everywhere ",
    "Ça va? Où est l’hôtel? Je ne sais pas…",
]

DAMAGE_ENCODINGS = [
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
    "windows-1252",
]

EXTRAS = [
    "&amp;", "&lt;", "&EACUTE;", "&#x2019;", "&#133;", "&#59;", "&#xffff;", "&nbsp",
    "&hellip;", "&#20x6;", "<b>", "\r\n", "\r", " ", "\x85", "\x1b[36;44m",
    "\x00", "\x7f", "﻿", "￼", "\ud83d", "\udca9", "💩", "\x81",
    "\x9d", "\x1a", "?", "�", "ﬁ", "ｗ", "　", "‘", "“", "ʼ", " ", "\xa0",
    "́", "é", "\U00010000", "Ã", "Â", "â€", "Ã©", "Ã ",
]


def mojibake(text: str, encoding: str, rng: random.Random) -> str:
    data = text.encode("utf-8")
    if rng.random() < 0.2:
        data = data.replace(b"\xa0", b" ")
    errors = "replace" if encoding == "windows-1252" else "strict"
    return data.decode(encoding, errors=errors)


def damaged_variants(text: str, rng: random.Random) -> list[str]:
    out = [text]
    for enc in DAMAGE_ENCODINGS:
        try:
            once = mojibake(text, enc, rng)
        except UnicodeDecodeError:
            continue
        out.append(once)
        try:
            out.append(mojibake(once, rng.choice(DAMAGE_ENCODINGS), rng))
        except UnicodeDecodeError:
            pass
    return out


def build_alphabet() -> list[str]:
    chars = set()
    for enc in DAMAGE_ENCODINGS[:-1]:
        chars.update(bytes(range(0x80, 0x100)).decode(enc))
    chars.update("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789")
    chars.update(" .,;:!?'\"-_()[]&#<>/\\\n\r\t\x0c")
    chars.update("�ﬁﬂ  　ｗｶ😀ひ한")
    return sorted(chars)


def generate_cases() -> list[str]:
    rng = random.Random(SEED)
    cases = []

    for sample in CLEAN_SAMPLES:
        cases.extend(damaged_variants(sample, rng))

    for path in sorted(TEST_CASE_DIR.glob("*.json")):
        for case in json.loads(path.read_text(encoding="utf-8")):
            cases.append(case["original"])
            cases.extend(damaged_variants(case["fixed"], rng))

    alphabet = build_alphabet()
    base = list(cases)
    while len(cases) < N_CASES:
        r = rng.random()
        if r < 0.4:
            n = rng.randint(1, 12)
            cases.append("".join(rng.choice(alphabet) for _ in range(n)))
        elif r < 0.7:
            # Splice extras into an existing case at random positions
            text = rng.choice(base)
            for _ in range(rng.randint(1, 3)):
                pos = rng.randint(0, len(text))
                text = text[:pos] + rng.choice(EXTRAS) + text[pos:]
            cases.append(text)
        elif r < 0.85:
            # Truncate or join mojibake, to make inconsistent encodings
            a, b = rng.choice(base), rng.choice(base)
            cases.append(a[: rng.randint(0, len(a))] + rng.choice([" ", "\n", ""]) + b)
        else:
            # Lose some characters, as a lossy codec would
            text = rng.choice(base)
            text = "".join(
                rng.choice(["�", "?", "\x1a"]) if ch > "\x7f" and rng.random() < 0.15 else ch
                for ch in text
            )
            cases.append(text)
    return cases


CASES = generate_cases()

CONFIG_FIELDS = [
    "remove_terminal_escapes",
    "fix_encoding",
    "restore_byte_a0",
    "replace_lossy_sequences",
    "decode_inconsistent_utf8",
    "fix_c1_controls",
    "fix_latin_ligatures",
    "fix_character_width",
    "uncurl_quotes",
    "fix_line_breaks",
    "fix_surrogates",
    "remove_control_chars",
    "explain",
]


def random_config_kwargs(rng: random.Random) -> dict:
    kwargs = {f: rng.random() < 0.8 for f in CONFIG_FIELDS}
    kwargs["unescape_html"] = rng.choice(["auto", True, False])
    kwargs["normalization"] = rng.choice(["NFC", "NFC", "NFD", "NFKC", "NFKD", None])
    kwargs["max_decode_length"] = rng.choice([1_000_000, 1_000_000, 5, 17])
    return kwargs


def chunks(seq, n):
    size = (len(seq) + n - 1) // n
    return [seq[i : i + size] for i in range(0, len(seq), size)]


@pytest.mark.parametrize("chunk", chunks(CASES, 20), ids=lambda c: f"{len(c)}cases")
def test_default_parity(chunk):
    for text in chunk:
        assert ftfy_rs.fix_text(text) == ftfy.fix_text(text), repr(text)
        assert ftfy_rs.fix_and_explain(text) == ftfy.fix_and_explain(text), repr(text)
        assert ftfy_rs.fix_encoding_and_explain(text) == ftfy.fix_encoding_and_explain(
            text
        ), repr(text)
        assert ftfy_rs.fix_encoding(text) == ftfy.fix_encoding(text), repr(text)


@pytest.mark.parametrize("chunk", chunks(CASES, 20), ids=lambda c: f"{len(c)}cases")
def test_config_parity(chunk):
    rng = random.Random(len(chunk[0]) + SEED)
    for text in chunk:
        kwargs = random_config_kwargs(rng)
        expected = ftfy.fix_text(text, ftfy.TextFixerConfig(**kwargs))
        assert ftfy_rs.fix_text(text, ftfy_rs.TextFixerConfig(**kwargs)) == expected, (
            repr(text),
            kwargs,
        )
        assert ftfy_rs.fix_and_explain(text, **kwargs) == ftfy.fix_and_explain(
            text, **kwargs
        ), (repr(text), kwargs)


@pytest.mark.parametrize("chunk", chunks(CASES, 10), ids=lambda c: f"{len(c)}cases")
def test_heuristic_and_fixer_parity(chunk):
    for text in chunk:
        assert ftfy_rs.badness.badness(text) == ftfy.badness.badness(text), repr(text)
        assert ftfy_rs.badness.is_bad(text) == ftfy.badness.is_bad(text), repr(text)
        for enc in ["ascii", *ftfy.chardata.CHARMAP_ENCODINGS]:
            assert ftfy_rs.chardata.possible_encoding(
                text, enc
            ) == ftfy.chardata.possible_encoding(text, enc), (repr(text), enc)
        for name in ftfy.FIXERS:
            if name in ("restore_byte_a0", "replace_lossy_sequences"):
                for enc in ("latin-1", "sloppy-windows-1252"):
                    try:
                        data = text.encode(enc)
                    except UnicodeEncodeError:
                        continue
                    assert ftfy_rs.FIXERS[name](data) == ftfy.FIXERS[name](data), (
                        name,
                        data,
                    )
            else:
                assert ftfy_rs.FIXERS[name](text) == ftfy.FIXERS[name](text), (
                    name,
                    repr(text),
                )


def test_explanations_replay():
    for text in CASES[:3000]:
        fixed, plan = ftfy_rs.fix_and_explain(text)
        if "normalize" not in [step[0] for step in plan]:
            assert ftfy_rs.apply_plan(text, plan) == fixed, repr(text)


def test_long_text_parity():
    rng = random.Random(SEED)
    text = "\n".join(rng.sample(CASES, 2000))
    assert ftfy_rs.fix_text(text) == ftfy.fix_text(text)
    joined = " ".join(rng.sample(CASES, 500))
    assert ftfy_rs.fix_text(joined, max_decode_length=300) == ftfy.fix_text(
        joined, max_decode_length=300
    )
