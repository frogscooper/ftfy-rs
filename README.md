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

- **ftfy's own test suite passes** against `ftfy_rs` (336 tests, including the CLI). The 10 cases ftfy marks as known failures fail in exactly the same way.
- **Unicode normalization matches your Python's `unicodedata`** for every codepoint in all four forms, even though the Rust side ships newer Unicode data (see below).
- **Differential fuzzing:** 20,000 generated inputs (500,000 in occasional longer runs) run through both libraries, with random configs, must give identical `fix_text`, `fix_and_explain` (including explanations), `fix_encoding`, `badness`, `possible_encoding`, and every individual fixer. Inputs cover mojibake in all 10 codecs ftfy handles, double mojibake, lossy `�`/`?` sequences, HTML entities, lone surrogates, line breaks, ANSI escapes and more. Every fix path in ftfy is exercised.

## Benchmarks

Windows 11, Python 3.13, `python bench/bench.py`:

| workload (`fix_text`)  | ftfy       | ftfy-rs   | speedup |
|------------------------|-----------:|----------:|--------:|
| clean ASCII lines      | 15.2 µs    | 0.29 µs   | **52×** |
| clean Unicode lines    | 31.4 µs    | 1.26 µs   | **25×** |
| mojibake lines         | 45.0 µs    | 1.97 µs   | **23×** |
| ftfy's test cases      | 59.8 µs    | 2.18 µs   | **27×** |
| fuzz corpus (20k)      | 71.1 µs    | 3.76 µs   | **19×** |
| 3.6 MB document        | 0.90 MB/s  | 18.6 MB/s | **21×** |

Long texts are fixed with the GIL released, so threads scale. `import ftfy_rs` takes about 35 ms, against 98 ms for ftfy.

To see where the Rust side spends its time, run `cargo run --release --example profile -- corpus.bin` (see [`bench/dump_corpus.py`](bench/dump_corpus.py)).

## How it stays faithful

ftfy's behavior lives mostly in a few big regexes and lookup tables. Rather than transcribing ~400 characters by hand, [`scripts/gen_tables.py`](scripts/gen_tables.py) generates [`tables.rs`](crates/ftfy-rs/src/tables.rs) from the installed ftfy:

- Python regexes are compiled to Rust `regex` syntax by walking their parsed form (`re._parser`), with Python's Unicode-aware `\w`/`\s`/`\d` expanded to the exact codepoint sets Python uses. Rust's definitions differ.
- Codec tables, HTML entities, the width map and ligatures are dumped directly.
- The one negative lookbehind (in `UTF8_DETECTOR_RE`) is applied by hand, since the `regex` crate has no lookaround.

The pipeline in [`lib.rs`](crates/ftfy-rs/src/lib.rs) then follows ftfy step by step, **quirks included**. ftfy does each of these, so ftfy-rs does too:

- `fix_text` ignores the `restore_byte_a0`/`replace_lossy_sequences`/... sub-options unless `explain=True`.
- `unescape_html="auto"` stays off for every later line once one line contains `<`.
- The `utf-8-variants` codec treats a final `\n` as the last byte of a CESU-8 sequence, because Python's regex `$` also matches just before a trailing newline. So `"\xed\xa0\xbd\xed\xb8\n"` decodes to 😊 and loses its newline.

## Using it

From Python (not on PyPI yet, so build from source with [maturin](https://www.maturin.rs/)):

```bash
pip install maturin
maturin develop --release
```

It also installs an `ftfy-rs` command that works like ftfy's `ftfy` command:

```bash
ftfy-rs mojibake.txt -o fixed.txt
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
- **Unicode versions.** Normalization uses the `unicode-normalization` crate's data (Unicode 18), which is newer than Python's. Because of Unicode's stability policy, the only characters that can differ are ones your Python doesn't know yet. So at import, `ftfy_rs` asks `unicodedata` which those are and leaves them alone, exactly as Python does. The pure Rust crate uses Unicode 18 as-is. The regex character classes (Python's `\w` and `\s`) are generated from Python 3.13, so on other Python versions, characters whose Unicode properties differ between that version and Unicode 15.1 may be classified differently than ftfy would there.
- **Not ported yet:** the less common parts of `ftfy.chardata`.

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
