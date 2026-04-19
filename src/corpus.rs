//! Corpus-level analytics for `agx corpus <dir>`. Walks a directory tree,
//! loads every session file it finds in parallel, and aggregates
//! cross-session stats (tokens, cost, per-model / per-tool / per-format
//! breakdowns).
//!
//! Design notes:
//!
//! - **Silent skip on non-session files.** The directory scan has no
//!   file-extension heuristic; we try every file and silently drop
//!   anything `format::detect` rejects. That lets users point agx at a
//!   dump of assorted files without getting noisy errors from `.DS_Store`
//!   / `README.md` / binaries. A file that LOOKS like a session but
//!   fails to parse still counts as an error — real format drift, not
//!   "this isn't a session file".
//!
//! - **Parallel parse via rayon.** Session files are embarrassingly
//!   parallel; on a typical corpus of a few hundred sessions the
//!   load phase fits under a second on a modern laptop. The walk
//!   itself stays serial (directory traversal is IO-bound enough that
//!   parallelism doesn't help, and a single `read_dir` iterator is
//!   simpler than managing a thread pool for the walk).
//!
//! - **Filters are AND-combined.** `--filter model=X --filter tool=Y`
//!   keeps only sessions that used both. Filter predicates run after
//!   per-session parse so we can filter on observed content.

use crate::format::{self, Format};
use crate::loader::load_session;
use crate::timeline::{SessionTotals, Step, compute_session_totals, compute_tool_stats};
use anyhow::{Result, anyhow};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One filter predicate from the `--filter` CLI flag. Multiple filters
/// are AND-combined by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// `--filter model=X` — keep sessions whose unique-models list includes X.
    Model(String),
    /// `--filter tool=X` — keep sessions that invoked tool X at least once.
    Tool(String),
    /// `--filter errored` — keep sessions where at least one tool_result
    /// matched `is_error_result`.
    Errored,
    /// `--filter annotated` — keep sessions with at least one user note
    /// stored under `~/.agx/notes/`.
    Annotated,
}

impl Filter {
    /// Parse one `--filter` value. Accepts `model=X`, `tool=X`, or one
    /// of the bare keywords `errored` / `annotated`.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("errored") {
            return Ok(Filter::Errored);
        }
        if s.eq_ignore_ascii_case("annotated") {
            return Ok(Filter::Annotated);
        }
        let (key, value) = s.split_once('=').ok_or_else(|| {
            anyhow!("--filter expects `key=value`, `errored`, or `annotated`, got `{s}`")
        })?;
        match key.trim() {
            "model" => Ok(Filter::Model(value.trim().to_string())),
            "tool" => Ok(Filter::Tool(value.trim().to_string())),
            other => Err(anyhow!(
                "unknown --filter key `{other}` (expected `model`, `tool`, `errored`, or `annotated`)"
            )),
        }
    }

    fn matches(&self, parsed: &ParsedSession) -> bool {
        match self {
            Filter::Model(m) => parsed.totals.unique_models.iter().any(|s| s == m),
            Filter::Tool(t) => parsed
                .tool_stats
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(t)),
            Filter::Errored => parsed.tool_stats.iter().any(|s| s.error_count > 0),
            Filter::Annotated => parsed.annotation_count > 0,
        }
    }
}

/// Result of parsing a single session file. Either a successful parse
/// with its derived aggregates, or a format-drift error we want to
/// surface in the corpus summary.
#[derive(Debug)]
pub struct ParsedSession {
    pub path: PathBuf,
    pub format: Format,
    pub totals: SessionTotals,
    pub tool_stats: Vec<crate::timeline::ToolStats>,
    pub step_count: usize,
    /// Unix timestamp in seconds of the session file's mtime, used by the
    /// corpus TUI to sort by recency. `None` when we couldn't stat the
    /// file (permission error, file replaced mid-walk, etc).
    pub mtime_secs: Option<u64>,
    /// Number of annotations stored for this session at the time of the
    /// scan (read from `~/.agx/notes/`). Used by `Filter::Annotated`
    /// and surfaced in `--jsonl` output for downstream tooling.
    pub annotation_count: usize,
    /// Number of fork-root steps in this session. Non-zero only for
    /// Claude Code sessions with edit/resume branches (Phase 5.1).
    /// Feeds `--trajectory-stats` branch-rate and is surfaced in
    /// `--jsonl` output.
    pub fork_root_count: usize,
}

#[derive(Debug)]
pub struct ParseError {
    pub path: PathBuf,
    pub error: anyhow::Error,
}

/// Recursive directory walk. Stdlib-only (no `walkdir` dep). Depth-limited
/// to avoid runaway recursion on symlink loops. Errors on individual
/// `read_dir` calls are silently skipped so permission-denied
/// subdirectories don't abort the whole scan.
pub fn discover_files(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, max_depth, &mut out);
    out
}

