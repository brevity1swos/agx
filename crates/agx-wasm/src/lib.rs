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

use agx_core::timeline::Step;
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

/// Detect format + parse from an in-memory buffer. On native targets
/// this goes through a secure tempfile (`tempfile::NamedTempFile` —
/// O_EXCL creation, random name, auto-cleanup on Drop). On wasm32
/// targets the filesystem isn't available, so we return a clear
/// error directing the caller at the out-of-band workflow.
///
/// The native path is primarily used for `cargo test` and
/// `cargo check` of this crate; real browser / Node usage hits the
/// wasm32 branch. A follow-up will add a bytes-first parser entry
/// point in `agx-core::loader` so the wasm32 branch can actually
/// parse instead of erroring.
#[cfg(not(target_arch = "wasm32"))]
fn load_from_bytes(bytes: &[u8]) -> anyhow::Result<Vec<Step>> {
    use std::io::Write as _;
    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    // `NamedTempFile::path` gives us a stable path for the loader;
    // the file is removed automatically when `tmp` drops (including
    // on error paths).
    agx_core::loader::load_session(tmp.path())
}

#[cfg(target_arch = "wasm32")]
fn load_from_bytes(_bytes: &[u8]) -> anyhow::Result<Vec<Step>> {
    // wasm32-unknown-unknown has no writable filesystem, and
    // agx-core's parsers currently expect a path. Tracked as the
    // 7.3 follow-up: add `agx_core::loader::load_bytes(&[u8])` with
    // bytes-first parser entry points, then route this call into
    // it. Until then, this target returns a clear error instead of
    // silently panicking inside a `std::fs::create` call.
    anyhow::bail!(
        "agx-wasm on wasm32 does not yet support `load(bytes)` — the parsers need a filesystem path. \
         Use `scan_pii` / `version` for now, or drive agx via its CLI / MCP server."
    )
}
