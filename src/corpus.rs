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
}

impl Filter {
    /// Parse one `--filter` value. Accepts `model=X`, `tool=X`, or the
    /// bare keyword `errored`.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("errored") {
            return Ok(Filter::Errored);
        }
        let (key, value) = s
            .split_once('=')
            .ok_or_else(|| anyhow!("--filter expects `key=value` or `errored`, got `{s}`"))?;
        match key.trim() {
            "model" => Ok(Filter::Model(value.trim().to_string())),
            "tool" => Ok(Filter::Tool(value.trim().to_string())),
            other => Err(anyhow!(
                "unknown --filter key `{other}` (expected `model`, `tool`, or `errored`)"
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
/// Per-path load result — kept as a type alias so clippy's
/// `type_complexity` lint doesn't fire on the collect site below.
type RawLoad = (PathBuf, Result<(Format, Vec<Step>)>);

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
            Ok((fmt, steps)) => {
                let totals = compute_session_totals(&steps);
                let tool_stats = compute_tool_stats(&steps);
                let mtime_secs = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                parsed.push(ParsedSession {
                    path,
                    format: fmt,
                    totals,
                    tool_stats,
                    step_count: steps.len(),
                    mtime_secs,
                });
            }
            // Silent drop: file wasn't a recognized session at all. We
            // use a sentinel error kind (AGX_SKIP) to distinguish this
            // from real parse failures.
            Err(e) if format!("{e:?}").contains("AGX_SKIP") => {}
            Err(e) => errors.push(ParseError { path, error: e }),
        }
    }
    (parsed, errors)
}

fn load_one(path: &Path) -> Result<(Format, Vec<Step>)> {
    // Detection failure → silent skip. Detection succeeds → attempt
    // parse and surface any failure as a real error.
    let fmt = match format::detect(path) {
        Ok(f) => f,
        Err(_) => return Err(anyhow!("AGX_SKIP: not a recognized session file")),
    };
    match load_session(path) {
        Ok(steps) => Ok((fmt, steps)),
        Err(e) => {
            // Non-UTF-8 files route to OtelProto at detection time, but if
            // the `otel-proto` feature is off (the default) load fails with
            // a rebuild-with-feature error. For corpus mode, those files
            // are almost certainly unrelated binaries (images, PDFs,
            // archives) rather than real OTLP protobuf exports the user
            // forgot to compile support for. Silently skip instead of
            // spamming the "rebuild with --features" message across
            // every image in a directory tree.
            if fmt == Format::OtelProto && format!("{e:#}").contains("--features otel-proto") {
                return Err(anyhow!(
                    "AGX_SKIP: binary file, otel-proto feature disabled"
                ));
            }
            Err(e)
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
    let stats = aggregate(&parsed, &errors, file_count, filtered_out);
    let agg_ms = t_agg.elapsed().as_secs_f64() * 1000.0;

    if args.tui {
        // Drop into the interactive corpus TUI. When the user selects a
        // session and hits Enter, the outer loop re-runs the TUI after
        // the drill-in per-session TUI exits.
        crate::corpus_tui::run(parsed, &stats, args.no_cost)?;
    } else if args.json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
    } else {
        print_text_summary(&stats, &args.dir, args.no_cost, &errors);
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
}
