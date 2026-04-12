use crate::format::Format;
use anyhow::{Context, Result};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct SessionFile {
    pub path: PathBuf,
    pub format: Format,
    pub modified_secs: Option<u64>,
}

/// Scan the three known session-storage locations and return all discovered
/// session files, sorted by modified time (newest first).
pub fn discover_all() -> Vec<SessionFile> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };

    let mut files = Vec::new();
    files.extend(discover_claude_code(&home));
    files.extend(discover_codex(&home));
    files.extend(discover_gemini(&home));

    // Sort by modified time descending (newest first). Files without mtime
    // end up at the bottom.
    files.sort_by(|a, b| b.modified_secs.cmp(&a.modified_secs));
    files
}

fn discover_claude_code(home: &Path) -> Vec<SessionFile> {
    // ~/.claude/projects/<project-dir>/<session-uuid>.jsonl
    let root = home.join(".claude").join("projects");
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(&root) else {
        return out;
    };
    for project in projects.flatten() {
        let Ok(files) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl")
                && let Some(modified_secs) = mtime_secs(&file)
            {
                out.push(SessionFile {
                    path,
                    format: Format::ClaudeCode,
                    modified_secs: Some(modified_secs),
                });
            }
        }
    }
    out
}

fn discover_codex(home: &Path) -> Vec<SessionFile> {
    // ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl
    let root = home.join(".codex").join("sessions");
    let mut out = Vec::new();
    walk_depth(&root, 3, &mut |entry| {
        let path = entry.path();
        let name_ok = path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|n| n.starts_with("rollout-"));
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"));
        if name_ok
            && ext_ok
            && let Some(modified_secs) = mtime_secs(entry)
        {
            out.push(SessionFile {
                path,
                format: Format::Codex,
                modified_secs: Some(modified_secs),
            });
        }
    });
    out
}

fn discover_gemini(home: &Path) -> Vec<SessionFile> {
    // ~/.gemini/tmp/<project>/chats/session-*.json
    let root = home.join(".gemini").join("tmp");
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(&root) else {
        return out;
    };
    for project in projects.flatten() {
        let chats = project.path().join("chats");
        let Ok(files) = std::fs::read_dir(&chats) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let name_ok = path
                .file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|n| n.starts_with("session-"));
            let ext_ok = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("json"));
            if name_ok
                && ext_ok
                && let Some(modified_secs) = mtime_secs(&file)
            {
                out.push(SessionFile {
                    path,
                    format: Format::Gemini,
                    modified_secs: Some(modified_secs),
                });
            }
        }
    }
    out
}

fn walk_depth(root: &Path, depth: usize, visit: &mut dyn FnMut(&std::fs::DirEntry)) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                walk_depth(&path, depth - 1, visit);
            }
        } else {
            visit(&entry);
        }
    }
}

fn mtime_secs(entry: &std::fs::DirEntry) -> Option<u64> {
    let meta = entry.metadata().ok()?;
    let modified = meta.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Format a duration-since-modified in short relative form.
pub fn format_relative_time(modified_secs: Option<u64>) -> String {
    let Some(m) = modified_secs else {
        return "?".into();
    };
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return "?".into();
    };
    let now_secs = now.as_secs();
    if m > now_secs {
        return "future".into();
    }
    let delta = now_secs - m;
    if delta < 60 {
        "just now".into()
    } else if delta < 3_600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3_600)
    } else if delta < 2_592_000 {
        format!("{}d ago", delta / 86_400)
    } else {
        format!("{}mo ago", delta / 2_592_000)
    }
}

fn short_path(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

/// Print a numbered list of session files and read the user's choice from
/// stdin. Returns Ok(None) on `q`/empty/Ctrl-D, Ok(Some(path)) on valid
/// numeric input, or bubbles up IO errors.
pub fn prompt_user_to_choose(files: &[SessionFile]) -> Result<Option<PathBuf>> {
    const MAX_SHOWN: usize = 30;
    if files.is_empty() {
        println!("agx: no session files found in ~/.claude, ~/.codex, or ~/.gemini");
        return Ok(None);
    }

    let shown = files.len().min(MAX_SHOWN);
    println!(
        "agx — {} recent session(s) found, showing {shown}:\n",
        files.len()
    );
    for (i, f) in files.iter().take(MAX_SHOWN).enumerate() {
        let format_tag = match f.format {
            Format::ClaudeCode => "[Claude]",
            Format::Codex => "[Codex ]",
            Format::Gemini => "[Gemini]",
            Format::Generic => "[Generic]",
        };
        let when = format_relative_time(f.modified_secs);
        let display_path = short_path(&f.path);
        println!("  {:>3}. {format_tag}  {:>9}  {display_path}", i + 1, when);
    }
    if files.len() > MAX_SHOWN {
        println!("  ... ({} more, not shown)", files.len() - MAX_SHOWN);
    }
    print!("\nEnter number (1-{shown}) or q to quit: ");
    io::stdout().flush().context("flushing stdout")?;

    let mut line = String::new();
    let stdin = io::stdin();
    let read = stdin.lock().read_line(&mut line).context("reading stdin")?;
    if read == 0 {
        // EOF
        return Ok(None);
    }
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("q") {
        return Ok(None);
    }
    match trimmed.parse::<usize>() {
        Ok(n) if n >= 1 && n <= shown => Ok(Some(files[n - 1].path.clone())),
        _ => {
            println!("agx: invalid selection '{trimmed}'");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_relative_time_handles_recent() {
        // Using mtime = now - 30s
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(format_relative_time(Some(now - 30)), "just now");
    }

    #[test]
    fn format_relative_time_handles_minutes() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let s = format_relative_time(Some(now - 120));
        assert!(s.ends_with("m ago"));
    }

    #[test]
    fn format_relative_time_handles_hours() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let s = format_relative_time(Some(now - 7_200));
        assert!(s.ends_with("h ago"));
    }

    #[test]
    fn format_relative_time_handles_days() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let s = format_relative_time(Some(now - 172_800));
        assert!(s.ends_with("d ago"));
    }

    #[test]
    fn format_relative_time_handles_none() {
        assert_eq!(format_relative_time(None), "?");
    }

    #[test]
    fn format_relative_time_handles_future() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(format_relative_time(Some(now + 1000)), "future");
    }

    #[test]
    fn short_path_shortens_home_prefix() {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            let p = home.join("foo/bar");
            let s = short_path(&p);
            assert!(s.starts_with("~/"), "expected tilde prefix, got: {s}");
        }
    }
}