fn walk(root: &Path, max_depth: usize, out: &mut Vec<PathBuf>) {
    if max_depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, max_depth - 1, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

/// Load every path in parallel. Paths that fail format detection are
/// dropped silently; paths that detect successfully but fail to parse
/// are returned as `ParseError`s so they show up in the "errored" count.
///
/// Test hook: when `AGX_CORPUS_SERIAL=1` is set we skip rayon entirely.
/// Useful in tests where thread-pool init noise would confuse `cargo test`.
/// Three-way outcome for a single-file corpus load. Dedicated enum
/// (rather than smuggling "skip" through an `anyhow` error with a
/// magic substring) so misclassification can't be introduced by a
/// future refactor that reshuffles error messages.
enum LoadOutcome {
    Ok(Format, Vec<Step>),
    /// File wasn't recognized as any session format, or was a binary
    /// blob we shouldn't try to parse (e.g. images in an OtelProto-
    /// detection fallback when the feature is off). Dropped silently.
    Skip,
    Err(anyhow::Error),
}

/// Per-path load result — kept as a type alias so clippy's
/// `type_complexity` lint doesn't fire on the collect site below.
type RawLoad = (PathBuf, LoadOutcome);

pub fn load_parallel(paths: &[PathBuf]) -> (Vec<ParsedSession>, Vec<ParseError>) {
    let raw: Vec<RawLoad> = if std::env::var_os("AGX_CORPUS_SERIAL").is_some() {
        paths.iter().map(|p| (p.clone(), load_one(p))).collect()
    } else {
        paths.par_iter().map(|p| (p.clone(), load_one(p))).collect()
    };

    let mut parsed = Vec::new();
    let mut errors = Vec::new();
    for (path, result) in raw {
        match result {
            LoadOutcome::Ok(fmt, steps) => {
                let totals = compute_session_totals(&steps);
                let tool_stats = compute_tool_stats(&steps);
                let mtime_secs = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                // Load annotation count eagerly during the parallel
                // parse phase — it's one small disk read per session
                // (the notes JSON under ~/.agx/notes/), cheap enough
                // to do unconditionally so we can filter / display
                // without a second pass.
                let annotation_count = crate::annotations::Annotations::load_for(&path).notes.len();
                // Compute fork-root count before `steps` is moved so
                // we don't have to re-walk. Non-Claude-Code formats
                // always yield 0, so this is essentially free for
                // everything except Claude Code.
                let fork_root_count = crate::timeline::fork_root_count(&steps);
                parsed.push(ParsedSession {
                    path,
                    format: fmt,
                    totals,
                    tool_stats,
                    step_count: steps.len(),
                    mtime_secs,
                    annotation_count,
                    fork_root_count,
                });
            }
            LoadOutcome::Skip => {}
            LoadOutcome::Err(error) => errors.push(ParseError { path, error }),
        }
    }
    (parsed, errors)
}

fn load_one(path: &Path) -> LoadOutcome {
    // Detection failure → silent skip. Detection succeeds → attempt
    // parse and surface any failure as a real error.
    let fmt = match format::detect(path) {
        Ok(f) => f,
        Err(_) => return LoadOutcome::Skip,
    };
    match load_session(path) {
        Ok(steps) => LoadOutcome::Ok(fmt, steps),
        Err(e) => {
            // Non-UTF-8 files route to OtelProto at detection time. When
            // the `otel-proto` feature is off (the default build), those
            // files are almost always unrelated binaries (images, PDFs,
            // archives) rather than real OTLP protobuf exports the user
            // forgot to compile support for. Skip them silently rather
            // than spamming the "rebuild with --features" message across
            // every image in the tree. The compile-time `cfg!` is
            // strictly stronger than matching on the stub error message:
            // it triggers on any load failure, not just the exact string.
            if fmt == Format::OtelProto && !cfg!(feature = "otel-proto") {
                return LoadOutcome::Skip;
            }
            LoadOutcome::Err(e)
        }
    }
}

/// Aggregate stats across the surviving parsed sessions.
#[derive(Debug, Default, Serialize)]
pub struct CorpusStats {
    pub file_count: usize,
    pub parse_success_count: usize,
    pub parse_error_count: usize,
    pub filtered_out_count: usize,
    pub total_steps: usize,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
    pub total_cache_read: u64,
    pub total_cache_create: u64,
    pub total_cost_usd: Option<f64>,
    pub per_model: Vec<ModelBucket>,
    pub per_tool: Vec<ToolBucket>,
    pub per_format: Vec<FormatBucket>,
}

#[derive(Debug, Default, Serialize)]
pub struct ModelBucket {
    pub model: String,
    pub session_count: usize,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Default, Serialize)]
