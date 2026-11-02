"""
The native ftfy-rs binary must behave exactly like ftfy's `ftfy` command:
the same output, the same error messages and the same exit code, for the
same input and options.
"""

import os
import subprocess
import sys

import pytest

from test_parity import CASES

from upstream.test_cli import NATIVE_BIN

pytestmark = pytest.mark.skipif(
    not NATIVE_BIN.exists(), reason=f"native binary not built: {NATIVE_BIN}"
)

ENV = {**os.environ, "PYTHONIOENCODING": "utf-8"}

# Lines that can be encoded as UTF-8, i.e. without lone surrogates.
LINES = [
    line
    for case in CASES[:6000]
    for line in case.replace("\r", "").split("\n")
    if not any(0xD800 <= ord(ch) <= 0xDFFF for ch in line)
]


def make_inputs(tmp_path):
    def write(name, data):
        path = tmp_path / name
        path.write_bytes(data)
        return path

    text = "\n".join(LINES) + "\n"
    half = len(LINES) // 2
    html_halfway = "\n".join(LINES[:half] + ["<p>now it's HTML &amp; stays that way</p>"] + LINES[half:])
    return {
        "utf-8": write("utf8.txt", text.encode("utf-8")),
        "crlf": write("crlf.txt", text.replace("\n", "\r\n").encode("utf-8")),
        "html halfway": write("html.txt", html_halfway.encode("utf-8")),
        "no final newline": write("nofinal.txt", text.rstrip("\n").encode("utf-8")),
        "latin-1": write("latin1.txt", text.encode("latin-1", "replace")),
        "windows-1252": write("cp1252.txt", text.encode("windows-1252", "replace")),
        "macroman, CR only": write(
            "mac.txt", "\r".join(LINES[:300]).encode("mac_roman", "replace")
        ),
        "bad byte halfway": write(
            "bad.txt",
            "\n".join(LINES[:half]).encode("utf-8")
            + b"\nthis line has a bad byte: \xff\n"
            + "\n".join(LINES[half:]).encode("utf-8"),
        ),
        "empty": write("empty.txt", b""),
    }


OPTIONS = [
    [],
    ["-g"],
    ["-e", "latin-1"],
    ["-e", "windows-1252"],
    ["-e", "sloppy-windows-1252"],
    ["-e", "macroman"],
    ["-e", "utf-8-variants"],
    ["-n", "NFKC"],
    ["-n", "none"],
    ["--preserve-entities"],
]


def run(command, args):
    result = subprocess.run(command + args, capture_output=True, env=ENV, timeout=300)
    return result.returncode, result.stdout, result.stderr


@pytest.fixture(scope="module")
def inputs(tmp_path_factory):
    return make_inputs(tmp_path_factory.mktemp("cli"))


@pytest.mark.parametrize("options", OPTIONS, ids=lambda o: " ".join(o) or "defaults")
def test_same_as_ftfy(inputs, options):
    for name, path in inputs.items():
        expected = run([sys.executable, "-m", "ftfy.cli"], options + [str(path)])
        actual = run([str(NATIVE_BIN)], options + [str(path)])
        assert actual[0] == expected[0], (name, actual[2], expected[2])
        assert actual[2] == expected[2], name
        assert actual[1] == expected[1], name


def test_stdin_and_output_file(inputs, tmp_path):
    path = inputs["windows-1252"]
    expected_out = tmp_path / "expected.txt"
    actual_out = tmp_path / "actual.txt"
    with open(path, "rb") as stdin:
        subprocess.run(
            [sys.executable, "-m", "ftfy.cli", "-g", "-o", str(expected_out)],
            stdin=stdin, env=ENV, check=True,
        )
    with open(path, "rb") as stdin:
        subprocess.run([str(NATIVE_BIN), "-g", "-o", str(actual_out)], stdin=stdin, check=True)
    assert actual_out.read_bytes() == expected_out.read_bytes()
