mod session;
mod timeline;
mod tui;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "agx",
    version,
    about = "Step-through debugger for AI agent execution traces"
)]
struct Cli {
    /// Path to a session JSONL file (Claude Code session format)
    session: PathBuf,

    /// Print a summary of the parsed timeline and exit (no TUI)
    #[arg(long)]
    summary: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let entries = session::load(&cli.session)?;
    let counts = session::count(&entries);
    let steps = timeline::build(&entries);

    if cli.summary {
        println!(
            "Loaded {} entries from {}",
            entries.len(),
            cli.session.display()
        );
        println!(
            "  user: {}  assistant: {}  other: {}  tool_uses: {}  tool_results: {}",
            counts.user, counts.assistant, counts.other, counts.tool_uses, counts.tool_results
        );
        println!("Built {} timeline steps. First 20:", steps.len());
        for (i, step) in steps.iter().take(20).enumerate() {
            println!("  {:>3}  {}", i + 1, step.label);
        }
        return Ok(());
    }

    tui::run(steps)?;
    Ok(())
}
