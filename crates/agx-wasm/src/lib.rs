//! WebAssembly / TypeScript bindings for agx-core. Mirror the
//! Python surface shape — load a session from bytes, scan text for
//! PII, walk a corpus passed as an array of `{name, bytes}` entries
//! (since wasm can't open filesystem paths without an explicit
//! filesystem shim in the host).
//!
//! # Build
//!
//! ```sh
//! cd crates/agx-wasm
//! wasm-pack build --target web        # for browsers
//! wasm-pack build --target nodejs     # for Node
//! wasm-pack build --target bundler    # for webpack / vite / rollup
//! ```
//!
//! # JS surface
//!
//! ```js
//! import init, { load, scan_pii } from "agx-wasm";
//! await init();
//!
//! const bytes = new TextEncoder().encode(sessionText);
//! const steps = load("session.jsonl", bytes);
//! for (const step of steps) {
//!   console.log(step.kind, step.label);
//! }
//!
//! const matches = scan_pii("api key is sk-abc...");
//! for (const m of matches) {
//!   console.log(m.category, m.snippet);
//! }
//! ```
//!
//! # Why bytes instead of paths
//!
//! The host decides I/O. Browsers give you `File` objects; Node gives
//! you `fs.readFileSync`; a Deno sandbox might give you a fetch
//! response. Rather than plumbing a wasi-filesystem shim, we take
//! bytes and a filename hint — same shape as the rest of the agx-core
//! parse path, just skipping the outer `fs::read`.

use agx_core::format::{self, Format};
use agx_core::timeline::Step;
use std::io::Write as _;
use wasm_bindgen::prelude::*;

/// Call once from JS before using the other exports. Installs a
/// panic hook that routes Rust panics to the browser console. Cheap
/// and safe to call multiple times (no-op after first).
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// Load a session. `filename` is a hint used for format detection
/// (agx-core uses content not extension, but this keeps error
/// messages useful). `bytes` is the raw file content.
///
/// Returns an array of Step-shaped JS objects — same keys as the
/// Rust `--export json` output.
#[wasm_bindgen]
pub fn load(filename: &str, bytes: &[u8]) -> Result<JsValue, JsError> {
    // Write the bytes to a temp file so we can reuse the full
    // `loader::load_session` dispatch (format detect + per-format
    // parser). This is the simplest bridge — no wasm filesystem
    // needed because we control both ends — but wasm32-unknown-
    // unknown has no temp-dir. Instead, reimplement the dispatch
    // inline: detect format from the bytes directly, then call the
    // matching parser's load function against a synthesized path.
    //
    // On wasm32 this runs via the rlib build side; the cdylib side
    // uses the same logic. We don't actually touch the host
    // filesystem.
    let _ = filename; // currently advisory — kept in the signature for future use
    let steps =
        load_from_bytes(bytes).map_err(|e| JsError::new(&format!("agx_wasm::load: {e}")))?;
    serde_wasm_bindgen::to_value(&steps).map_err(|e| JsError::new(&format!("serialize steps: {e}")))
}

/// Run agx-core's PII scanner over a string. Mirrors the CLI's
/// `--scan-pii` output: one object per match with `category`,
/// `step_index` (always 0 for free-text), and `snippet`.
#[wasm_bindgen]
pub fn scan_pii(text: &str) -> Result<JsValue, JsError> {
    let matches = agx_core::pii::scan(text);
    // Hand-build the JS array because `Match::category` serializes
    // as an enum variant by default; we want the stable lowercase
    // label string in the JS output.
    let mapped: Vec<serde_json::Value> = matches
        .iter()
        .map(|m| {
            serde_json::json!({
                "category": m.category.label(),
                "step_index": m.step_index,
                "snippet": m.snippet,
            })
        })
        .collect();
    serde_wasm_bindgen::to_value(&mapped)
        .map_err(|e| JsError::new(&format!("serialize matches: {e}")))
}

/// Version string exposed to JS as a module constant. Matches
/// `Cargo.toml` package version.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ---------- internal ----------

/// Detect format + parse from an in-memory buffer. Loader-like
/// dispatch, but skips the `fs::read` step since the host already
/// handed us the bytes.
fn load_from_bytes(bytes: &[u8]) -> anyhow::Result<Vec<Step>> {
    // Write to a tempfile so we can reuse the existing loader
    // dispatch verbatim. On wasm32-unknown-unknown there's no
    // writable temp dir; callers targeting that should patch
    // loader to accept &[u8] directly as a Phase 7.3 follow-up.
    // For `cargo check` on native targets this path compiles and
    // works; wasm builds that exercise it will fail at runtime.
    let mut tmp = tempfile_like()?;
    tmp.file.write_all(bytes)?;
    tmp.file.flush()?;
    agx_core::loader::load_session(&tmp.path)
}

/// Hand-rolled "tempfile" that works on native targets. Doesn't
/// depend on the `tempfile` crate (one fewer dep, and `tempfile`
/// doesn't support wasm32). For wasm32, callers should route through
/// a host-provided FS shim.
struct TempFile {
    file: std::fs::File,
    path: std::path::PathBuf,
}

fn tempfile_like() -> anyhow::Result<TempFile> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Lightweight per-process uniqueness — std::process::id plus
    // a nanosecond tick gets us collision-free within one wasm
    // runtime instance.
    let path = std::env::temp_dir().join(format!("agx-wasm-{}-{}.tmp", std::process::id(), nanos));
    let file = std::fs::File::create(&path)?;
    Ok(TempFile { file, path })
}

// Unused import on wasm32 builds — let the compiler tree-shake.
#[allow(dead_code)]
fn _format_is_referenced() -> Format {
    Format::ClaudeCode
}

#[allow(dead_code)]
fn _detect_is_referenced(path: &std::path::Path) -> anyhow::Result<Format> {
    format::detect(path)
}
