"""
Compare ftfy_rs against Python ftfy on a few workloads.

Usage:  python bench/bench.py
"""

from __future__ import annotations

import json
import platform
import sys
import time
from pathlib import Path

import ftfy

import ftfy_rs

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tests"))
from test_parity import CASES  # noqa: E402

TEST_CASES = []
for path in sorted((ROOT / "tests" / "upstream" / "test-cases").glob("*.json")):
    TEST_CASES += [c["original"] for c in json.loads(path.read_text(encoding="utf-8"))]

CLEAN_ASCII = [
    "Just setting up my twttr",
    "The quick brown fox jumps over the lazy dog, again and again.",
    "lol did you see that game last night? #sports",
    "Meeting moved to 3pm, see you all there.",
] * 250

CLEAN_UNICODE = [
    "Renée Fleming sang “Ave Maria” — à la française.",
    "Съешь же ещё этих мягких французских булок.",
    "日本語のテキスト、カタカナとひらがな。",
    "Emoji time 😀🎉👍🏽 and café crème",
] * 250

MOJIBAKE = [
    "RenÃ©e Fleming sang â€œAve Mariaâ€\x9d â€” Ã\xa0 la franÃ§aise.",
    "Ð¡ÑŠÐµÑˆÑŒ Ð¶Ðµ ÐµÑ‰Ñ‘ Ñ\x8dÑ‚Ð¸Ñ… Ð¼Ñ\x8fÐ³ÐºÐ¸Ñ…",
    "Ã¦â€”Â¥Ã¦Å“Â¬Ã¨ÂªÅ¾",
    "âœ” No problems, schÃ¶n",
] * 250

DOCUMENT = "\n".join(CASES * 3)


def time_calls(fn, inputs, min_time=1.0):
    """Return seconds per call over `inputs`, repeating until min_time passes."""
    for text in inputs[:50]:
        fn(text)
    rounds = 0
    start = time.perf_counter()
    while True:
        for text in inputs:
            fn(text)
        rounds += 1
        elapsed = time.perf_counter() - start
        if elapsed >= min_time:
            return elapsed / (rounds * len(inputs))


def main():
    print(f"Python {platform.python_version()} on {platform.platform()}")
    print(f"ftfy {ftfy.__version__} vs ftfy_rs {ftfy_rs.__version__}\n")
    workloads = [
        ("clean ASCII lines", CLEAN_ASCII),
        ("clean Unicode lines", CLEAN_UNICODE),
        ("mojibake lines", MOJIBAKE),
        ("ftfy's test cases", TEST_CASES),
        ("fuzz corpus (20k)", CASES),
    ]
    print(f"{'workload (fix_text)':24} {'ftfy':>12} {'ftfy_rs':>12} {'speedup':>9}")
    for name, inputs in workloads:
        py = time_calls(ftfy.fix_text, inputs)
        rs = time_calls(ftfy_rs.fix_text, inputs)
        print(f"{name:24} {py * 1e6:9.2f} us {rs * 1e6:9.2f} us {py / rs:8.1f}x")

    size_mb = len(DOCUMENT.encode("utf-8", "surrogatepass")) / 1e6
    py = time_calls(ftfy.fix_text, [DOCUMENT], min_time=3.0)
    rs = time_calls(ftfy_rs.fix_text, [DOCUMENT], min_time=3.0)
    print(
        f"{f'{size_mb:.1f} MB document':24} {size_mb / py:7.2f} MB/s {size_mb / rs:7.2f} MB/s"
        f" {py / rs:8.1f}x"
    )


if __name__ == "__main__":
    main()
