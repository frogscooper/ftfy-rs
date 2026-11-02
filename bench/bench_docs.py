"""
Compare ftfy and ftfy_rs on whole documents, such as the ones written by
bench/make_docs.py. ftfy only gets the first megabyte of each, so the run
doesn't take minutes; the outputs are checked to match on that part.

Usage:  python bench/bench_docs.py bench/docs/*.txt
"""

import os
import sys
import time
from pathlib import Path

import ftfy

import ftfy_rs


def megabytes_per_second(fix, text, min_seconds=2.0):
    runs = 0
    start = time.perf_counter()
    while True:
        fix(text)
        runs += 1
        elapsed = time.perf_counter() - start
        if elapsed >= min_seconds and runs >= 3:
            return len(text.encode("utf-8")) / 1e6 * runs / elapsed


def main():
    print(f"{os.cpu_count()} CPU threads\n")
    print(f"{'':12} {'MB':>5} {'ftfy':>10} {'ftfy_rs':>11} {'speedup':>8}")
    for path in map(Path, sys.argv[1:]):
        text = path.read_text(encoding="utf-8")
        sample = text[: text.index("\n", 1_000_000) + 1]
        assert ftfy_rs.fix_text(sample) == ftfy.fix_text(sample), path
        slow = megabytes_per_second(ftfy.fix_text, sample, min_seconds=0)
        fast = megabytes_per_second(ftfy_rs.fix_text, text)
        print(
            f"{path.stem:12} {len(text.encode('utf-8')) / 1e6:5.1f}"
            f" {slow:5.1f} MB/s {fast:6.0f} MB/s {fast / slow:7.0f}x"
        )


if __name__ == "__main__":
    main()
