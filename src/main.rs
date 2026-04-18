mod browser;
mod codex;
mod corpus;
mod debug_unknowns;
mod export;
mod format;
mod gemini;
mod generic;
mod langchain;
mod loader;
mod otel_json;
mod otel_proto;
mod pricing;
mod session;
mod timeline;
mod tui;
mod vercel_ai;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use loader::load_session;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use timeline::{Step, compute_session_totals, compute_tool_stats, count_from_steps};

#[derive(Copy, Clone, Debug, ValueEnum)]
enum ExportFormat {
    Md,
    Html,
    Json,
}

#[derive(Parser, Debug)]
#[command(name = "agx", version, about = "Step-through debugger for your agent")]
struct Cli {
    /// Path to a session file (Claude Code JSONL, Codex CLI JSONL, or Gemini CLI JSON).
    /// Omit to browse recent sessions from ~/.claude, ~/.codex, and ~/.gemini.
    session: Option<PathBuf>,

    /// Print a summary of the parsed timeline and exit (no TUI)
    #[arg(long)]
    summary: bool,

    /// Compare two sessions side-by-side (text summary)
    #[arg(long)]
    diff: Option<PathBuf>,

    /// Live mode: watch for file changes and auto-refresh
    #[arg(long)]
    live: bool,

    /// Generate shell completions and print to stdout
    #[arg(long, value_name = "SHELL")]
    completions: Option<Shell>,

    /// Scan the session for entry types or fields the parser doesn't recognize
    /// and print a report to stderr. Useful for diagnosing format drift.
    #[arg(long)]
    debug_unknowns: bool,

    /// Suppress cost estimates in --summary, stats overlay, and TUI status
    /// bar. Token counts are still shown. Use when working with unpriced
    /// custom models or when cost estimates are noise.
    #[arg(long)]
    no_cost: bool,

    /// Export the session to stdout in the given format instead of
    /// launching the TUI. Mutually exclusive with --summary.
    #[arg(long, value_enum, value_name = "FORMAT")]
    export: Option<ExportFormat>,

    /// Print load / parse / render timing breakdown to stderr. Hidden
    /// diagnostic flag for performance-regression reports.
    #[arg(long, hide = true)]
    bench: bool,

    /// Optional subcommand. When present, overrides the single-session
    /// flow. Today only `corpus` exists — it aggregates stats across
    /// every session file in a directory tree.
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan a directory tree, load every session in parallel, and print
    /// aggregate stats (per-model / per-tool / per-format breakdowns,
    /// totals, and cost). Unrecognized files are silently skipped.
    Corpus(CorpusArgs),
}

#[derive(clap::Args, Debug)]
struct CorpusArgs {
    /// Directory to scan. Walks recursively up to `--max-depth`.
    dir: PathBuf,

    /// Keep only sessions matching ALL of the given filters. Value shapes:
    /// `model=<name>`, `tool=<name>`, or the bare keyword `errored`.
    /// Can be repeated.
    #[arg(long, value_name = "FILTER")]
    filter: Vec<String>,

    /// Emit the aggregate stats as JSON instead of a human-readable summary.
    #[arg(long)]
    json: bool,

    /// Suppress cost estimates (mirror of the top-level --no-cost flag).
    #[arg(long)]
    no_cost: bool,

    /// Maximum directory-tree depth to walk. Default is 8, enough for
    /// every format's canonical storage layout (Claude Code's
    /// `~/.claude/projects/<encoded>/<uuid>.jsonl` sits at depth 4 from
    /// `~/.claude`, Codex at depth 5 from `~/.codex`, etc.).
    #[arg(long, default_value_t = 8)]
    max_depth: usize,

    /// Print walk / load / aggregate timing breakdown to stderr.
    #[arg(long, hide = true)]
    bench: bool,
}

// `load_session` itself lives in src/loader.rs so both the single-session
// flow below and the corpus subcommand dispatch through the same entry.

