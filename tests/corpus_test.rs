//! Integration test that scans `tests/corpus/<format>/` and asserts every
//! fixture parses without error.
//!
//! Users contribute anonymized real-world session files here (see
//! CONTRIBUTING.md). Synthetic fixtures live in `assets/`; `tests/corpus/`
//! is reserved for things pulled from real agent runs that have been
//! scrubbed of PII.
//!
//! The test is a no-op when the corpus directory is empty or absent — that
//! is the v0.1 state. As fixtures accumulate, this becomes our format-drift
//! safety net.

use std::path::{Path, PathBuf};
use std::process::Command;

fn agx_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agx"))
}

fn collect_fixtures(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(formats) = std::fs::read_dir(root) else {
        return out;
    };
    for format_dir in formats.flatten() {
        let Ok(files) = std::fs::read_dir(format_dir.path()) else {
            continue;
        };
        for file in files.flatten() {
            let p = file.path();
            if p.is_file() {
                out.push(p);
            }
        }
    }
    out
}

#[test]
fn every_corpus_fixture_parses_cleanly() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let fixtures = collect_fixtures(&root);
    if fixtures.is_empty() {
        // v0.1 baseline: no fixtures committed yet. Not a failure — see
        // CONTRIBUTING.md "Contributing fixtures" for how to add one.
        return;
    }
    let mut failures = Vec::new();
    for path in &fixtures {
        let output = Command::new(agx_bin())
            .arg("--summary")
            .arg(path)
            .output()
            .expect("failed to run agx");
        if !output.status.success() {
            failures.push(format!(
                "{}: exit {:?}\nstderr: {}",
                path.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} corpus fixture(s) failed to parse:\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
}
