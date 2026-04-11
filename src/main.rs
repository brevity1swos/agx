mod codex;
mod format;
mod gemini;
mod session;
mod timeline;
mod tui;

use anyhow::Result;
use clap::Parser;
use format::Format;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "agx",
    version,
    about = "Step-through debugger for AI agent execution traces"
)]
struct Cli {
    /// Path to a session file (Claude Code JSONL, Codex CLI JSONL, or Gemini CLI JSON)
    session: PathBuf,

    /// Print a summary of the parsed timeline and exit (no TUI)
    #[arg(long)]
    summary: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let fmt = format::detect(&cli.session)?;
    let steps = match fmt {
        Format::ClaudeCode => {
            let entries = session::load(&cli.session)?;
            timeline::build(&entries)
        }
        Format::Codex => codex::load(&cli.session)?,
        Format::Gemini => gemini::load(&cli.session)?,
    };
    let counts = timeline::count_from_steps(&steps);

    if cli.summary {
        println!("Loaded {} session from {}", fmt, cli.session.display());
        println!(
            "  {} timeline steps: {} user, {} assistant, {} tool_uses, {} tool_results",
            steps.len(),
            counts.user,
            counts.assistant,
            counts.tool_uses,
            counts.tool_results
        );
        println!("First 20:");
        for (i, step) in steps.iter().take(20).enumerate() {
            println!("  {:>3}  {}", i + 1, step.label);
        }
        return Ok(());
    }

    tui::run(steps)?;
    Ok(())
}
