# ftfy-rs

A fast, drop-in replacement for [ftfy](https://github.com/rspeer/python-ftfy) ("fixes text for you"), written in Rust.

```python
import ftfy_rs as ftfy

ftfy.fix_text("âœ” No problems")        # '✔ No problems'
ftfy.fix_text("schÃ¶n")                 # 'schön'
ftfy.fix_and_explain("sÃ³")
# ExplainedText(text='só', explanation=[('encode', 'latin-1'), ('decode', 'utf-8')])
```

**Byte-for-byte identical output to ftfy 6.3.1: 20–80× faster on single strings, and 200–500× on whole documents.**

## Status

Early, but it already matches ftfy everywhere we've tested:

- **ftfy's own test suite passes** against `ftfy_rs` (342 tests, including the CLI). The 10 cases ftfy marks as known failures fail in exactly the same way.
- **Unicode normalization matches your Python's `unicodedata`** for every codepoint in all four forms, even though the Rust side ships newer Unicode data (see below).
- **Differential fuzzing:** 20,000 generated inputs (500,000 in occasional longer runs) run through both libraries, with random configs, must give identical `fix_text`, `fix_and_explain` (including explanations), `fix_encoding`, `badness`, `possible_encoding`, and every individual fixer. Inputs cover mojibake in all 10 codecs ftfy handles, double mojibake, lossy `�`/`?` sequences, HTML entities, lone surrogates, line breaks, ANSI escapes and more. Every fix path in ftfy is exercised.
- **The shortcuts are checked too:** 200,000 random strings packed with the characters ftfy's mojibake regex looks for (about 45% of them bad) must get the same `is_bad` and `badness`, and whole documents must come out the same with every `max_decode_length` and HTML setting.

## Benchmarks

Windows 11, Python 3.13, `python bench/bench.py`:

| workload (`fix_text`)  | ftfy       | ftfy-rs   | speedup  |
|------------------------|-----------:|----------:|---------:|
| clean ASCII lines      | 15.8 µs    | 0.21 µs   | **75×**  |
| clean Unicode lines    | 32.6 µs    | 0.42 µs   | **78×**  |
| mojibake lines         | 48.9 µs    | 1.46 µs   | **34×**  |
| ftfy's test cases      | 69.9 µs    | 1.65 µs   | **42×**  |
| fuzz corpus (20k)      | 68.9 µs    | 3.13 µs   | **22×**  |
| 3.6 MB document        | 0.88 MB/s  | 94 MB/s   | **107×** |

The fuzz corpus is three-quarters mojibake, so it's the slow case. `import ftfy_rs` takes about 35 ms, against 98 ms for ftfy.

**Documents.** Real text is mostly correct, and that's where ftfy-rs is fastest. On 10 MB documents written by [`bench/make_docs.py`](https://github.com/frogscooper/ftfy-rs/blob/master/bench/make_docs.py), mostly clean text with curly quotes, HTML entities, CRLFs and a little mojibake mixed in (`python bench/bench_docs.py`, 16 CPU threads):

| document | ftfy      | ftfy_rs  | speedup  |
|----------|----------:|---------:|---------:|
| English  | 2.7 MB/s  | 848 MB/s | **314×** |
| French   | 1.3 MB/s  | 686 MB/s | **518×** |
| Russian  | 3.8 MB/s  | 753 MB/s | **199×** |
| Chinese  | 2.8 MB/s  | 634 MB/s | **223×** |
| tweets   | 1.5 MB/s  | 598 MB/s | **402×** |

Long texts are fixed on all cores, with the GIL released. From Rust, without converting Python strings, the same documents go through at 1.1–2.2 GB/s, or 260–600 MB/s on one thread (`cargo run --release --example throughput -- bench/docs/*.txt`).

**Batches.** `ftfy_rs.fix_texts(texts)` fixes a whole list across all CPU cores. On 100,000 strings from the fuzz corpus, with 16 CPU threads:

| 100,000 strings              | time    | vs ftfy  |
|------------------------------|--------:|---------:|
| `ftfy.fix_text` in a loop    | 7.05 s  | 1×       |
| `ftfy_rs.fix_text` in a loop | 0.32 s  | 22×      |
| `ftfy_rs.fix_texts`          | 0.07 s  | **101×** |

To see where the Rust side spends its time, run `cargo run --release --example profile -- corpus.bin` (see [`bench/dump_corpus.py`](https://github.com/frogscooper/ftfy-rs/blob/master/bench/dump_corpus.py)), or pass it a `.txt` document to profile line by line.

## How it's fast

Most text needs no fixing, so most of the speed comes from proving that cheaply and exactly:

- **Bulk skipping.** A segment can only change if it has a byte outside printable ASCII, or an `&` or `<` that HTML unescaping cares about. `fix_text` looks for those eight bytes at a time and jumps straight to the next segment that has one.
- **One scan per segment.** A single pass records which fixers could possibly change the segment, and the rest are skipped. An exhaustive test over all 1,114,112 codepoints checks that skipping never changes the result.
- **A prefilter generated from ftfy's regex.** ftfy's mojibake detector is one big regex. [`scripts/gen_tables.py`](https://github.com/frogscooper/ftfy-rs/blob/master/scripts/gen_tables.py) walks its parsed form and derives, for every alternative, a character or a pair of adjacent characters that every match must contain, chosen to be rare in correct text. Ordinary English, French, Russian or Chinese never triggers it, so the regex doesn't run.
- **Normalization tables.** Characters that NFC or NFKC can never change, and that can't change their neighbors, are tabulated. Text made only of those skips normalization entirely.
- **All cores.** Long texts are split at line starts, which are always segment boundaries, and fixed in parallel. One search for the first `<` tells each chunk whether HTML unescaping is still on.

## How it stays faithful

ftfy's behavior lives mostly in a few big regexes and lookup tables. Rather than transcribing ~400 characters by hand, [`scripts/gen_tables.py`](https://github.com/frogscooper/ftfy-rs/blob/master/scripts/gen_tables.py) generates [`tables.rs`](https://github.com/frogscooper/ftfy-rs/blob/master/crates/ftfy-rs/src/tables.rs) from the installed ftfy:

- Python regexes are compiled to Rust `regex` syntax by walking their parsed form (`re._parser`), with Python's Unicode-aware `\w`/`\s`/`\d` expanded to the exact codepoint sets Python uses. Rust's definitions differ.
- Codec tables, HTML entities, the width map and ligatures are dumped directly.
- The one negative lookbehind (in `UTF8_DETECTOR_RE`) is applied by hand, since the `regex` crate has no lookaround.

The pipeline in [`lib.rs`](https://github.com/frogscooper/ftfy-rs/blob/master/crates/ftfy-rs/src/lib.rs) then follows ftfy step by step, **quirks included**. ftfy does each of these, so ftfy-rs does too:

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

To fix many strings at once, such as a column of data, use `fix_texts`. It's an addition that ftfy doesn't have, and gives the same results as calling `fix_text` on each string, but runs in parallel with the GIL released:

```python
fixed = ftfy_rs.fix_texts(df["comment"].tolist())
```

There's also a native `ftfy-rs` command, written in Rust, for large files. It takes the same options as ftfy's command and gives byte-for-byte the same output and error messages, but runs on all cores. On a 10.5 MB file with 16 CPU threads (`python bench/bench_cli.py`):

| command                         | time    | speed     | vs ftfy |
|---------------------------------|--------:|----------:|--------:|
| `ftfy` (Python)                 | 11.9 s  | 0.9 MB/s  | 1×      |
| `ftfy-rs` (from the Python package) | 1.13 s | 9.3 MB/s | 10× |
| `ftfy-rs` (native)              | 0.19 s  | 56 MB/s   | **62×** |

Install it with Cargo:

```bash
cargo install --git https://github.com/frogscooper/ftfy-rs ftfy-rs
```

The native command handles UTF-8, `utf-8-variants`, UTF-16, Latin-1, Windows-1252, MacRoman, CP437, ISO-8859-2 and ftfy's `sloppy-windows-*` codecs. For any other encoding, use the Python one: `python -m ftfy_rs.cli -e shift_jis ...`.

The API mirrors ftfy: `fix_text`, `fix_and_explain`, `fix_encoding`, `fix_encoding_and_explain`, `fix_text_segment`, `fix_file`, `guess_bytes`, `apply_plan`, `explain_unicode`, `TextFixerConfig`, `ftfy_rs.fixes.*`, `ftfy_rs.badness.*`, and the `sloppy-*`/`utf-8-variants` codecs.

From Rust:

```rust
use ftfy_rs::{fix_text, TextFixerConfig};

let fixed = fix_text("schÃ¶n", &TextFixerConfig::default());
assert_eq!(fixed, "schön");
```

Texts over 256 KiB are fixed on rayon's global thread pool. To stay on one thread, turn off the default `parallel` feature: `ftfy-rs = { version = "0.1", default-features = false }`.

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
python bench/make_docs.py bench/docs && python bench/bench_docs.py bench/docs/*.txt
python scripts/gen_tables.py # regenerate tables.rs from the installed ftfy
```

## License

Apache-2.0, like ftfy. ftfy-rs is a port of ftfy by Robyn Speer; the vendored `bad_codecs` and `formatting` modules, the test data and the ported tests come from ftfy and are marked as such.