pub struct ToolBucket {
    pub tool: String,
    pub use_count: usize,
    pub error_count: usize,
    pub session_count: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct FormatBucket {
    pub format: String,
    pub session_count: usize,
}

/// Compute corpus-level stats from the parallel-load outputs.
pub fn aggregate(
    parsed: &[ParsedSession],
    errors: &[ParseError],
    file_count: usize,
    filtered_out: usize,
) -> CorpusStats {
    let mut stats = CorpusStats {
        file_count,
        parse_success_count: parsed.len(),
        parse_error_count: errors.len(),
        filtered_out_count: filtered_out,
        ..CorpusStats::default()
    };

    let mut model_map: HashMap<String, ModelBucket> = HashMap::new();
    let mut tool_map: HashMap<String, ToolBucket> = HashMap::new();
    let mut format_map: HashMap<String, usize> = HashMap::new();
    let mut any_cost: Option<f64> = None;

    for session in parsed {
        stats.total_steps += session.step_count;
        stats.total_tokens_in += session.totals.tokens_in;
        stats.total_tokens_out += session.totals.tokens_out;
        stats.total_cache_read += session.totals.cache_read;
        stats.total_cache_create += session.totals.cache_create;
        if let Some(c) = session.totals.cost_usd {
            any_cost = Some(any_cost.unwrap_or(0.0) + c);
        }

        *format_map.entry(session.format.to_string()).or_insert(0) += 1;

        // Per-model: session_count counts unique sessions that used the
        // model (not per-step). tokens/cost sum across all sessions that
        // used the model — over-attributes for multi-model sessions, but
        // multi-model sessions are rare and this is the simplest correct
        // behavior.
        for model in &session.totals.unique_models {
            let bucket = model_map
                .entry(model.clone())
                .or_insert_with(|| ModelBucket {
                    model: model.clone(),
                    ..ModelBucket::default()
                });
            bucket.session_count += 1;
            bucket.tokens_in += session.totals.tokens_in;
            bucket.tokens_out += session.totals.tokens_out;
            if let Some(c) = session.totals.cost_usd {
                bucket.cost_usd = Some(bucket.cost_usd.unwrap_or(0.0) + c);
            }
        }

        for tool in &session.tool_stats {
            let bucket = tool_map
                .entry(tool.name.clone())
                .or_insert_with(|| ToolBucket {
                    tool: tool.name.clone(),
                    ..ToolBucket::default()
                });
            bucket.use_count += tool.use_count;
            bucket.error_count += tool.error_count;
            bucket.session_count += 1;
        }
    }

    stats.total_cost_usd = any_cost;

    let mut models: Vec<ModelBucket> = model_map.into_values().collect();
    models.sort_by(|a, b| {
        b.session_count
            .cmp(&a.session_count)
            .then_with(|| a.model.cmp(&b.model))
    });
    stats.per_model = models;

    let mut tools: Vec<ToolBucket> = tool_map.into_values().collect();
    tools.sort_by(|a, b| {
        b.use_count
            .cmp(&a.use_count)
            .then_with(|| a.tool.cmp(&b.tool))
    });
    stats.per_tool = tools;

    let mut formats: Vec<FormatBucket> = format_map
        .into_iter()
        .map(|(format, session_count)| FormatBucket {
            format,
            session_count,
        })
        .collect();
    formats.sort_by(|a, b| {
        b.session_count
            .cmp(&a.session_count)
            .then_with(|| a.format.cmp(&b.format))
    });
    stats.per_format = formats;

    stats
}

/// One metric's distribution across the corpus — percentiles plus the
/// sum. Kept alongside the aggregate `CorpusStats` struct because the
/// shape is different (cross-session min/p50/p90/p99/max) and the
/// rendering is only used by `--trajectory-stats`.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Distribution {
    pub min: u64,
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub max: u64,
    pub mean: f64,
    pub total: u64,
}

impl Distribution {
    /// Build a distribution from an unsorted slice of per-session
    /// values. Zero-length input yields all-zero values so callers
    /// never have to branch. `mean` is `0.0` in that case rather than
    /// `NaN`.
    fn from_values(values: &[u64]) -> Self {
        if values.is_empty() {
            return Self::default();
        }
        let mut v = values.to_vec();
        v.sort_unstable();
        let n = v.len();
        let pick = |p: f64| -> u64 {
            // Nearest-rank percentile. Matches numpy's "lower"
            // interpolation and is the simplest correct choice for
            // integer-valued distributions. Clamp to valid indices.
            let idx = ((n as f64) * p).ceil() as usize;
            let idx = idx.saturating_sub(1).min(n - 1);
            v[idx]
        };
        let total: u64 = v.iter().sum();
        #[allow(clippy::cast_precision_loss)]
        let mean = total as f64 / n as f64;
        Self {
            min: v[0],
            p50: pick(0.50),
            p90: pick(0.90),
            p99: pick(0.99),
            max: v[n - 1],
            mean,
            total,
        }
    }
}

/// Dataset-level distribution stats for `agx corpus --trajectory-stats`.
/// Serialized directly when `--json` is combined; rendered as a terse
/// text report otherwise.
#[derive(Debug, Default, Clone, Serialize)]
pub struct TrajectoryStats {
    pub session_count: usize,
    pub steps_per_session: Distribution,
    pub tool_calls_per_session: Distribution,
    pub tokens_in_per_session: Distribution,
    pub tokens_out_per_session: Distribution,
    /// Fraction of sessions (0.0–1.0) that contain at least one fork
    /// root. Non-zero only when the corpus includes Claude Code
    /// edit/resume sessions.
    pub branched_rate: f64,
    /// Fraction of sessions with ≥1 stored annotation.
    pub annotated_rate: f64,
    /// Fraction of sessions where any tool_result matched the
    /// `is_error_result` heuristic.
    pub errored_rate: f64,
}

