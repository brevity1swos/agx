//! Per-step annotations — the first persistent write-back feature.
//!
//! Notes live outside the session file (agx is read-only with respect to
//! session data, always), in a sidecar JSON under `~/.agx/notes/`. Keyed
//! by a FNV-1a hash of the canonical session path so moves-within-the-
//! same-canonical-path keep their notes while renames start fresh (a
//! deliberate trade-off — session UUID extraction varies per format and
//! isn't available for all of them).
//!
//! File format (version 1):
//! ```json
//! {
//!   "version": 1,
//!   "path": "/absolute/path/to/session.jsonl",
//!   "notes": {
//!     "0": {"text": "...", "created_at_ms": 1704000000000, "updated_at_ms": 1704000000000},
//!     "5": {...}
//!   }
//! }
//! ```
//! Key is the 0-based step index as a JSON-string (since JSON objects
//! require string keys). `created_at_ms` never changes; `updated_at_ms`
//! refreshes on every edit.
//!
//! Writes go through a temp-file + rename so a partial write never
//! corrupts an existing notes file. Reads are fault-tolerant: a missing
//! file or a malformed parse yields an empty `Annotations` so the TUI
//! always has something to render against.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub text: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Annotations {
    #[serde(default = "default_version")]
    pub version: u32,
    /// Canonical absolute path of the session file these annotations
    /// belong to. Recorded for reference / portability — the disk-side
    /// filename is derived via `annotations_file_for`.
    #[serde(default)]
    pub path: String,
    /// Step index (0-based, as a JSON-string per the format) → note.
    /// `BTreeMap` keeps iteration order stable for the list overlay
    /// in follow-up work.
    #[serde(default)]
    pub notes: BTreeMap<String, Note>,
}

fn default_version() -> u32 {
    CURRENT_VERSION
}

impl Annotations {
    /// Fresh, empty annotations bound to the given session path.
    pub fn new(session_path: &Path) -> Self {
        Annotations {
            version: CURRENT_VERSION,
            path: session_path
                .canonicalize()
                .unwrap_or_else(|_| session_path.to_path_buf())
                .display()
                .to_string(),
            notes: BTreeMap::new(),
        }
    }

    /// True when no notes are stored. Used by the export integrations
    /// to skip emitting a "notes" section and by tests.
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// Get the note for a step index, if any.
    pub fn get(&self, step_idx: usize) -> Option<&Note> {
        self.notes.get(&step_idx.to_string())
    }

    /// True when the given step index has an annotation.
    pub fn has(&self, step_idx: usize) -> bool {
        self.notes.contains_key(&step_idx.to_string())
    }

    /// Upsert a note. Empty / whitespace-only text deletes the entry
    /// (the intuitive "clear the annotation" behavior). Returns `true`
    /// when the set changed anything.
    pub fn set(&mut self, step_idx: usize, text: &str) -> bool {
        let key = step_idx.to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return self.notes.remove(&key).is_some();
        }
        let now = now_ms();
        match self.notes.get_mut(&key) {
            Some(existing) if existing.text == trimmed => false,
            Some(existing) => {
                existing.text = trimmed.to_string();
                existing.updated_at_ms = now;
                true
            }
            None => {
                self.notes.insert(
                    key,
                    Note {
                        text: trimmed.to_string(),
                        created_at_ms: now,
                        updated_at_ms: now,
                    },
                );
                true
            }
        }
    }

    /// Iterate notes in numeric step-index order. `BTreeMap` iterates
    /// string keys lexicographically, which would put "12" before "2";
    /// we collect and re-sort by the parsed usize instead. Consumed by
    /// the TUI `A` list overlay and by the export writers (md / html /
    /// json) for their per-step note sections.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &Note)> {
        let mut items: Vec<(usize, &Note)> = self
            .notes
            .iter()
            .filter_map(|(k, v)| k.parse::<usize>().ok().map(|idx| (idx, v)))
            .collect();
        items.sort_by_key(|(idx, _)| *idx);
        items.into_iter()
    }

    /// Load annotations for a session from disk. Returns an empty set
    /// (not an error) when the file doesn't exist — the common case
    /// for sessions the user hasn't annotated yet.
    ///
    /// A corrupted / malformed notes file also returns empty rather
    /// than erroring, so one bad file doesn't prevent the TUI from
    /// launching. A stderr warning is emitted so users know to look.
    pub fn load_for(session_path: &Path) -> Self {
        let file = match annotations_file_for(session_path) {
            Ok(p) => p,
            Err(_) => return Self::new(session_path),
        };
        let Ok(contents) = fs::read_to_string(&file) else {
            return Self::new(session_path);
        };
        match serde_json::from_str::<Annotations>(&contents) {
            Ok(mut a) => {
                // Canonicalize the recorded path if it was a no-op
                // before (e.g. first save happened before canonicalize
                // succeeded).
                if a.path.is_empty() {
                    a.path = session_path.display().to_string();
                }
                a
            }
            Err(e) => {
                eprintln!(
                    "agx: ignoring malformed annotations file {}: {}",
                    file.display(),
                    e
                );
                Self::new(session_path)
            }
        }
    }

    /// Write to disk atomically: serialize to a sibling `*.tmp`, then
    /// rename. `rename(2)` is atomic on the same filesystem, so partial
    /// writes never corrupt an existing notes file.
    pub fn save_for(&self, session_path: &Path) -> Result<PathBuf> {
        let dest = annotations_file_for(session_path)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = dest.with_extension("json.tmp");
        {
            let mut f = fs::File::create(&tmp)
                .with_context(|| format!("creating temp file {}", tmp.display()))?;
            f.write_all(json.as_bytes())
                .with_context(|| format!("writing {}", tmp.display()))?;
            f.sync_all().ok();
        }
        fs::rename(&tmp, &dest)
            .with_context(|| format!("renaming {} → {}", tmp.display(), dest.display()))?;
        Ok(dest)
    }
}

