//! Session-loading front door. Takes a path, detects the format, calls
//! the right parser, returns `Vec<Step>`. Lives in its own module so both
//! the single-session TUI/summary path (in `main.rs`) and the corpus
//! subcommand (in `corpus.rs`) dispatch through the same function.

use crate::format::{self, Format};
use crate::timeline::Step;
use crate::{
    codex, gemini, generic, langchain, otel_json, otel_proto, session, timeline, vercel_ai,
};
use anyhow::Result;
use std::path::Path;

/// Detect the format of a session file and load it into a timeline of
/// steps. This is the canonical entry point — every code path that
/// consumes a single session should go through here so new formats only
/// need to be registered in one place (here + [`Format`] + [`format::detect`]).
pub fn load_session(path: &Path) -> Result<Vec<Step>> {
    let fmt = format::detect(path)?;
    let steps = match fmt {
        Format::ClaudeCode => {
            let entries = session::load(path)?;
            timeline::build(&entries)
        }
        Format::Codex => codex::load(path)?,
        Format::Gemini => gemini::load(path)?,
        Format::Generic => generic::load(path)?,
        Format::Langchain => langchain::load(path)?,
        Format::OtelJson => otel_json::load(path)?,
        Format::OtelProto => otel_proto::load(path)?,
        Format::VercelAi => vercel_ai::load(path)?,
    };
    Ok(steps)
}