/// Build a `TrajectoryStats` from the surviving `ParsedSession` slice.
/// Pure function — the render step takes the output and prints
/// separately. Extracted so tests can assert on the stats struct
/// without spinning up a full run.
pub fn compute_trajectory_stats(parsed: &[ParsedSession]) -> TrajectoryStats {
    let session_count = parsed.len();
    if session_count == 0 {
        return TrajectoryStats::default();
    }
    let steps: Vec<u64> = parsed.iter().map(|p| p.step_count as u64).collect();
    let tool_calls: Vec<u64> = parsed
        .iter()
        .map(|p| p.tool_stats.iter().map(|t| t.use_count as u64).sum())
        .collect();
    let tokens_in: Vec<u64> = parsed.iter().map(|p| p.totals.tokens_in).collect();
    let tokens_out: Vec<u64> = parsed.iter().map(|p| p.totals.tokens_out).collect();
    #[allow(clippy::cast_precision_loss)]
    let branched =
        parsed.iter().filter(|p| p.fork_root_count > 0).count() as f64 / session_count as f64;
    #[allow(clippy::cast_precision_loss)]
    let annotated =
        parsed.iter().filter(|p| p.annotation_count > 0).count() as f64 / session_count as f64;
    #[allow(clippy::cast_precision_loss)]
    let errored = parsed
        .iter()
        .filter(|p| p.tool_stats.iter().any(|t| t.error_count > 0))
        .count() as f64
        / session_count as f64;
    TrajectoryStats {
        session_count,
        steps_per_session: Distribution::from_values(&steps),
        tool_calls_per_session: Distribution::from_values(&tool_calls),
        tokens_in_per_session: Distribution::from_values(&tokens_in),
        tokens_out_per_session: Distribution::from_values(&tokens_out),
        branched_rate: branched,
        annotated_rate: annotated,
        errored_rate: errored,
    }
}

fn print_trajectory_stats_text(stats: &TrajectoryStats) {
    println!("Trajectory stats — {} sessions", stats.session_count);
    if stats.session_count == 0 {
        println!("  (no sessions after filter / sample)");
        return;
    }
    let row = |label: &str, d: &Distribution| {
        println!(
            "  {label:<22}  min={:>8}  p50={:>8}  p90={:>8}  p99={:>8}  max={:>8}  mean={:>10.1}  total={:>12}",
            d.min, d.p50, d.p90, d.p99, d.max, d.mean, d.total
        );
    };
    row("steps/session", &stats.steps_per_session);
    row("tool_calls/session", &stats.tool_calls_per_session);
    row("tokens_in/session", &stats.tokens_in_per_session);
    row("tokens_out/session", &stats.tokens_out_per_session);
    println!();
    println!(
        "  branched:  {:>5.1}%    annotated: {:>5.1}%    errored: {:>5.1}%",
        stats.branched_rate * 100.0,
        stats.annotated_rate * 100.0,
        stats.errored_rate * 100.0
    );
}

/// Arguments for the `agx corpus` subcommand. Wired up in `main.rs`.
#[derive(Debug)]
pub struct CorpusArgs {
    pub dir: PathBuf,
    pub filters: Vec<Filter>,
    pub json: bool,
    pub no_cost: bool,
    pub max_depth: usize,
    /// When true, emit walk / load / aggregate timings to stderr after
    /// the main output. Wired from the hidden `--bench` CLI flag.
    pub bench: bool,
    /// When true, launch the interactive corpus TUI (session list +
    /// selected-session summary, Enter drills into the per-session TUI).
    /// Mutually exclusive with `--json` (the TUI owns the terminal; JSON
    /// needs stdout clean).
    pub tui: bool,
    /// When true, emit one JSON object per session to stdout instead of
    /// the default text summary or the aggregate `--json` blob. Parse
    /// errors go to stderr so stdout stays pipeable into `jq` / `xargs`.
    /// Intended for CI / eval pipelines.
    pub jsonl: bool,
    /// When true, exit with code 2 if any parse failure occurred OR any
    /// tool_result across the corpus matched the is_error_result
    /// heuristic. Exit 0 otherwise. Orthogonal to the rendering mode.
    pub fail_on_errored: bool,
    /// When true, replace the default aggregate rendering with a
    /// distributional breakdown across the corpus (percentiles for
    /// steps / tool-calls / tokens, plus branch / annotation / error
    /// rates). Combines with `--json` for machine-readable output.
    /// Phase 6.2 — the numbers a researcher needs before publishing a
    /// trajectory dataset.
    pub trajectory_stats: bool,
    /// When set, keep only the first N sessions after sorting by
    /// mtime descending (most recent first). Applied *after* filters
    /// so `--filter model=X --sample 20` gives you the 20 most
    /// recent sessions of that model. Phase 6.2 spot-check workflow.
    pub sample: Option<usize>,
}