/// Resolve the annotations file path for a given session.
///
/// Scheme: `<agx_dir>/notes/<session_stem>-<hash8>.json` where
/// `<hash8>` is the first 8 hex chars of FNV-1a-64 over the canonical
/// session path. Human-readable stem + short unique tag → collisions
/// are vanishingly unlikely in practice while filenames stay
/// recognizable when users inspect the directory.
pub fn annotations_file_for(session_path: &Path) -> Result<PathBuf> {
    let canonical = session_path
        .canonicalize()
        .unwrap_or_else(|_| session_path.to_path_buf());
    let key = canonical.display().to_string();
    let hash = fnv1a_64(key.as_bytes());
    let stem = canonical
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    let filename = format!("{stem}-{:08x}.json", hash as u32);
    Ok(agx_home_dir()?.join("notes").join(filename))
}

/// Root directory for agx's persistent state. `AGX_HOME` overrides for
/// tests; otherwise `~/.agx`. Returns an error when the HOME
/// environment variable is unset (which is very unusual on the
/// platforms we target, but we surface it explicitly rather than
/// silently dropping writes).
pub fn agx_home_dir() -> Result<PathBuf> {
    if let Some(override_dir) = std::env::var_os("AGX_HOME") {
        return Ok(PathBuf::from(override_dir));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("$HOME is not set"))?;
    Ok(PathBuf::from(home).join(".agx"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| {
            let millis = d.as_millis();
            u64::try_from(millis).unwrap_or(u64::MAX)
        })
        .unwrap_or(0)
}

/// FNV-1a 64-bit. Deterministic (unlike `std::collections::hash_map::DefaultHasher`
/// whose seed is process-random) so notes files don't change name
/// across agx invocations. 5-line implementation, no new crate dep.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};
    use tempfile::TempDir;

    // Cargo runs tests in parallel, so mutating the process-wide
    // `AGX_HOME` env var races across threads. Serialize access via
    // a module-level mutex — each test holds the guard for its full
    // lifetime (returned alongside the `TempDir` so both drop at
    // end-of-scope together).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn test_home() -> (TempDir, MutexGuard<'static, ()>) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        unsafe {
            std::env::set_var("AGX_HOME", tmp.path());
        }
        (tmp, guard)
    }

    #[test]
    fn new_is_empty_and_bound_to_path() {
        let _home = test_home();
        let a = Annotations::new(Path::new("/tmp/foo.jsonl"));
        assert!(a.is_empty());
        assert_eq!(a.version, CURRENT_VERSION);
        assert!(!a.path.is_empty());
    }

    #[test]
    fn set_inserts_and_trims_whitespace() {
        let _home = test_home();
        let mut a = Annotations::new(Path::new("/tmp/foo.jsonl"));
        let changed = a.set(0, "  hello  ");
        assert!(changed);
        let note = a.get(0).unwrap();
        assert_eq!(note.text, "hello");
        assert!(note.created_at_ms > 0);
        assert_eq!(note.created_at_ms, note.updated_at_ms);
    }

    #[test]
    fn set_with_empty_text_deletes() {
        let _home = test_home();
        let mut a = Annotations::new(Path::new("/tmp/foo.jsonl"));
        a.set(3, "real note");
        assert!(a.has(3));
        let changed = a.set(3, "   ");
        assert!(changed);
        assert!(!a.has(3));
    }

    #[test]
    fn set_to_identical_text_is_a_noop() {
        let _home = test_home();
        let mut a = Annotations::new(Path::new("/tmp/foo.jsonl"));
        a.set(1, "same");
        let changed = a.set(1, "same");
        assert!(!changed);
    }

    #[test]
    fn set_updates_updated_at() {
        let _home = test_home();
        let mut a = Annotations::new(Path::new("/tmp/foo.jsonl"));
        a.set(0, "first");
        let before = a.get(0).unwrap().updated_at_ms;
        std::thread::sleep(std::time::Duration::from_millis(2));
        a.set(0, "second");
        let after = a.get(0).unwrap().updated_at_ms;
        assert!(after > before);
        // created_at_ms stays the same across edits.
        assert_eq!(a.get(0).unwrap().created_at_ms, before);
    }

    #[test]
    fn iter_yields_notes_in_step_index_order() {
        let _home = test_home();
        let mut a = Annotations::new(Path::new("/tmp/foo.jsonl"));
        a.set(5, "five");
        a.set(1, "one");
        a.set(12, "twelve");
        let got: Vec<usize> = a.iter().map(|(idx, _)| idx).collect();
        assert_eq!(got, vec![1, 5, 12]);
    }

    #[test]
    fn save_then_load_round_trip() {
        let _home = test_home();
        let session = Path::new("/tmp/session-foo.jsonl");
        let mut a = Annotations::new(session);
        a.set(2, "this went wrong");
        a.set(7, "revisit this edit");
        let written = a.save_for(session).unwrap();
        assert!(written.exists(), "expected saved notes file to exist");

        let loaded = Annotations::load_for(session);
        assert_eq!(loaded.notes.len(), 2);
        assert_eq!(loaded.get(2).unwrap().text, "this went wrong");
        assert_eq!(loaded.get(7).unwrap().text, "revisit this edit");
    }

    #[test]
    fn load_for_nonexistent_file_returns_empty_without_error() {
        let _home = test_home();
        let a = Annotations::load_for(Path::new("/tmp/nonexistent.jsonl"));
        assert!(a.is_empty());
    }

    #[test]
    fn load_for_malformed_file_returns_empty_without_panic() {
        let home = test_home();
        let session = Path::new("/tmp/session-mal.jsonl");
        let target = annotations_file_for(session).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "{not valid json").unwrap();
        let a = Annotations::load_for(session);
        assert!(a.is_empty());
        // Keep `home` alive so the TempDir isn't dropped mid-test.
        let _ = home;
    }

    #[test]
    fn annotations_file_for_produces_readable_stem_plus_hash() {
        let _home = test_home();
        let path = annotations_file_for(Path::new("/tmp/abcd.jsonl")).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("abcd-"), "unexpected filename: {name}");
        assert!(name.ends_with(".json"), "unexpected filename: {name}");
        // Format: <stem>-<8-hex>.json → stem + 1 dash + 8 chars + 5 chars
        assert_eq!(name.len(), "abcd".len() + 1 + 8 + ".json".len());
    }

    #[test]
    fn annotations_file_for_different_paths_differ_in_hash_suffix() {
        let _home = test_home();
        let a = annotations_file_for(Path::new("/tmp/a/session.jsonl")).unwrap();
        let b = annotations_file_for(Path::new("/tmp/b/session.jsonl")).unwrap();
        assert_ne!(a.file_name(), b.file_name());
    }

    #[test]
    fn fnv1a_64_is_deterministic() {
        // The whole point of rolling our own FNV is determinism across
        // process launches — std's hashmap hasher has a random seed.
        let h1 = fnv1a_64(b"/tmp/foo.jsonl");
        let h2 = fnv1a_64(b"/tmp/foo.jsonl");
        assert_eq!(h1, h2);
        assert_ne!(h1, fnv1a_64(b"/tmp/bar.jsonl"));
    }
}
