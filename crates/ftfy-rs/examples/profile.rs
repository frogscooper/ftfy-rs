//! Time each stage of the pipeline over a corpus, to see where fix_text
//! spends its time.
//!
//! The corpus is a file of length-prefixed strings (little-endian u32 byte
//! length, then UTF-8), as written by `bench/dump_corpus.py`.
//!
//! Usage:  cargo run --release --example profile -- corpus.bin

use std::hint::black_box;
use std::time::{Duration, Instant};

use ftfy_rs::{fixes, Normalization, TextFixerConfig};

fn load(path: &str) -> Vec<String> {
    let data = std::fs::read(path).expect("read corpus");
    let mut cases = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let len = u32::from_le_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        cases.push(String::from_utf8(data[i + 4..i + 4 + len].to_vec()).unwrap());
        i += 4 + len;
    }
    cases
}

fn time(cases: &[String], f: impl Fn(&str)) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..5 {
        let start = Instant::now();
        for case in cases {
            f(case);
        }
        best = best.min(start.elapsed());
    }
    best / cases.len() as u32
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: profile <corpus.bin>");
    let cases = load(&path);
    let bad: Vec<String> = cases.iter().filter(|s| ftfy_rs::is_bad(s)).cloned().collect();
    let config = TextFixerConfig::no_explain();
    let explain = TextFixerConfig::default();
    println!("{} cases, {} of them look bad
", cases.len(), bad.len());

    let rows: Vec<(&str, Duration)> = vec![
        ("fix_text", time(&cases, |s| { black_box(ftfy_rs::fix_text(s, &config)); })),
        ("fix_and_explain", time(&cases, |s| { black_box(ftfy_rs::fix_and_explain(s, &explain)); })),
        ("fix_encoding", time(&cases, |s| { black_box(ftfy_rs::fix_encoding(s, &config)); })),
        ("fix_encoding (bad only)", time(&bad, |s| { black_box(ftfy_rs::fix_encoding(s, &config)); })),
        ("is_bad", time(&cases, |s| { black_box(ftfy_rs::is_bad(s)); })),
        ("possible_encodings", time(&cases, |s| { black_box(ftfy_rs::chardata::possible_encodings(s)); })),
        ("has_utf8_mojibake", time(&cases, |s| { black_box(fixes::has_utf8_mojibake(s)); })),
        ("decode_inconsistent_utf8", time(&cases, |s| { black_box(fixes::decode_inconsistent_utf8(s)); })),
        ("unescape_html", time(&cases, |s| { black_box(fixes::unescape_html(s)); })),
        ("fix_c1_controls", time(&cases, |s| { black_box(fixes::fix_c1_controls(s)); })),
        ("fix_latin_ligatures", time(&cases, |s| { black_box(fixes::fix_latin_ligatures(s)); })),
        ("fix_character_width", time(&cases, |s| { black_box(fixes::fix_character_width(s)); })),
        ("uncurl_quotes", time(&cases, |s| { black_box(fixes::uncurl_quotes(s)); })),
        ("fix_line_breaks", time(&cases, |s| { black_box(fixes::fix_line_breaks(s)); })),
        ("remove_terminal_escapes", time(&cases, |s| { black_box(fixes::remove_terminal_escapes(s)); })),
        ("remove_control_chars", time(&cases, |s| { black_box(fixes::remove_control_chars(s)); })),
        ("normalize NFC", time(&cases, |s| { black_box(ftfy_rs::normalize(s, Normalization::Nfc)); })),
    ];
    for (name, per_case) in rows {
        println!("{name:26} {:8.3} us", per_case.as_secs_f64() * 1e6);
    }

    let document = cases.join("\n");
    let start = Instant::now();
    let mut runs = 0;
    while start.elapsed() < Duration::from_secs(3) {
        black_box(ftfy_rs::fix_text(&document, &config));
        runs += 1;
    }
    let mb = document.len() as f64 / 1e6;
    println!("\n{mb:.1} MB document: {:.1} MB/s", mb * runs as f64 / start.elapsed().as_secs_f64());
}