fn print_diff(path_a: &Path, steps_a: &[Step], path_b: &Path, steps_b: &[Step]) {
    let fmt_a = format::detect(path_a).map_or_else(|_| "?".into(), |f| f.to_string());
    let fmt_b = format::detect(path_b).map_or_else(|_| "?".into(), |f| f.to_string());
    let counts_a = count_from_steps(steps_a);
    let counts_b = count_from_steps(steps_b);
    let stats_a = compute_tool_stats(steps_a);
    let stats_b = compute_tool_stats(steps_b);

    println!("agx diff\n");
    println!(
        "  {:<40} {:<40}",
        format!("A: {} ({})", fmt_a, path_a.display()),
        format!("B: {} ({})", fmt_b, path_b.display())
    );
    println!();
    println!(
        "  {:<40} {:<40}",
        format!("Steps: {}", steps_a.len()),
        format!("Steps: {}", steps_b.len())
    );
    println!(
        "  {:<40} {:<40}",
        format!(
            "user:{} asst:{} tool:{} result:{}",
            counts_a.user, counts_a.assistant, counts_a.tool_uses, counts_a.tool_results
        ),
        format!(
            "user:{} asst:{} tool:{} result:{}",
            counts_b.user, counts_b.assistant, counts_b.tool_uses, counts_b.tool_results
        ),
    );
    println!();

    let names_a: HashSet<String> = stats_a.iter().map(|s| s.name.clone()).collect();
    let names_b: HashSet<String> = stats_b.iter().map(|s| s.name.clone()).collect();

    let both: Vec<&String> = names_a.intersection(&names_b).collect();
    println!("  Tools in both ({}):", both.len());
    for name in &both {
        let a = stats_a.iter().find(|s| &s.name == *name).unwrap();
        let b = stats_b.iter().find(|s| &s.name == *name).unwrap();
        #[allow(clippy::cast_possible_wrap)]
        let delta = b.use_count as i64 - a.use_count as i64;
        let sign = if delta >= 0 { "+" } else { "" };
        println!(
            "    {:<20} A:{:<4} B:{:<4} ({sign}{delta})",
            name, a.use_count, b.use_count
        );
    }
    let only_a: Vec<&String> = names_a.difference(&names_b).collect();
    let only_b: Vec<&String> = names_b.difference(&names_a).collect();
    if !only_a.is_empty() {
        let list: Vec<&str> = only_a.iter().map(|s| s.as_str()).collect();
        println!("  Tools only in A: {}", list.join(", "));
    }
    if !only_b.is_empty() {
        let list: Vec<&str> = only_b.iter().map(|s| s.as_str()).collect();
        println!("  Tools only in B: {}", list.join(", "));
    }

    let errors_a: usize = stats_a.iter().map(|s| s.error_count).sum();
    let errors_b: usize = stats_b.iter().map(|s| s.error_count).sum();
    println!();
    println!(
        "  {:<40} {:<40}",
        format!("Errors: {errors_a}"),
        format!("Errors: {errors_b}")
    );
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, "agx", &mut std::io::stdout());
        return Ok(());
    }

    // `agx corpus <dir>` subcommand takes over before the single-session
    // flow when the user asks for corpus-level analytics.
    if let Some(Commands::Corpus(args)) = cli.command {
        let filters = args
            .filter
            .iter()
            .map(|s| corpus::Filter::parse(s))
            .collect::<Result<Vec<_>>>()?;
        let corpus_args = corpus::CorpusArgs {
            dir: args.dir,
            filters,
            json: args.json,
            no_cost: args.no_cost,
            max_depth: args.max_depth,
            bench: args.bench,
        };
        return corpus::run(&corpus_args);
    }

    let session_path = if let Some(p) = cli.session {
        p
    } else if cli.diff.is_some() {
        return Err(anyhow::anyhow!(
            "--diff requires a session path as the first argument"
        ));
    } else {
        let files = browser::discover_all();
        match browser::prompt_user_to_choose(&files)? {
            Some(p) => p,
            None => return Ok(()),
        }
    };

    if cli.debug_unknowns {
        let fmt = format::detect(&session_path)?;
        let report = debug_unknowns::scan(fmt, &session_path)?;
        report.print(&mut std::io::stderr())?;
    }

    // Bench timing wraps the whole load path so users filing perf issues
    // can attach a concrete number. Writes to stderr so stdout stays
    // pipeable for --summary / --export.
    let load_start = std::time::Instant::now();
    let steps = load_session(&session_path)?;
    if cli.bench {
        eprintln!(
            "[bench] load: {:.2}ms ({} steps)",
            load_start.elapsed().as_secs_f64() * 1000.0,
            steps.len()
        );
    }

    if let Some(diff_path) = &cli.diff {
        let steps_b = load_session(diff_path)?;
        print_diff(&session_path, &steps, diff_path, &steps_b);
        return Ok(());
    }

    if let Some(fmt) = cli.export {
        let totals = compute_session_totals(&steps);
        let out = match fmt {
            ExportFormat::Json => export::json(&steps, &totals)?,
            ExportFormat::Md => export::markdown(&steps, &totals, cli.no_cost),
            ExportFormat::Html => export::html(&steps, &totals, cli.no_cost),
        };
        print!("{out}");
        return Ok(());
    }

    if cli.summary {
        let fmt = format::detect(&session_path)?;
        let counts = count_from_steps(&steps);
        let totals = compute_session_totals(&steps);
        println!("Loaded {} session from {}", fmt, session_path.display());
        println!(
            "  {} timeline steps: {} user, {} assistant, {} tool_uses, {} tool_results",
            steps.len(),
            counts.user,
            counts.assistant,
            counts.tool_uses,
            counts.tool_results
        );
        if totals.has_tokens() {
            println!(
                "  {} input tokens, {} output, {} cache_read, {} cache_create",
                totals.tokens_in, totals.tokens_out, totals.cache_read, totals.cache_create
            );
        }
        if !totals.unique_models.is_empty() {
            println!("  models: {}", totals.unique_models.join(", "));
        }
        if !cli.no_cost {
            match totals.cost_usd {
                Some(c) => println!("  estimated cost: ${c:.4} USD"),
                None if totals.has_tokens() => {
                    println!("  estimated cost: (unknown — no pricing entry for model)")
                }
                None => {}
            }
        }
        println!("First 20:");
        for (i, step) in steps.iter().take(20).enumerate() {
            println!("  {:>3}  {}", i + 1, step.label);
        }
        return Ok(());
    }

    let reload_fn: Option<Box<dyn Fn() -> Result<Vec<Step>>>> = if cli.live {
        let path = session_path.clone();
        Some(Box::new(move || load_session(&path)))
    } else {
        None
    };
    tui::run(steps, reload_fn.as_deref(), cli.no_cost)?;
    Ok(())
}
