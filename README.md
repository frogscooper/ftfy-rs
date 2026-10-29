# ftfy-rs

A fast, drop-in replacement for [ftfy](https://github.com/rspeer/python-ftfy) ("fixes text for you"), written in Rust.

```python
import ftfy_rs as ftfy

ftfy.fix_text("âœ” No problems")        # '✔ No problems'
ftfy.fix_text("schÃ¶n")                 # 'schön'
ftfy.fix_and_explain("sÃ³")
# ExplainedText(text='só', explanation=[('encode', 'latin-1'), ('decode', 'utf-8')])
```

**Goal: byte-for-byte identical output to ftfy 6.3.1, 15–50× faster.**

## Status

Early, but it already matches ftfy everywhere we've tested:

- **ftfy's own test suite passes** against `ftfy_rs` (330 tests). The 10 cases ftfy marks as known failures fail in exactly the same way.
- **Differential fuzzing:** 20,000 generated inputs run through both libraries, with random configs, must give identical `fix_text`, `fix_and_explain` (including explanations), `fix_encoding`, `badness`, `possible_encoding`, and every individual fixer. Inputs cover mojibake in all 10 codecs ftfy handles, double mojibake, lossy `�`/`?` sequences, HTML entities, lone surrogates, line breaks, ANSI escapes and more. Every fix path in ftfy is exercised.

## Benchmarks

Windows 11, Python 3.13, `python bench/bench.py`:

| workload (`fix_text`)  | ftfy       | ftfy-rs   | speedup |
|------------------------|-----------:|----------:|--------:|
| clean ASCII lines      | 14.7 µs    | 0.29 µs   | **50×** |
| clean Unicode lines    | 29.9 µs    | 1.31 µs   | **23×** |
| mojibake lines         | 44.3 µs    | 2.40 µs   | **18×** |
| ftfy's test cases      | 58.7 µs    | 2.74 µs   | **21×** |
| fuzz corpus (20k)      | 69.9 µs    | 4.15 µs   | **17×** |
| 3.6 MB document        | 0.93 MB/s  | 16.2 MB/s | **17×** |

No profiling or tuning has been done yet. Long texts are fixed with the GIL released, so threads scale.

## How it stays faithful

ftfy's behavior lives mostly in a few big regexes and lookup tables. Rather than transcribing ~400 characters by hand, [`scripts/gen_tables.py`](scripts/gen_tables.py) generates [`tables.rs`](crates/ftfy-rs/src/tables.rs) from the installed ftfy:

- Python regexes are compiled to Rust `regex` syntax by walking their parsed form (`re._parser`), with Python's Unicode-aware `\w`/`\s`/`\d` expanded to the exact codepoint sets Python uses. Rust's definitions differ.
- Codec tables, HTML entities, the width map and ligatures are dumped directly.
- The one negative lookbehind (in `UTF8_DETECTOR_RE`) is applied by hand, since the `regex` crate has no lookaround.

The pipeline in [`lib.rs`](crates/ftfy-rs/src/lib.rs) then follows ftfy step by step, **quirks included**. For example, `fix_text` ignores the `restore_byte_a0`/`replace_lossy_sequences`/... sub-options unless `explain=True`. `unescape_html="auto"` also stays off for every later line once one line contains `<`. ftfy does both, so ftfy-rs does too.

## Using it

From Python (not on PyPI yet, so build from source with [maturin](https://www.maturin.rs/)):

```bash
pip install maturin
maturin develop --release
```

The API mirrors ftfy: `fix_text`, `fix_and_explain`, `fix_encoding`, `fix_encoding_and_explain`, `fix_text_segment`, `fix_file`, `guess_bytes`, `apply_plan`, `explain_unicode`, `TextFixerConfig`, `ftfy_rs.fixes.*`, `ftfy_rs.badness.*`, and the `sloppy-*`/`utf-8-variants` codecs.

From Rust:

```rust
use ftfy_rs::{fix_text, TextFixerConfig};

let fixed = fix_text("schÃ¶n", &TextFixerConfig::default());
assert_eq!(fixed, "schön");
```

## Known differences

- **Lone surrogates.** Rust strings can't hold them, so the Python bindings carry them through as private-use characters U+10F800–U+10FFFF. Input that contains *both* lone surrogates *and* real characters from that range may come out differently.
- **Unicode versions.** NFC/NFKC normalization comes from the `unicode-normalization` crate, whose Unicode version may differ slightly from your Python's `unicodedata`.
- **Not ported yet:** the `ftfy` command-line tool, and the less common parts of `ftfy.chardata`.

## Development

```bash
python -m venv .venv && .venv/Scripts/activate   # or bin/activate
pip install maturin pytest ftfy==6.3.1
maturin develop --release
pytest                       # upstream tests + parity tests against ftfy
cargo test -p ftfy-rs
python bench/bench.py
python scripts/gen_tables.py # regenerate tables.rs from the installed ftfy
```

## License

Apache-2.0, like ftfy. ftfy-rs is a port of ftfy by Robyn Speer; the vendored `bad_codecs` and `formatting` modules, the test data and the ported tests come from ftfy and are marked as such.