/// Entry point called from `main.rs::main`. Walks the directory, loads
/// every session in parallel, applies filters, aggregates, and prints.
pub fn run(args: &CorpusArgs) -> Result<()> {
    use std::time::Instant;
    let t_walk = Instant::now();
    let files = discover_files(&args.dir, args.max_depth);
    let file_count = files.len();
    let walk_ms = t_walk.elapsed().as_secs_f64() * 1000.0;

    let t_load = Instant::now();
    let (mut parsed, errors) = load_parallel(&files);
    let load_ms = t_load.elapsed().as_secs_f64() * 1000.0;

    let t_agg = Instant::now();
    let before_filter = parsed.len();
    if !args.filters.is_empty() {
        parsed.retain(|p| args.filters.iter().all(|f| f.matches(p)));
    }
    let filtered_out = before_filter - parsed.len();
    // `--sample N` keeps the N most-recent-by-mtime sessions. Applied
    // after filters so `--filter model=X --sample 20` gives the 20
    // most recent X-model sessions. Deterministic (not random) to
    // avoid adding a PRNG dep and to keep runs reproducible — users
    // who want true random can `ls -u | shuf | head` and pass the
    // file list via another mechanism (future `--sample-random`
    // follow-up if demand surfaces).
    if let Some(n) = args.sample
        && parsed.len() > n
    {
        parsed.sort_by_key(|p| std::cmp::Reverse(p.mtime_secs));
        parsed.truncate(n);
    }
    let stats = aggregate(&parsed, &errors, file_count, filtered_out);
    let agg_ms = t_agg.elapsed().as_secs_f64() * 1000.0;

    // `--fail-on-errored` turns parse errors or tool-level error_results
    // into a nonzero exit. Evaluated before the rendering branch so we
    // don't have to clone `parsed` — the TUI path takes it by value.
    // The rendering side effects still run; the fail is reported at the
    // end via `anyhow::bail` (exit code 1 via anyhow's normal error
    // path — simpler than reserving a distinct code 2 and bypassing
    // anyhow's reporting for this one case).
    let parse_error_count = errors.len();
    let tool_error_count: usize = parsed
        .iter()
        .flat_map(|p| p.tool_stats.iter())
        .map(|t| t.error_count)
        .sum();
    let fail_on_errored = args.fail_on_errored && (parse_error_count > 0 || tool_error_count > 0);

    if args.trajectory_stats {
        // `--trajectory-stats` replaces the default aggregate rendering
        // with a distributional breakdown. Still compatible with `--json`
        // (emit the TrajectoryStats struct as JSON) and `--jsonl` (emit
        // stats to stderr so stdout stays per-session JSONL).
        let tstats = compute_trajectory_stats(&parsed);
        if args.jsonl {
            // Keep stdout per-session JSONL; dump trajectory stats to
            // stderr so both streams stay usable in a pipeline.
            print_jsonl(&parsed, &errors);
            eprintln!("{}", serde_json::to_string_pretty(&tstats)?);
        } else if args.json {
            println!("{}", serde_json::to_string_pretty(&tstats)?);
        } else {
            print_trajectory_stats_text(&tstats);
        }
    } else if args.tui {
        // Drop into the interactive corpus TUI. When the user selects a
        // session and hits Enter, the outer loop re-runs the TUI after
        // the drill-in per-session TUI exits.
        crate::corpus_tui::run(parsed, &stats, args.no_cost)?;
    } else if args.jsonl {
        print_jsonl(&parsed, &errors);
    } else if args.json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        print_text_summary(&stats, &args.dir, args.no_cost, &errors);
    }

    if fail_on_errored {
        anyhow::bail!(
            "--fail-on-errored: {parse_error_count} parse error(s), \
             {tool_error_count} tool-error result(s) detected",
        );
    }

    if args.bench {
        eprintln!(
            "[bench] walk: {:.2}ms ({} files)  load: {:.2}ms ({} parsed, {} errored)  aggregate: {:.2}ms  total: {:.2}ms",
            walk_ms,
            file_count,
            load_ms,
            stats.parse_success_count,
            stats.parse_error_count,
            agg_ms,
            walk_ms + load_ms + agg_ms,
        );
    }
    Ok(())
}

fn print_text_summary(stats: &CorpusStats, dir: &Path, no_cost: bool, errors: &[ParseError]) {
    println!("agx corpus {}", dir.display());
    println!(
        "  {} files scanned; {} parsed; {} errored; {} filtered out",
        stats.file_count,
        stats.parse_success_count,
        stats.parse_error_count,
        stats.filtered_out_count,
    );
    if stats.parse_success_count == 0 {
        println!("  (no sessions to aggregate)");
        return;
    }
    println!(
        "  Total: {} steps, {} input tokens, {} output, {} cache_read, {} cache_create",
        stats.total_steps,
        stats.total_tokens_in,
        stats.total_tokens_out,
        stats.total_cache_read,
        stats.total_cache_create,
    );
    if !no_cost {
        match stats.total_cost_usd {
            Some(c) => println!("  Estimated cost: ${c:.4} USD"),
            None if stats.total_tokens_in > 0 || stats.total_tokens_out > 0 => {
                println!("  Estimated cost: (no priced models detected)");
            }
            None => {}
        }
    }

    if !stats.per_format.is_empty() {
        println!("\nBy format:");
        for f in &stats.per_format {
            println!("  {:<32} {}", f.format, f.session_count);
        }
    }

    if !stats.per_model.is_empty() {
        println!("\nTop models:");
        for m in stats.per_model.iter().take(10) {
            let cost = match m.cost_usd {
                Some(c) if !no_cost => format!(" ${c:.4}"),
                _ => String::new(),
            };
            println!(
                "  {:<28} {:>4} sess  {:>10} in  {:>10} out{}",
                m.model, m.session_count, m.tokens_in, m.tokens_out, cost,
            );
        }
    }

    if !stats.per_tool.is_empty() {
        println!("\nTop tools:");
        for t in stats.per_tool.iter().take(10) {
            let err_pct = if t.use_count > 0 {
                #[allow(clippy::cast_precision_loss)]
                let r = t.error_count as f64 / t.use_count as f64;
                format!("({:.1}% err)", r * 100.0)
            } else {
                String::new()
            };
            println!(
                "  {:<28} {:>5} uses  {:>4} errors {}",
                t.tool, t.use_count, t.error_count, err_pct,
            );
        }
    }

    if !errors.is_empty() {
        println!("\nParse errors (first {}):", errors.len().min(5));
        for err in errors.iter().take(5) {
            println!("  {}: {}", err.path.display(), err.error);
        }
        if errors.len() > 5 {
            println!("  ... ({} more)", errors.len() - 5);
        }
    }
}

