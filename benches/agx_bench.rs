//! Criterion benchmarks for agx's hot paths — format loading, totals /
//! tool-stats aggregation, and corpus parallel load.
//!
//! Run:
//! ```
//! cargo bench --bench agx_bench
//! cargo bench --bench agx_bench -- --save-baseline main
//! cargo bench --bench agx_bench -- --baseline main
//! ```
//!
//! These live at the library layer (src/lib.rs) so criterion can import
//! the parsers directly without going through the binary. Fixtures are
//! the same synthetic sessions used by unit tests — zero personal data,
//! stable under CI.

use agx::corpus;
use agx::loader::load_session;
use agx::timeline::{Step, compute_session_totals, compute_tool_stats};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::path::{Path, PathBuf};

/// Every format's synthetic fixture. Keeps the bench list coupled to
/// `assets/` so adding a new format surfaces here immediately.
const FIXTURES: &[(&str, &str)] = &[
    ("claude_code", "assets/sample_session.jsonl"),
    ("codex", "assets/sample_codex_session.jsonl"),
    ("gemini", "assets/sample_gemini_session.json"),
    ("generic", "assets/sample_generic_session.json"),
    ("langchain", "assets/sample_langchain_export.json"),
    ("otel_json", "assets/sample_otel_json_traces.json"),
    ("vercel_ai", "assets/sample_vercel_ai_session.json"),
];

/// Measure cold-cache load for each format. Throughput is set to the
/// raw file size so criterion reports bytes/sec — useful when comparing
/// parsers or tracking streaming regressions.
fn bench_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("load");
    for (name, path) in FIXTURES {
        let p = Path::new(path);
        if !p.exists() {
            // Missing fixture is a signal to update FIXTURES; warn but
            // don't fail so partial-repo checkouts still run the rest
            // of the bench suite.
            eprintln!("skipping {name}: fixture {path} not found");
            continue;
        }
        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        group.throughput(Throughput::Bytes(size));
        group.bench_with_input(BenchmarkId::from_parameter(name), path, |b, path| {
            b.iter(|| {
                let steps = load_session(Path::new(path)).expect("load session");
                std::hint::black_box(steps);
            });
        });
    }
    group.finish();
}

/// Aggregate benchmarks — run after a cheap load so the timer only
/// measures the aggregation. Varies step count to surface O(N^2)
/// regressions (e.g. in `unique_models` dedup).
fn bench_aggregate(c: &mut Criterion) {
    let base_steps =
        load_session(Path::new("assets/sample_session.jsonl")).expect("load base fixture");
    let mut group = c.benchmark_group("aggregate");
    for &n in &[100_usize, 1_000, 10_000] {
        // Replicate + relabel to simulate a larger session with the
        // same shape. Keeps the bench self-contained — no gigabyte
        // fixtures checked into git.
        let steps: Vec<Step> = (0..n)
            .map(|i| {
                let idx = i % base_steps.len();
                base_steps[idx].clone()
            })
            .collect();
        group.bench_with_input(
            BenchmarkId::new("compute_session_totals", n),
            &steps,
            |b, s| {
                b.iter(|| std::hint::black_box(compute_session_totals(s)));
            },
        );
        group.bench_with_input(BenchmarkId::new("compute_tool_stats", n), &steps, |b, s| {
            b.iter(|| std::hint::black_box(compute_tool_stats(s)));
        });
    }
    group.finish();
}

/// End-to-end corpus load covering file discovery + parallel parse +
/// aggregate. Uses the repo's own `assets/` directory so multiple formats
/// exercise the rayon pool. Not throughput-scaled because the file set is
/// fixed; criterion's default ms-per-iter number is the useful signal.
fn bench_corpus(c: &mut Criterion) {
    let dir = PathBuf::from("assets");
    if !dir.is_dir() {
        eprintln!("skipping corpus bench: assets/ not found");
        return;
    }
    c.bench_function("corpus_load_parallel", |b| {
        b.iter(|| {
            let paths = corpus::discover_files(&dir, 8);
            let (parsed, errors) = corpus::load_parallel(&paths);
            std::hint::black_box((parsed, errors));
        });
    });
}

criterion_group!(benches, bench_load, bench_aggregate, bench_corpus);
criterion_main!(benches);
