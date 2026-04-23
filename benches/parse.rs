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
//! **Baseline (1.1.0 release, ran on John's dev machine, 2026-04-23):**
//!
//! ```text
//! manga::parse/simple         1.10 µs
//! manga::parse/full grammar   1.53 µs
//! manga::parse/cjk            2.12 µs
//! manga::parse/range          1.70 µs
//! novel::parse/full grammar   0.98 µs
//! novel::parse/smoke corpus   1.18 ms   (512 entries, ~432 K entries/sec)
//! ```
//!
//! At ~1-2 µs per parse, a 100-result Nyaa search is <0.2 ms of parsing —
//! the parser is not the bottleneck. If that changes (e.g. future CJK
//! work adds allocation per token), bump the numbers in the Baseline
//! block when you commit.

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
