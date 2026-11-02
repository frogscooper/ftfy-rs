# Changelog

## 0.1.0 (unreleased)

First release: a Rust port of [ftfy](https://github.com/rspeer/python-ftfy) 6.3.1, with Python bindings that work as a drop-in replacement.

- The same API as ftfy: `fix_text`, `fix_and_explain`, `fix_encoding`, `fix_encoding_and_explain`, `fix_text_segment`, `fix_file`, `guess_bytes`, `apply_plan`, `explain_unicode`, `TextFixerConfig`, `ftfy_rs.fixes`, `ftfy_rs.badness`, and the `sloppy-*` and `utf-8-variants` codecs.
- An `ftfy-rs` command that works like ftfy's `ftfy` command.
- Identical output to ftfy, explanations included. ftfy's own test suite passes, and 500,000 generated inputs match exactly.
- Unicode normalization matches the running Python's `unicodedata`, whatever its Unicode version.
- 22–78× faster than ftfy on short strings, and 200–500× on whole documents. Long texts are fixed on all cores with the GIL released.
- Reproduces ftfy's quirks, including its `utf-8-variants` codec treating a final newline as the last byte of a CESU-8 sequence.
- `fix_texts(texts)`, an addition to ftfy's API: fixes a list of strings in parallel with the GIL released, with the same results as calling `fix_text` on each.
- Unchanged text is returned as the same object, without a copy.
- A native `ftfy-rs` command, installable with Cargo, that matches ftfy's `ftfy` command byte for byte and runs on all cores: 62× faster on a 10 MB file.
- `ftfy_rs::codecs` for Rust: ftfy's sloppy codecs, `utf-8-variants`, `guess_bytes`, and Python-style decode errors.
- Also usable as a pure Rust crate, `ftfy-rs`. Its `parallel` feature (on by default) fixes long texts on all cores with rayon: 1–2 GB/s on 16 threads, and 260–600 MB/s on one.
