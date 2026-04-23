//! Benchmarks for `manga::parse` and `novel::parse`.
//!
//! Baseline shape: every Ryokan search result lands in this parser. For a
//! typical Nyaa query returning ~100 torrents, total parse time matters if
//! it's not utterly trivial — these benches pin the per-call cost and the
//! per-batch cost so regressions surface as numbers, not vibes.
//!
//! Run: `cargo bench`. Results print to stdout in Criterion's standard
//! format; no HTML reports (we disabled plotters to keep the dev-dep
//! footprint small).
//!
//! **Baselines** (John's dev machine, 2026-04-23):
//!
//! ```text
//! Benchmark                    1.1.0      1.2.0      Δ
//! ─────────────────────────────────────────────────────
//! manga::parse/simple          1.10 µs    740 ns    −33%
//! manga::parse/full grammar    1.53 µs    1.03 µs   −33%
//! manga::parse/cjk             2.12 µs    1.47 µs   −31%
//! manga::parse/range           1.70 µs    905 ns    −47%
//! novel::parse/full grammar    980 ns     635 ns    −35%
//! novel::parse/smoke corpus    1.18 ms    776 µs    −34%
//!                              (432 K/s)  (660 K/s)
//! ```
//!
//! The 1.2.0 speedup is across-the-board, not just CJK — the prior
//! `find_cjk_marker_number_in_word` allocated a `Vec<char>` on every call,
//! which fired once per Word token regardless of whether the word had any
//! CJK chars. Fixing the CJK path lifted every parse because every parse
//! walks the CJK-marker pass.
//!
//! At <1-1.5 µs per parse, a 100-result Nyaa search is <0.15 ms of
//! parsing — the parser is not the bottleneck. If that changes, bump the
//! numbers in this block when you commit.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};

// Representative manga filenames covering the detector passes:
//   - SIMPLE:   ASCII top-level prefix + parens, no allocation in parse
//   - FULL:     full grammar run (bracket group + vol + decimal chapter + source + ext)
//   - CJK:      CJK marker path (第N卷)
//   - RANGE:    underscore-delimited, bare-number chapter metadata fallback
const MANGA_SIMPLE: &str = "BTOOOM! v01 (2013) (Digital) (Shadowcat-Empire)";
const MANGA_FULL: &str = "[Yen Press] Sword Art Online v10 c042.5 (Digital).cbz";
const MANGA_CJK: &str = "幽游白书完全版 第03卷 天下";
const MANGA_RANGE: &str = "Historys Strongest Disciple Kenichi_v11_c90-98.zip";

const NOVEL_FULL: &str = "[Unpaid Ferryman] Youjo Senki v01-23 (2018-2024) (Digital) (LuCaZ)";

/// The full 512-entry smoke corpus, compiled in so the benchmark doesn't
/// touch the filesystem between iterations.
const CORPUS: &str = include_str!("../corpus/smoke_novel.txt");

fn corpus_lines() -> Vec<&'static str> {
    CORPUS
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

fn bench_manga(c: &mut Criterion) {
    let mut group = c.benchmark_group("manga::parse");
    group.bench_function("simple", |b| {
        b.iter(|| chaptr::manga::parse(black_box(MANGA_SIMPLE)));
    });
    group.bench_function("full grammar", |b| {
        b.iter(|| chaptr::manga::parse(black_box(MANGA_FULL)));
    });
    group.bench_function("cjk", |b| {
        b.iter(|| chaptr::manga::parse(black_box(MANGA_CJK)));
    });
    group.bench_function("range", |b| {
        b.iter(|| chaptr::manga::parse(black_box(MANGA_RANGE)));
    });
    group.finish();
}

fn bench_novel(c: &mut Criterion) {
    let mut group = c.benchmark_group("novel::parse");
    group.bench_function("full grammar", |b| {
        b.iter(|| chaptr::novel::parse(black_box(NOVEL_FULL)));
    });

    let lines = corpus_lines();
    group.throughput(Throughput::Elements(lines.len() as u64));
    group.bench_function("smoke corpus", |b| {
        b.iter(|| {
            for line in &lines {
                let _ = chaptr::novel::parse(black_box(line));
            }
        });
    });
    group.finish();
}

criterion_group!(benches, bench_manga, bench_novel);
criterion_main!(benches);