/// JSON-Lines output: one session per line on stdout, parse errors to
/// stderr. Schema intentionally flat and stable — downstream eval
/// pipelines can rely on these field names.
#[derive(serde::Serialize)]
struct SessionLine {
    path: String,
    format: String,
    step_count: usize,
    tokens_in: u64,
    tokens_out: u64,
    cache_read: u64,
    cache_create: u64,
    cost_usd: Option<f64>,
    models: Vec<String>,
    tool_counts: Vec<ToolLine>,
    error_count: usize,
    annotation_count: usize,
    fork_root_count: usize,
    mtime_secs: Option<u64>,
}

#[derive(serde::Serialize)]
struct ToolLine {
    name: String,
    use_count: usize,
    error_count: usize,
}

fn session_to_line(s: &ParsedSession) -> SessionLine {
    SessionLine {
        path: s.path.display().to_string(),
        format: s.format.to_string(),
        step_count: s.step_count,
        tokens_in: s.totals.tokens_in,
        tokens_out: s.totals.tokens_out,
        cache_read: s.totals.cache_read,
        cache_create: s.totals.cache_create,
        cost_usd: s.totals.cost_usd,
        models: s.totals.unique_models.clone(),
        tool_counts: s
            .tool_stats
            .iter()
            .map(|t| ToolLine {
                name: t.name.clone(),
                use_count: t.use_count,
                error_count: t.error_count,
            })
            .collect(),
        error_count: s.tool_stats.iter().map(|t| t.error_count).sum(),
        annotation_count: s.annotation_count,
        fork_root_count: s.fork_root_count,
        mtime_secs: s.mtime_secs,
    }
}

