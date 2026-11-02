"""
Compare the command-line tools on a large file: ftfy's `ftfy`, the Python
`ftfy-rs` command (ftfy's CLI on top of ftfy_rs), and the native binary.

Usage:  python bench/bench_cli.py [megabytes]
"""

import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tests"))
from test_parity import CASES  # noqa: E402

NATIVE_BIN = ROOT / "target" / "release" / ("ftfy-rs.exe" if os.name == "nt" else "ftfy-rs")


def main():
    megabytes = float(sys.argv[1]) if len(sys.argv) > 1 else 10
    lines = [
        line
        for case in CASES
        for line in case.replace("\r", "").split("\n")
        if not any(0xD800 <= ord(ch) <= 0xDFFF for ch in line)
    ]
    chunk = ("\n".join(lines) + "\n").encode("utf-8")
    data = chunk * max(1, round(megabytes * 1e6 / len(chunk)))
    size_mb = len(data) / 1e6

    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "input.txt"
        path.write_bytes(data)
        commands = [
            ("ftfy (Python)", [sys.executable, "-m", "ftfy.cli"]),
            ("ftfy-rs (Python CLI)", [sys.executable, "-m", "ftfy_rs.cli"]),
            ("ftfy-rs (native)", [str(NATIVE_BIN)]),
        ]
        print(f"{size_mb:.1f} MB of text, {os.cpu_count()} CPU threads\n")
        results = []
        for name, command in commands:
            start = time.perf_counter()
            subprocess.run(
                command + [str(path), "-o", str(Path(tmp) / "out.txt")],
                check=True,
                env={**os.environ, "PYTHONIOENCODING": "utf-8"},
            )
            seconds = time.perf_counter() - start
            results.append((name, seconds))
        baseline = results[0][1]
        for name, seconds in results:
            print(
                f"{name:22} {seconds:8.2f} s {size_mb / seconds:8.1f} MB/s"
                f" {baseline / seconds:8.1f}x"
            )


if __name__ == "__main__":
    main()
