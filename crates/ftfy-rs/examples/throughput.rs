//! Measure fix_text throughput on whole documents, such as the ones written
//! by `bench/make_docs.py`, on one thread and on all of them.
//!
//! Usage:  cargo run --release --example throughput -- file.txt...

use std::hint::black_box;
use std::time::{Duration, Instant};

use ftfy_rs::TextFixerConfig;

fn megabytes_per_second(text: &str) -> f64 {
    let config = TextFixerConfig::no_explain();
    let start = Instant::now();
    let mut runs = 0;
    while start.elapsed() < Duration::from_secs(2) || runs < 3 {
        black_box(ftfy_rs::fix_text_cow(text, &config));
        runs += 1;
    }
    text.len() as f64 / 1e6 * runs as f64 / start.elapsed().as_secs_f64()
}

fn main() {
    let one_thread = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    println!(
        "{:16} {:>6} {:>12} {:>12}",
        "",
        "MB",
        "1 thread",
        format!("{} threads", rayon::current_num_threads())
    );
    for path in std::env::args().skip(1) {
        let text = std::fs::read_to_string(&path).expect("read document");
        let single = one_thread.install(|| megabytes_per_second(&text));
        let all = megabytes_per_second(&text);
        let name = std::path::Path::new(&path)
            .file_name()
            .unwrap()
            .to_string_lossy();
        println!(
            "{name:16} {:6.1} {single:7.0} MB/s {all:7.0} MB/s",
            text.len() as f64 / 1e6
        );
    }
}
