"""
Write the parity-test corpus as length-prefixed UTF-8 strings, for the Rust
profiling example. Strings with lone surrogates are skipped, since Rust
strings can't hold them.

Usage:  python bench/dump_corpus.py corpus.bin
"""

import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "tests"))
from test_parity import CASES  # noqa: E402


def main():
    count = 0
    with open(sys.argv[1], "wb") as out:
        for case in CASES:
            if any(0xD800 <= ord(ch) <= 0xDFFF for ch in case):
                continue
            data = case.encode("utf-8")
            out.write(struct.pack("<I", len(data)))
            out.write(data)
            count += 1
    print(f"wrote {count} cases to {sys.argv[1]}")


if __name__ == "__main__":
    main()
