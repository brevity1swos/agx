//! Python bindings for `agx-core`. Mirror the three most load-bearing
//! entry points: load a single session, scan a directory, run a PII
//! scan on a raw string.
//!
//! # Wheel build
//!
//! ```sh
//! cd crates/agx-py
//! maturin build --release
//! ```
//!
//! # Install (development)
//!
//! ```sh
//! cd crates/agx-py
//! maturin develop
//! python -c 'import agx; print(agx.__version__)'
//! ```
//!
//! # Python surface
//!
//! ```python
//! import agx
//!
//! steps = agx.load("session.jsonl")
//! for step in steps:
//!     print(step["kind"], step["label"])
//!
//! # Corpus scan — one (path, steps) per session, matches the Rust
//! # `load_parallel` shape but as a plain list of tuples for
//! # predictability in downstream dataframes.
//! for path, steps in agx.load_corpus("sessions/"):
//!     print(f"{path}: {len(steps)} steps")
//!
//! # PII scan — mirrors `agx --scan-pii` over a raw string.
//! matches = agx.scan_pii("my api key is sk-abc123…")
//! for m in matches:
//!     print(m["category"], m["snippet"])
//! ```
//!
//! # Shape
//!
//! The Python objects are plain `dict`s mapping the same field names
//! as the stable JSON schema (documented in `docs/eval-integration.md`).
//! This keeps the bridge simple and avoids pyclass wrappers that would
//! diverge from the JSON shape every time an agx-core field lands.

use pyo3::IntoPyObjectExt;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::path::PathBuf;

/// Convert a `serde_json::Value` to a Python object. Used to mirror the
/// stable JSON schema into Python dicts without hand-rolling a pyclass
/// for every agx-core type.
fn json_to_py(py: Python<'_>, v: &serde_json::Value) -> PyResult<Py<PyAny>> {
    match v {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(b.into_py_any(py)?),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_py_any(py)?)
            } else if let Some(u) = n.as_u64() {
                Ok(u.into_py_any(py)?)
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_py_any(py)?)
            } else {
                Ok(py.None())
            }
        }
        serde_json::Value::String(s) => Ok(s.clone().into_py_any(py)?),
        serde_json::Value::Array(arr) => {
            let out = PyList::empty(py);
            for item in arr {
                let py_item = json_to_py(py, item)?;
                out.append(py_item)?;
            }
            Ok(out.into())
        }
        serde_json::Value::Object(map) => {
            let out = PyDict::new(py);
            for (k, val) in map {
                let py_val = json_to_py(py, val)?;
                out.set_item(k, py_val)?;
            }
            Ok(out.into())
        }
    }
}

/// Serialize a `serde::Serialize` value → Python dict via the JSON
/// representation. One-hop conversion keeps the bridge schema honest:
/// whatever `--export json` shows, Python sees the same keys.
fn serialize_to_py<T: serde::Serialize>(py: Python<'_>, value: &T) -> PyResult<Py<PyAny>> {
    let v = serde_json::to_value(value)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    json_to_py(py, &v)
}

/// Load a single session. Auto-detects format. Returns a list of Step
/// dicts (see `docs/eval-integration.md` for the stable field names).
#[pyfunction]
fn load(py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
    let steps = agx_core::loader::load_session(&PathBuf::from(path))
        .map_err(|e| pyo3::exceptions::PyIOError::new_err(format!("{e}")))?;
    serialize_to_py(py, &steps)
}

/// Scan a directory of sessions and return one dict per session.
/// Parses in parallel via rayon; non-session files are silently
/// skipped. Files that LOOK like sessions but fail to parse come back
/// at the end as `{path, error}` dicts so downstream code can
/// distinguish "not a session" from "bad session."
///
/// Per-session dict shape:
/// `{path, format, step_count, totals: {tokens_in, tokens_out, …},
///   annotation_count, fork_root_count, mtime_secs, tool_stats: [{name, use_count, error_count}…]}`.
///
/// Steps aren't included here to keep corpus scans cheap; call
/// `agx.load(path)` per session when you actually need the steps.
#[pyfunction]
fn load_corpus(py: Python<'_>, dir: &str) -> PyResult<Py<PyAny>> {
    let paths = agx_core::corpus::discover_files(&PathBuf::from(dir), 8);
    let (parsed, errors) = agx_core::corpus::load_parallel(&paths);
    let out = PyList::empty(py);
    for session in &parsed {
        let d = PyDict::new(py);
        d.set_item("path", session.path.display().to_string())?;
        d.set_item("format", session.format.to_string())?;
        d.set_item("step_count", session.step_count)?;
        d.set_item("annotation_count", session.annotation_count)?;
        d.set_item("fork_root_count", session.fork_root_count)?;
        d.set_item("mtime_secs", session.mtime_secs)?;
        let totals = PyDict::new(py);
        totals.set_item("tokens_in", session.totals.tokens_in)?;
        totals.set_item("tokens_out", session.totals.tokens_out)?;
        totals.set_item("cache_read", session.totals.cache_read)?;
        totals.set_item("cache_create", session.totals.cache_create)?;
        totals.set_item("cost_usd", session.totals.cost_usd)?;
        totals.set_item("unique_models", session.totals.unique_models.clone())?;
        d.set_item("totals", totals)?;
        let tools = PyList::empty(py);
        for t in &session.tool_stats {
            let td = PyDict::new(py);
            td.set_item("name", t.name.clone())?;
            td.set_item("use_count", t.use_count)?;
            td.set_item("result_count", t.result_count)?;
            td.set_item("error_count", t.error_count)?;
            tools.append(td)?;
        }
        d.set_item("tool_stats", tools)?;
        out.append(d)?;
    }
    for err in &errors {
        let tup = PyDict::new(py);
        tup.set_item("path", err.path.display().to_string())?;
        tup.set_item("error", format!("{}", err.error))?;
        out.append(tup)?;
    }
    Ok(out.into())
}

/// Run the `--scan-pii` heuristic over arbitrary text. Returns a list
/// of `{category, step_index, snippet}` dicts; `step_index` is always
/// 0 when called on a free-text string.
#[pyfunction]
fn scan_pii(py: Python<'_>, text: &str) -> PyResult<Py<PyAny>> {
    let matches = agx_core::pii::scan(text);
    let out = PyList::empty(py);
    for m in &matches {
        let d = PyDict::new(py);
        d.set_item("category", m.category.label())?;
        d.set_item("step_index", m.step_index)?;
        d.set_item("snippet", m.snippet.clone())?;
        out.append(d)?;
    }
    Ok(out.into())
}

/// Module entry point. `import agx` hits this.
#[pymodule]
fn agx(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(load, m)?)?;
    m.add_function(wrap_pyfunction!(load_corpus, m)?)?;
    m.add_function(wrap_pyfunction!(scan_pii, m)?)?;
    Ok(())
}