fn print_jsonl(parsed: &[ParsedSession], errors: &[ParseError]) {
    // Sessions on stdout, one line each — compact (not pretty) so
    // downstream `jq -c` / `xargs` consumers see line-delimited JSON.
    for session in parsed {
        let line = session_to_line(session);
        match serde_json::to_string(&line) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("agx: failed to serialize session line: {e}"),
        }
    }
    // Parse errors on stderr so they don't corrupt the stdout stream
    // that consumers are piping into jq / xargs / a file.
    for err in errors {
        eprintln!("agx: parse error: {}: {}", err.path.display(), err.error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{
        ToolStats, assistant_text_step, tool_result_step, tool_use_step, user_text_step,
    };

    fn mk_session(path: &str, fmt: Format, steps: Vec<Step>) -> ParsedSession {
        let totals = compute_session_totals(&steps);
        let tool_stats = compute_tool_stats(&steps);
        ParsedSession {
            path: PathBuf::from(path),
            format: fmt,
            step_count: steps.len(),
            totals,
            tool_stats,
            mtime_secs: None,
            annotation_count: 0,
            fork_root_count: 0,
        }
    }

    fn priced_session(model: &str) -> Vec<Step> {
        let mut a = assistant_text_step("hi");
        a.model = Some(model.into());
        a.tokens_in = Some(100);
        a.tokens_out = Some(50);
        vec![user_text_step("q"), a]
    }

    #[test]
    fn filter_parse_accepts_all_forms() {
        assert_eq!(
            Filter::parse("model=claude-opus-4-6").unwrap(),
            Filter::Model("claude-opus-4-6".into())
        );
        assert_eq!(
            Filter::parse("tool=Bash").unwrap(),
            Filter::Tool("Bash".into())
        );
        assert_eq!(Filter::parse("errored").unwrap(), Filter::Errored);
        assert_eq!(Filter::parse("  errored  ").unwrap(), Filter::Errored);
        assert_eq!(
            Filter::parse("  model = gpt-5  ").unwrap(),
            Filter::Model("gpt-5".into())
        );
    }

    #[test]
    fn filter_parse_rejects_unknown_key() {
        assert!(Filter::parse("foo=bar").is_err());
    }

    #[test]
    fn filter_parse_rejects_bare_word() {
        assert!(Filter::parse("not-a-thing").is_err());
    }

    #[test]
    fn filter_model_matches_session_with_that_model() {
        let s = mk_session("a", Format::ClaudeCode, priced_session("claude-opus-4-6"));
        assert!(Filter::Model("claude-opus-4-6".into()).matches(&s));
        assert!(!Filter::Model("gpt-5".into()).matches(&s));
    }

    #[test]
    fn filter_tool_matches_case_insensitive() {
        let steps = vec![
            user_text_step("q"),
            tool_use_step("t1", "Bash", "{}"),
            tool_result_step("t1", "ok", Some("Bash"), Some("{}")),
        ];
        let s = mk_session("a", Format::ClaudeCode, steps);
        assert!(Filter::Tool("Bash".into()).matches(&s));
        assert!(Filter::Tool("bash".into()).matches(&s));
        assert!(!Filter::Tool("Write".into()).matches(&s));
    }

    #[test]
    fn filter_errored_matches_session_with_error_result() {
        let steps = vec![
            tool_use_step("t1", "Bash", "{}"),
            tool_result_step("t1", "error: command failed", Some("Bash"), Some("{}")),
        ];
        let s = mk_session("a", Format::ClaudeCode, steps);
        assert!(Filter::Errored.matches(&s));
    }

    #[test]
    fn filter_errored_does_not_match_clean_session() {
        let steps = vec![
            tool_use_step("t1", "Bash", "{}"),
            tool_result_step("t1", "success", Some("Bash"), Some("{}")),
        ];
        let s = mk_session("a", Format::ClaudeCode, steps);
        assert!(!Filter::Errored.matches(&s));
    }

    #[test]
    fn aggregate_sums_tokens_across_sessions() {
        let sessions = vec![
            mk_session("a", Format::ClaudeCode, priced_session("claude-opus-4-6")),
            mk_session("b", Format::Codex, priced_session("gpt-5")),
        ];
        let stats = aggregate(&sessions, &[], 2, 0);
        assert_eq!(stats.parse_success_count, 2);
        assert_eq!(stats.total_tokens_in, 200);
        assert_eq!(stats.total_tokens_out, 100);
        assert!(stats.total_cost_usd.is_some());
        // Two formats, two models, no tools.
        assert_eq!(stats.per_format.len(), 2);
        assert_eq!(stats.per_model.len(), 2);
        assert!(stats.per_tool.is_empty());
    }

    #[test]
    fn aggregate_per_model_sorts_by_session_count_desc() {
        let sessions = vec![
            mk_session("a", Format::ClaudeCode, priced_session("gpt-5")),
            mk_session("b", Format::ClaudeCode, priced_session("gpt-5")),
            mk_session("c", Format::ClaudeCode, priced_session("claude-opus-4-6")),
        ];
        let stats = aggregate(&sessions, &[], 3, 0);
        assert_eq!(stats.per_model[0].model, "gpt-5");
        assert_eq!(stats.per_model[0].session_count, 2);
        assert_eq!(stats.per_model[1].model, "claude-opus-4-6");
    }

    #[test]
    fn aggregate_per_tool_sums_use_and_error_counts() {
        let s1 = mk_session(
            "a",
            Format::ClaudeCode,
            vec![
                tool_use_step("t1", "Bash", "{}"),
                tool_result_step("t1", "ok", Some("Bash"), Some("{}")),
            ],
        );
        let s2 = mk_session(
            "b",
            Format::ClaudeCode,
            vec![
                tool_use_step("t2", "Bash", "{}"),
                tool_result_step("t2", "error: failed", Some("Bash"), Some("{}")),
            ],
        );
        let stats = aggregate(&[s1, s2], &[], 2, 0);
        assert_eq!(stats.per_tool.len(), 1);
        assert_eq!(stats.per_tool[0].tool, "Bash");
        assert_eq!(stats.per_tool[0].use_count, 2);
        assert_eq!(stats.per_tool[0].error_count, 1);
    }

    #[test]
    fn aggregate_empty_input_returns_zeros() {
        let stats = aggregate(&[], &[], 0, 0);
        assert_eq!(stats.parse_success_count, 0);
        assert_eq!(stats.total_tokens_in, 0);
        assert_eq!(stats.total_cost_usd, None);
        assert!(stats.per_model.is_empty());
        assert!(stats.per_tool.is_empty());
    }

    #[test]
    fn aggregate_counts_filtered_and_errored() {
        let sessions = vec![mk_session("a", Format::ClaudeCode, priced_session("gpt-5"))];
        let errors = vec![ParseError {
            path: PathBuf::from("bad.jsonl"),
            error: anyhow!("format drift"),
        }];
        let stats = aggregate(&sessions, &errors, 5, 3);
        assert_eq!(stats.file_count, 5);
        assert_eq!(stats.parse_success_count, 1);
        assert_eq!(stats.parse_error_count, 1);
        assert_eq!(stats.filtered_out_count, 3);
    }

    #[test]
    fn tool_bucket_ordering_is_stable_on_ties() {
        // Equal use_count → alphabetic tie-break.
        let sessions = vec![
            mk_session(
                "a",
                Format::ClaudeCode,
                vec![tool_use_step("t1", "Zebra", "{}")],
            ),
            mk_session(
                "b",
                Format::ClaudeCode,
                vec![tool_use_step("t2", "Apple", "{}")],
            ),
        ];
        let stats = aggregate(&sessions, &[], 2, 0);
        assert_eq!(stats.per_tool[0].tool, "Apple");
        assert_eq!(stats.per_tool[1].tool, "Zebra");
    }

    #[test]
    fn unused_tool_stats_type_reference() {
        // Sanity: the ToolStats type is in scope so future tests can
        // construct one directly if needed. This test just compiles.
        let _ = ToolStats {
            name: "x".into(),
            use_count: 0,
            result_count: 0,
            error_count: 0,
        };
    }

    // -------- Phase 6.2 trajectory stats --------

    #[test]
    fn distribution_empty_slice_is_all_zero() {
        let d = Distribution::from_values(&[]);
        assert_eq!(d.min, 0);
        assert_eq!(d.max, 0);
        assert_eq!(d.mean, 0.0);
        assert_eq!(d.total, 0);
    }

    #[test]
    fn distribution_single_value() {
        let d = Distribution::from_values(&[42]);
        assert_eq!(d.min, 42);
        assert_eq!(d.p50, 42);
        assert_eq!(d.p90, 42);
        assert_eq!(d.p99, 42);
        assert_eq!(d.max, 42);
        assert!((d.mean - 42.0).abs() < 1e-6);
        assert_eq!(d.total, 42);
    }

    #[test]
    fn distribution_percentiles_on_ordered_integers() {
        // 1..=100 — p50 should be 50, p90 ≈ 90, p99 ≈ 99, max 100.
        let values: Vec<u64> = (1..=100).collect();
        let d = Distribution::from_values(&values);
        assert_eq!(d.min, 1);
        assert_eq!(d.max, 100);
        assert_eq!(d.p50, 50);
        assert_eq!(d.p90, 90);
        assert_eq!(d.p99, 99);
        assert_eq!(d.total, 5050);
        assert!((d.mean - 50.5).abs() < 1e-6);
    }

    #[test]
    fn distribution_handles_unsorted_input() {
        // Input order doesn't matter — the constructor sorts internally.
        let a = Distribution::from_values(&[5, 1, 3, 2, 4]);
        let b = Distribution::from_values(&[1, 2, 3, 4, 5]);
        assert_eq!(a.min, b.min);
        assert_eq!(a.max, b.max);
        assert_eq!(a.p50, b.p50);
        assert_eq!(a.total, b.total);
    }

    #[test]
    fn trajectory_stats_empty_corpus() {
        let stats = compute_trajectory_stats(&[]);
        assert_eq!(stats.session_count, 0);
        assert_eq!(stats.branched_rate, 0.0);
        assert_eq!(stats.annotated_rate, 0.0);
        assert_eq!(stats.errored_rate, 0.0);
    }

    #[test]
    fn trajectory_stats_branched_rate_counts_fork_roots() {
        let a = {
            let mut s = mk_session("a.jsonl", Format::ClaudeCode, Vec::new());
            s.fork_root_count = 2;
            s
        };
        let b = mk_session("b.jsonl", Format::ClaudeCode, Vec::new());
        let c = {
            let mut s = mk_session("c.jsonl", Format::Gemini, Vec::new());
            s.fork_root_count = 1;
            s
        };
        let stats = compute_trajectory_stats(&[a, b, c]);
        assert_eq!(stats.session_count, 3);
        // 2 of 3 sessions have forks.
        assert!((stats.branched_rate - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn trajectory_stats_annotated_rate_counts_annotation_count() {
        let mut a = mk_session("a", Format::ClaudeCode, Vec::new());
        a.annotation_count = 3;
        let b = mk_session("b", Format::ClaudeCode, Vec::new());
        let stats = compute_trajectory_stats(&[a, b]);
        assert!((stats.annotated_rate - 0.5).abs() < 1e-6);
    }

    #[test]
    fn trajectory_stats_errored_rate_counts_sessions_not_errors() {
        // A session with 5 errors still counts as 1 errored session;
        // the rate is session-level, not error-level.
        let a = mk_session(
            "a",
            Format::ClaudeCode,
            vec![crate::timeline::tool_result_step(
                "t1",
                "error: bad",
                Some("X"),
                None,
            )],
        );
        // A clean session.
        let b = mk_session("b", Format::ClaudeCode, Vec::new());
        let stats = compute_trajectory_stats(&[a, b]);
        assert!((stats.errored_rate - 0.5).abs() < 1e-6);
    }

    #[test]
    fn trajectory_stats_steps_distribution_reflects_step_counts() {
        let a = mk_session(
            "a",
            Format::ClaudeCode,
            vec![
                crate::timeline::user_text_step("one"),
                crate::timeline::assistant_text_step("two"),
            ],
        );
        let b = mk_session(
            "b",
            Format::ClaudeCode,
            vec![crate::timeline::user_text_step("solo")],
        );
        let stats = compute_trajectory_stats(&[a, b]);
        assert_eq!(stats.steps_per_session.min, 1);
        assert_eq!(stats.steps_per_session.max, 2);
        assert_eq!(stats.steps_per_session.total, 3);
    }
}
