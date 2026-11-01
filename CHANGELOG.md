# Changelog

## 0.1.0 (unreleased)

First release: a Rust port of [ftfy](https://github.com/rspeer/python-ftfy) 6.3.1, with Python bindings that work as a drop-in replacement.

- The same API as ftfy: `fix_text`, `fix_and_explain`, `fix_encoding`, `fix_encoding_and_explain`, `fix_text_segment`, `fix_file`, `guess_bytes`, `apply_plan`, `explain_unicode`, `TextFixerConfig`, `ftfy_rs.fixes`, `ftfy_rs.badness`, and the `sloppy-*` and `utf-8-variants` codecs.
- An `ftfy-rs` command that works like ftfy's `ftfy` command.
- Identical output to ftfy, explanations included. ftfy's own test suite passes, and 500,000 generated inputs match exactly.
- Unicode normalization matches the running Python's `unicodedata`, whatever its Unicode version.
- 19–52× faster than ftfy on the included benchmarks. Long texts are fixed with the GIL released.
- Reproduces ftfy's quirks, including its `utf-8-variants` codec treating a final newline as the last byte of a CESU-8 sequence.
- Also usable as a pure Rust crate, `ftfy-rs`.
