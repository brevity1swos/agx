use crate::timeline::{
    SessionTotals, Step, StepKind, ToolStats, compute_session_totals, compute_tool_stats,
    format_duration_ms, is_error_result, truncate,
};
use anyhow::Result;
use arboard::Clipboard;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap};
use std::collections::HashMap;
use std::io;
use std::time::Duration;

const PAGE_STEP: usize = 10;
const HELP_POPUP_WIDTH: u16 = 64;
const ALT_BG: Color = Color::Indexed(236);
const SEARCH_HIT_BG: Color = Color::Indexed(58);

enum InputMode {
    Command(String),
    Filter(String),
    Search(String),
    /// `a` in normal mode opens this mode for the current step. Enter
    /// upserts the note (empty text deletes). Esc discards. The
    /// attached `step_idx` is the *original* (pre-filter) index, so
    /// the note lands on the right step even when the view is
    /// filtered.
    Annotation {
        step_idx: usize,
        buffer: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKey {
    SetMark,
    JumpMark,
}

#[allow(clippy::struct_excessive_bools)]
pub struct App {
    steps: Vec<Step>,
    list_state: ListState,
    bg_flags: Vec<bool>,
    batch_flags: Vec<bool>,
    filtered_view: Vec<usize>,
    filter: Option<String>,
    search: Option<String>,
    search_matches: Vec<usize>,
    bookmarks: HashMap<char, usize>,
    pending: Option<PendingKey>,
    input_mode: Option<InputMode>,
    show_help: bool,
    status_msg: Option<String>,
    list_area: Option<Rect>,
    conversation_indices: Vec<usize>,
    conversation_list_state: ListState,
    three_pane: bool,
    count_buffer: String,
    tool_stats: Vec<ToolStats>,
    heatmap: Vec<u8>,
    show_heatmap: bool,
    show_stats: bool,
    /// When true, render the annotations list overlay. Toggled by `A`
    /// in normal mode. `annotations_list_state` tracks the cursor inside
    /// the overlay, independent of the main timeline's `list_state`.
    show_annotations: bool,
    annotations_list_state: ListState,
    /// When true, render the conversation-branch (fork-root) list
    /// overlay. Toggled by `b` in normal mode. Independent cursor.
    /// Population is a pre-computed `fork_indices` so the overlay
    /// stays constant-time to open.
    show_forks: bool,
    forks_list_state: ListState,
    fork_indices: Vec<usize>,
    /// When true, suppress cost estimates in status bar, detail pane, and
    /// stats overlay. Token counts are still shown. Set via `--no-cost`.
    no_cost: bool,
    session_totals: SessionTotals,
    /// Persisted per-step annotations. Empty by default when no session
    /// path is bound (e.g. unit-test App construction). Mutations go
    /// through `save_annotation` so on-disk state stays in sync with
    /// the in-memory view.
    annotations: crate::annotations::Annotations,
    /// The session file we're attached to, used to derive the notes
    /// file path. `None` in tests and for scratch usage where writes
    /// would have no meaningful destination.
    session_path: Option<std::path::PathBuf>,
}

impl App {
    pub fn new(steps: Vec<Step>, no_cost: bool) -> Self {
        let mut list_state = ListState::default();
        if !steps.is_empty() {
            list_state.select(Some(0));
        }
        let bg_flags = compute_bg_flags(&steps);
        let batch_flags = compute_batch_flags(&steps);
        let filtered_view: Vec<usize> = (0..steps.len()).collect();
        let conversation_indices = compute_conversation_indices(&steps);
        let mut conversation_list_state = ListState::default();
        if !conversation_indices.is_empty() {
            conversation_list_state.select(Some(0));
        }
        let mut app = Self {
            steps,
            list_state,
            bg_flags,
            batch_flags,
            filtered_view,
            filter: None,
            search: None,
            search_matches: Vec::new(),
            bookmarks: HashMap::new(),
            pending: None,
            input_mode: None,
            show_help: false,
            status_msg: None,
            list_area: None,
            conversation_indices,
            conversation_list_state,
            three_pane: true,
            count_buffer: String::new(),
            tool_stats: Vec::new(),
            heatmap: Vec::new(),
            show_heatmap: false,
            show_stats: false,
            show_annotations: false,
            annotations_list_state: ListState::default(),
            show_forks: false,
            forks_list_state: ListState::default(),
            fork_indices: Vec::new(),
            no_cost,
            session_totals: SessionTotals::default(),
            annotations: crate::annotations::Annotations::default(),
            session_path: None,
        };
        app.tool_stats = compute_tool_stats(&app.steps);
        app.heatmap = compute_heatmap(&app.steps);
        app.session_totals = compute_session_totals(&app.steps);
        app.fork_indices = crate::timeline::fork_root_indices(&app.steps);
        app.sync_conversation_cursor();
        app
    }

    fn toggle_heatmap(&mut self) {
        self.show_heatmap = !self.show_heatmap;
    }

    fn reload_steps(&mut self, new_steps: Vec<Step>) {
        let old_sel = self.list_state.selected();
        self.steps = new_steps;
        self.bg_flags = compute_bg_flags(&self.steps);
        self.batch_flags = compute_batch_flags(&self.steps);
        self.heatmap = compute_heatmap(&self.steps);
        self.conversation_indices = compute_conversation_indices(&self.steps);
        self.tool_stats = compute_tool_stats(&self.steps);
        self.session_totals = compute_session_totals(&self.steps);
        self.fork_indices = crate::timeline::fork_root_indices(&self.steps);
        self.filter = None;
        self.search = None;
        self.search_matches.clear();
        self.bookmarks.clear();
        self.filtered_view = (0..self.steps.len()).collect();
        if let Some(sel) = old_sel {
            let clamped = sel.min(self.steps.len().saturating_sub(1));
            self.list_state.select(Some(clamped));
        }
        self.sync_conversation_cursor();
    }

    fn toggle_stats(&mut self) {
        self.show_stats = !self.show_stats;
    }

    /// Open or close the annotations list overlay. On open, seed the
    /// overlay cursor at the first annotation so `Enter` always jumps
    /// somewhere meaningful. When there are no annotations, open with
    /// an empty `ListState` and render the empty-state hint.
    fn toggle_annotations_list(&mut self) {
        if self.show_annotations {
            self.show_annotations = false;
            return;
        }
        self.show_annotations = true;
        let has_any = self.annotations.iter().next().is_some();
        self.annotations_list_state
            .select(if has_any { Some(0) } else { None });
    }

    /// Move the annotations-overlay cursor by `delta` (positive = down).
    /// Clamped to the valid range; no-op when the list is empty.
    fn annotations_cursor_move(&mut self, delta: isize) {
        let len = self.annotations.iter().count();
        if len == 0 {
            return;
        }
        let cur = self.annotations_list_state.selected().unwrap_or(0);
        let next = (cur as isize + delta).clamp(0, (len - 1) as isize);
        self.annotations_list_state.select(Some(next as usize));
    }

    /// Open or close the conversation-branch (fork-root) list overlay.
    /// Fork roots live outside the main linear timeline — edit/resume
    /// in Claude Code creates them. The overlay is how users see the
    /// branch structure without having to guess from the `║` gutter
    /// marker in the main list.
    fn toggle_forks_list(&mut self) {
        if self.show_forks {
            self.show_forks = false;
            return;
        }
        self.show_forks = true;
        self.forks_list_state
            .select(if self.fork_indices.is_empty() {
                None
            } else {
                Some(0)
            });
    }

    /// Move the fork-overlay cursor by `delta`. Clamped; no-op when the
    /// fork list is empty.
    fn forks_cursor_move(&mut self, delta: isize) {
        if self.fork_indices.is_empty() {
            return;
        }
        let len = self.fork_indices.len();
        let cur = self.forks_list_state.selected().unwrap_or(0);
        let next = (cur as isize + delta).clamp(0, (len - 1) as isize);
        self.forks_list_state.select(Some(next as usize));
    }

    /// Jump the main timeline cursor to the fork-root step selected in
    /// the overlay. Same degradation story as the annotation overlay:
    /// filter-hidden targets surface a status message rather than
    /// silently moving somewhere else.
    fn jump_to_selected_fork(&mut self) {
        let Some(overlay_idx) = self.forks_list_state.selected() else {
            self.show_forks = false;
            return;
        };
        let Some(&orig_idx) = self.fork_indices.get(overlay_idx) else {
            self.show_forks = false;
            return;
        };
        self.show_forks = false;
        match self.filtered_view.iter().position(|&i| i == orig_idx) {
            Some(view_idx) => {
                self.list_state.select(Some(view_idx));
            }
            None => {
                self.status_msg = Some(format!(
                    "fork root at step {} is hidden by the active filter",
                    orig_idx + 1
                ));
            }
        }
    }

    /// Jump the main timeline cursor to the annotation currently selected
    /// in the overlay. Closes the overlay as a side effect so the user
    /// lands on the step ready to act on it. When the target step is
    /// hidden by the active filter, surface that via `status_msg`
    /// instead of silently moving somewhere else.
    fn jump_to_selected_annotation(&mut self) {
        let Some(overlay_idx) = self.annotations_list_state.selected() else {
            self.show_annotations = false;
            return;
        };
        let Some((orig_idx, _)) = self.annotations.iter().nth(overlay_idx) else {
            self.show_annotations = false;
            return;
        };
        self.show_annotations = false;
        match self.filtered_view.iter().position(|&i| i == orig_idx) {
            Some(view_idx) => {
                self.list_state.select(Some(view_idx));
            }
            None => {
                self.status_msg = Some(format!(
                    "annotation on step {} is hidden by the active filter",
                    orig_idx + 1
                ));
            }
        }
    }

    fn copy_current_step(&mut self) {
        let Some(view_idx) = self.list_state.selected() else {
            self.status_msg = Some("nothing to copy".into());
            return;
        };
        let Some(&orig) = self.filtered_view.get(view_idx) else {
            self.status_msg = Some("nothing to copy".into());
            return;
        };
        let Some(step) = self.steps.get(orig) else {
            self.status_msg = Some("nothing to copy".into());
            return;
        };
        match Clipboard::new().and_then(|mut cb| cb.set_text(step.detail.clone())) {
            Ok(()) => {
                self.status_msg = Some(format!(
                    "copied step {} to clipboard ({} chars)",
                    orig + 1,
                    step.detail.len()
                ));
            }
            Err(e) => {
                self.status_msg = Some(format!("clipboard error: {e}"));
            }
        }
    }

    fn append_count_digit(&mut self, c: char) {
        if c.is_ascii_digit() && self.count_buffer.len() < 6 {
            self.count_buffer.push(c);
        }
    }

    fn take_count(&mut self) -> usize {
        let n = self.count_buffer.parse::<usize>().unwrap_or(1).max(1);
        self.count_buffer.clear();
        n
    }

    fn has_count(&self) -> bool {
        !self.count_buffer.is_empty()
    }

    fn clear_count(&mut self) {
        self.count_buffer.clear();
    }

    fn sync_conversation_cursor(&mut self) {
        let Some(view_idx) = self.list_state.selected() else {
            self.conversation_list_state.select(None);
            return;
        };
        let Some(&orig) = self.filtered_view.get(view_idx) else {
            self.conversation_list_state.select(None);
            return;
        };
        let target = self.conversation_indices.iter().rposition(|&i| i <= orig);
        self.conversation_list_state.select(target);
    }

    fn toggle_layout(&mut self) {
        self.three_pane = !self.three_pane;
    }

    fn click_to_select(&mut self, view_idx: usize) {
        if self.filtered_view.is_empty() {
            return;
        }
        let clamped = view_idx.min(self.filtered_view.len() - 1);
        self.list_state.select(Some(clamped));
    }

    fn visible_count(&self) -> usize {
        self.filtered_view.len()
    }

    fn next(&mut self) {
        if self.filtered_view.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + 1).min(self.filtered_view.len() - 1);
        self.list_state.select(Some(next));
    }

    fn prev(&mut self) {
        if self.filtered_view.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.saturating_sub(1)));
    }

    fn page_down(&mut self, n: usize) {
        if self.filtered_view.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + n).min(self.filtered_view.len() - 1);
        self.list_state.select(Some(next));
    }

    fn page_up(&mut self, n: usize) {
        if self.filtered_view.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.saturating_sub(n)));
    }

    fn home(&mut self) {
        if !self.filtered_view.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    fn end(&mut self) {
        if !self.filtered_view.is_empty() {
            self.list_state.select(Some(self.filtered_view.len() - 1));
        }
    }

    fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    fn enter_command_mode(&mut self) {
        self.input_mode = Some(InputMode::Command(String::new()));
        self.status_msg = None;
    }

    fn enter_filter_mode(&mut self) {
        let existing = self.filter.clone().unwrap_or_default();
        self.input_mode = Some(InputMode::Filter(existing));
        self.status_msg = None;
    }

    fn enter_search_mode(&mut self) {
        let existing = self.search.clone().unwrap_or_default();
        self.input_mode = Some(InputMode::Search(existing));
        self.status_msg = None;
    }

    /// Open the annotation input for the current step. Prefills with
    /// any existing note so `a` acts as edit-in-place when a note is
    /// already there.
    fn enter_annotation_mode(&mut self) {
        let Some(view_idx) = self.list_state.selected() else {
            self.status_msg = Some("no step selected to annotate".into());
            return;
        };
        let Some(&orig_idx) = self.filtered_view.get(view_idx) else {
            self.status_msg = Some("no step selected to annotate".into());
            return;
        };
        let existing = self
            .annotations
            .get(orig_idx)
            .map(|n| n.text.clone())
            .unwrap_or_default();
        self.input_mode = Some(InputMode::Annotation {
            step_idx: orig_idx,
            buffer: existing,
        });
        self.status_msg = None;
    }

    /// Apply an annotation buffer from input mode — upsert on non-empty
    /// text, delete on empty. Save to disk when a session path is
    /// bound; surface any write failure via `status_msg` rather than
    /// panicking. Called from the Enter handler in the event loop.
    fn save_annotation(&mut self, step_idx: usize, text: &str) {
        let trimmed = text.trim();
        let changed = self.annotations.set(step_idx, trimmed);
        if let Some(path) = &self.session_path {
            match self.annotations.save_for(path) {
                Ok(_) => {
                    let msg = if trimmed.is_empty() {
                        format!("cleared annotation for step {}", step_idx + 1)
                    } else if changed {
                        format!("saved annotation for step {}", step_idx + 1)
                    } else {
                        "annotation unchanged".into()
                    };
                    self.status_msg = Some(msg);
                }
                Err(e) => {
                    self.status_msg = Some(format!("annotation save failed: {e}"));
                }
            }
        } else {
            self.status_msg = Some("annotations not saved (no session path bound)".into());
        }
    }

    fn begin_set_mark(&mut self) {
        self.pending = Some(PendingKey::SetMark);
        self.status_msg = None;
    }

    fn begin_jump_mark(&mut self) {
        self.pending = Some(PendingKey::JumpMark);
        self.status_msg = None;
    }

    fn cancel_pending(&mut self) {
        self.pending = None;
    }

    fn set_mark(&mut self, ch: char) {
        let Some(view_idx) = self.list_state.selected() else {
            self.status_msg = Some("no current step to bookmark".into());
            return;
        };
        let Some(&orig) = self.filtered_view.get(view_idx) else {
            self.status_msg = Some("no current step to bookmark".into());
            return;
        };
        self.bookmarks.insert(ch, orig);
        self.status_msg = Some(format!("bookmark '{ch}' set at step {}", orig + 1));
    }

    fn jump_to_mark(&mut self, ch: char) {
        let Some(&orig) = self.bookmarks.get(&ch) else {
            self.status_msg = Some(format!("no bookmark '{ch}'"));
            return;
        };
        match self.filtered_view.iter().position(|&i| i == orig) {
            Some(view_idx) => {
                self.list_state.select(Some(view_idx));
            }
            None => {
                self.status_msg = Some(format!(
                    "bookmark '{ch}' points to step {} (hidden by filter)",
                    orig + 1
                ));
            }
        }
    }

    /// Position the timeline cursor at a 0-indexed step, clamped to the
    /// visible (filtered) range. Sets `status_msg` to a clamp warning
    /// when the requested step is out of range. Extracted as a method
    /// so the `--jump-to` CLI flag (sift Timeline-jump integration)
    /// can be tested headlessly without stepping through the TUI event
    /// loop.
    pub(crate) fn apply_initial_step(&mut self, n: usize) {
        if self.filtered_view.is_empty() {
            return;
        }
        let max = self.filtered_view.len() - 1;
        let target = n.min(max);
        self.list_state.select(Some(target));
        self.sync_conversation_cursor();
        if n > max {
            self.status_msg = Some(format!(
                "--jump-to {n} out of range (session has {} steps); clamped to last",
                self.filtered_view.len()
            ));
        }
    }

    fn goto_step(&mut self, step_num: usize) {
        if self.filtered_view.is_empty() {
            self.status_msg = Some("no steps to navigate".into());
            return;
        }
        if step_num == 0 {
            self.status_msg = Some("step number must be >= 1".into());
            return;
        }
        let idx = (step_num - 1).min(self.filtered_view.len() - 1);
        self.list_state.select(Some(idx));
    }

    fn execute_command(&mut self, input: &str) {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return;
        }
        // `:@<duration>` — jump to first step at-or-after that offset
        // from the session's first-step timestamp. Uses the shared
        // slice duration parser so the grammar matches `--after` /
        // `--before` exactly.
        if let Some(offset_str) = trimmed.strip_prefix('@') {
            match crate::slice::parse_duration_ms(offset_str) {
                Ok(offset_ms) => self.goto_time_offset(offset_ms),
                Err(e) => {
                    self.status_msg = Some(format!("bad time offset `@{offset_str}`: {e}"));
                }
            }
            return;
        }
        match trimmed.parse::<usize>() {
            Ok(n) => self.goto_step(n),
            Err(_) => {
                self.status_msg = Some(format!("unknown command: :{trimmed}"));
            }
        }
    }

    /// Jump the cursor to the first step whose timestamp is at least
    /// `offset_ms` past the session's first-step timestamp. Used by
    /// the `:@<duration>` command. No-op (with a status message) when
    /// the session has no step timestamps.
    fn goto_time_offset(&mut self, offset_ms: u64) {
        let Some(session_start_ms) = self.steps.iter().find_map(|s| s.timestamp_ms) else {
            self.status_msg = Some("session has no step timestamps; `:@` jump unavailable".into());
            return;
        };
        let target = session_start_ms.saturating_add(offset_ms);
        let Some(target_idx) = self
            .steps
            .iter()
            .position(|s| s.timestamp_ms.is_some_and(|ts| ts >= target))
        else {
            self.status_msg = Some(format!(
                "no step at-or-after +{}ms from session start",
                offset_ms
            ));
            return;
        };
        // Translate original-step index into the current filtered view.
        // If the target is hidden by a filter, report it explicitly
        // rather than silently jumping somewhere unexpected.
        match self.filtered_view.iter().position(|&i| i == target_idx) {
            Some(view_idx) => self.list_state.select(Some(view_idx)),
            None => {
                self.status_msg = Some(format!(
                    "step {} at +{}ms is hidden by the active filter",
                    target_idx + 1,
                    offset_ms
                ));
            }
        }
    }

    fn apply_filter(&mut self, query: &str) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            self.clear_filter();
            return;
        }
        let needle = trimmed.to_lowercase();
        let indices: Vec<usize> = self
            .steps
            .iter()
            .enumerate()
            .filter(|(_, s)| s.label.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect();
        if indices.is_empty() {
            self.status_msg = Some(format!("no matches for '{trimmed}'"));
            return;
        }
        self.filter = Some(trimmed.to_string());
        self.filtered_view = indices;
        self.list_state.select(Some(0));
        self.recompute_search_matches();
    }

    fn clear_filter(&mut self) {
        self.filter = None;
        self.filtered_view = (0..self.steps.len()).collect();
        if self.filtered_view.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
        self.recompute_search_matches();
    }

    fn apply_search(&mut self, query: &str) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            self.clear_search();
            return;
        }
        // Leading `//` → semantic search (Phase 4.4). Everything after the
        // prefix is the query sent to the embedder. The `//` marker was
        // picked over `/` because the regular search prompt is opened
        // with `/`, and `//foo` is an unambiguous, easy-to-type way to
        // say "I mean semantic, not substring." When the feature is off,
        // `semantic::rank` returns `None` and we surface the rebuild
        // hint via status_msg without touching current search state.
        if let Some(sem_query) = trimmed.strip_prefix("//") {
            self.apply_semantic_search(sem_query);
            return;
        }
        self.search = Some(trimmed.to_string());
        self.recompute_search_matches();
        if self.search_matches.is_empty() {
            self.status_msg = Some(format!("no matches for '{trimmed}'"));
            self.search = None;
            return;
        }
        // Jump to first match at-or-after the current selection, wrapping if needed.
        let current = self.list_state.selected().unwrap_or(0);
        let target = self
            .search_matches
            .iter()
            .copied()
            .find(|&idx| idx >= current)
            .or_else(|| self.search_matches.first().copied());
        if let Some(idx) = target {
            self.list_state.select(Some(idx));
        }
    }

    /// Dispatch a `//query` semantic search. Converts the embedder's
    /// original-index results into filtered_view positions so the
    /// existing highlight + jump paths work without modification.
    /// When the feature is off, surfaces the rebuild hint and leaves
    /// any active string-match search state untouched.
    fn apply_semantic_search(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            self.status_msg = Some("empty semantic query".into());
            return;
        }
        let Some(orig_matches) = crate::semantic::rank(query, &self.steps) else {
            self.status_msg = Some(crate::semantic::FEATURE_DISABLED_MESSAGE.into());
            return;
        };
        if orig_matches.is_empty() {
            self.status_msg = Some(format!("no semantic matches for '{query}'"));
            self.search = None;
            self.search_matches.clear();
            return;
        }
        // Map original step indices into the current filtered view.
        // Steps not in filtered_view are dropped (they can't be
        // highlighted in the list anyway).
        let mut view_matches: Vec<usize> = orig_matches
            .into_iter()
            .filter_map(|orig| self.filtered_view.iter().position(|&i| i == orig))
            .collect();
        view_matches.sort_unstable();
        view_matches.dedup();
        if view_matches.is_empty() {
            self.status_msg = Some(format!(
                "semantic matches for '{query}' are all hidden by the active filter"
            ));
            return;
        }
        self.search = Some(format!("//{query}"));
        self.search_matches = view_matches;
        // Jump to first match at-or-after the current selection.
        let current = self.list_state.selected().unwrap_or(0);
        let target = self
            .search_matches
            .iter()
            .copied()
            .find(|&idx| idx >= current)
            .or_else(|| self.search_matches.first().copied());
        if let Some(idx) = target {
            self.list_state.select(Some(idx));
        }
    }

    fn clear_search(&mut self) {
        self.search = None;
        self.search_matches.clear();
    }

    fn recompute_search_matches(&mut self) {
        self.search_matches.clear();
        let Some(query) = self.search.as_deref() else {
            return;
        };
        // Semantic searches (stored with the `//` prefix in
        // `self.search`) aren't re-embeddable on every filter change
        // without blocking the UI. Drop them when filters change — the
        // user can re-run `//query` to refresh. Cheaper than a cached
        // embedding index and keeps the hot path reserved for
        // substring search.
        if query.starts_with("//") {
            self.search = None;
            return;
        }
        let needle = query.to_lowercase();
        for (view_idx, &orig) in self.filtered_view.iter().enumerate() {
            let Some(step) = self.steps.get(orig) else {
                continue;
            };
            if step.label.to_lowercase().contains(&needle)
                || step.detail.to_lowercase().contains(&needle)
            {
                self.search_matches.push(view_idx);
            }
        }
    }

    fn next_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let target = self
            .search_matches
            .iter()
            .copied()
            .find(|&idx| idx > current)
            .or_else(|| self.search_matches.first().copied());
        if let Some(idx) = target {
            self.list_state.select(Some(idx));
        }
    }

    fn prev_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let target = self
            .search_matches
            .iter()
            .copied()
            .rev()
            .find(|&idx| idx < current)
            .or_else(|| self.search_matches.last().copied());
        if let Some(idx) = target {
            self.list_state.select(Some(idx));
        }
    }
}

const HEATMAP_WINDOW: usize = 5;

fn compute_heatmap(steps: &[Step]) -> Vec<u8> {
    let len = steps.len();
    (0..len)
        .map(|i| {
            let lo = i.saturating_sub(HEATMAP_WINDOW);
            let hi = (i + HEATMAP_WINDOW + 1).min(len);
            let count = steps[lo..hi]
                .iter()
                .filter(|s| matches!(s.kind, StepKind::ToolUse | StepKind::ToolResult))
                .count();
            u8::try_from(count).unwrap_or(u8::MAX)
        })
        .collect()
}

fn density_color(density: u8) -> Option<Color> {
    match density {
        0 => None,
        1..=2 => Some(Color::Indexed(17)),
        3..=4 => Some(Color::Indexed(22)),
        5..=7 => Some(Color::Indexed(130)),
        8..=10 => Some(Color::Indexed(208)),
        _ => Some(Color::Indexed(196)),
    }
}

// Detect runs of 2+ consecutive tool_use or tool_result steps. These are
// batched parallel tool calls (Claude Code parallel Agent dispatches,
// Codex batched function_calls). Returns a flag per original step index.
fn compute_batch_flags(steps: &[Step]) -> Vec<bool> {
    let mut flags = vec![false; steps.len()];
    let mut i = 0;
    while i < steps.len() {
        let kind = steps[i].kind;
        if matches!(kind, StepKind::ToolUse | StepKind::ToolResult) {
            let mut j = i;
            while j < steps.len() && steps[j].kind == kind {
                j += 1;
            }
            if j - i >= 2 {
                for item in flags.iter_mut().take(j).skip(i) {
                    *item = true;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    flags
}

// Collect original step indices of user/assistant text steps, in order.
// These form the "conversation view" — the flowing read-only pane that
// complements the step-by-step timeline.
fn compute_conversation_indices(steps: &[Step]) -> Vec<usize> {
    steps
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            matches!(s.kind, StepKind::UserText | StepKind::AssistantText).then_some(i)
        })
        .collect()
}

// Returns a Vec<bool> parallel to `steps`. For each ToolUse / ToolResult step,
// the flag alternates so adjacent tool calls get distinct backgrounds. Text
// steps get `false` (no alternating bg).
fn compute_bg_flags(steps: &[Step]) -> Vec<bool> {
    let mut flags = vec![false; steps.len()];
    let mut tool_use_parity = false;
    let mut tool_result_parity = false;
    for (i, step) in steps.iter().enumerate() {
        match step.kind {
            StepKind::ToolUse => {
                flags[i] = tool_use_parity;
                tool_use_parity = !tool_use_parity;
            }
            StepKind::ToolResult => {
                flags[i] = tool_result_parity;
                tool_result_parity = !tool_result_parity;
            }
            _ => {}
        }
    }
    flags
}

fn kind_color(kind: StepKind) -> Color {
    // StepKind is `#[non_exhaustive]` per docs/stability.md — new
    // variants (e.g. MCP resource reads from Phase 5.2) will fall
    // through the wildcard and render in white until we add an
    // explicit arm. That's the right default: visible but not
    // mis-categorized.
    match kind {
        StepKind::UserText => Color::Cyan,
        StepKind::AssistantText => Color::Green,
        StepKind::ToolUse => Color::Yellow,
        StepKind::ToolResult => Color::Magenta,
        _ => Color::White,
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
    }
}

/// Runtime configuration for live-mode desktop notifications. Kept
/// here rather than in `notify.rs` because the field shape is a TUI
/// concern — `notify.rs` owns only the "fire a notification" wrapper,
/// not the policy for when to fire. Both fields are no-ops on a
/// feature-off build (the `notify::error` / `notify::idle` calls
/// return `Ok(())` without touching anything), so leaving this struct
/// outside any `cfg` keeps the event loop readable.
#[derive(Debug, Clone, Copy, Default)]
pub struct NotifyConfig {
    /// Fire a notification when a newly arrived `tool_result` matches
    /// `is_error_result`.
    pub on_error: bool,
    /// Fire a notification when the session hasn't grown for at least
    /// this many milliseconds. `None` disables. Fires at most once per
    /// idle interval — growth resets the trigger.
    pub on_idle_ms: Option<u64>,
}

pub fn run(
    steps: Vec<Step>,
    reload_fn: Option<&dyn Fn() -> Result<Vec<Step>>>,
    no_cost: bool,
    session_path: Option<&std::path::Path>,
    initial_step: Option<usize>,
    notify: NotifyConfig,
    replay: crate::replay::ReplayConfig,
) -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(steps, no_cost);
    // Attach annotation state when a session path is provided. Load is
    // fault-tolerant: a missing or malformed notes file returns an
    // empty `Annotations` with a stderr warning, not an error.
    if let Some(path) = session_path {
        app.annotations = crate::annotations::Annotations::load_for(path);
        app.session_path = Some(path.to_path_buf());
    }
    // Apply `--jump-to` if provided. 0-indexed, clamped to the visible
    // range. Out-of-bounds surfaces a status-bar warning rather than
    // exit-erroring — the TUI still launches so the user can see the
    // session they asked for. Per docs/suite-conventions.md §5 this is
    // the public CLI surface sift's Timeline-jump integration targets.
    if let Some(n) = initial_step {
        app.apply_initial_step(n);
    }

    let result = run_loop(
        &mut terminal,
        &mut app,
        reload_fn,
        notify,
        replay,
        session_path,
    );
    let _ = terminal.show_cursor();
    result
}

// run_loop is intentionally one function — TUI render + event handling form one
// logical operation per frame; splitting hurts readability more than it helps.
#[allow(clippy::too_many_lines)]
fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    reload_fn: Option<&dyn Fn() -> Result<Vec<Step>>>,
    notify: NotifyConfig,
    replay: crate::replay::ReplayConfig,
    session_path: Option<&std::path::Path>,
) -> Result<()> {
    // Track the last time the session grew. Drives both --notify-on-idle
    // (fire when too-long-since-growth) and the idle_fired latch that
    // prevents the notification from re-firing every frame once the
    // threshold has been crossed.
    let mut last_growth = std::time::Instant::now();
    let mut idle_fired = false;
    // Phase 5.4 — pending replay confirm. Set when the user presses
    // `R` on a replayable step; cleared on y/n. A Some value means
    // "next keystroke is a confirm response."
    let mut pending_replay: Option<(usize, String)> = None;
    loop {
        // Live reload: poll file and refresh if step count changed.
        if let Some(reload) = reload_fn
            && let Ok(new_steps) = reload()
            && new_steps.len() != app.steps.len()
        {
            let prev_len = app.steps.len();
            let grew = new_steps.len() > prev_len;
            // Snapshot the newly-added steps before reload_steps() moves
            // the vec. For shrinkage (file truncation / rewrite) we
            // skip error scanning entirely — the delta isn't an append.
            let error_labels: Vec<String> = if grew && notify.on_error {
                new_steps[prev_len..]
                    .iter()
                    .filter(|s| is_error_result(s))
                    .map(|s| s.label.clone())
                    .collect()
            } else {
                Vec::new()
            };
            app.reload_steps(new_steps);
            last_growth = std::time::Instant::now();
            idle_fired = false;
            for label in error_labels {
                // Best-effort — a failed OS notification must never
                // crash the live loop.
                let _ = crate::notify::error(&label);
            }
        }

        // Idle notification check runs every iteration (not just on
        // reload) so we fire promptly once the threshold elapses.
        if let Some(threshold_ms) = notify.on_idle_ms
            && !idle_fired
            && u64::try_from(last_growth.elapsed().as_millis()).unwrap_or(u64::MAX) >= threshold_ms
        {
            let _ = crate::notify::idle(threshold_ms / 1_000);
            idle_fired = true;
        }

        terminal.draw(|f| {
            // Keep the conversation pane cursor in lockstep with the timeline.
            app.sync_conversation_cursor();

            // Outer layout: main area + 1-row status/command bar at the bottom.
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(f.area());

            // Main area: either 3-pane (timeline | conversation | detail)
            // or 2-pane (timeline | detail). Tab toggles.
            let (chunks, conv_chunk): (std::rc::Rc<[Rect]>, Option<Rect>) = if app.three_pane {
                let split = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(25),
                        Constraint::Percentage(40),
                        Constraint::Percentage(35),
                    ])
                    .split(outer[0]);
                // Rearrange as [list, detail] so the existing downstream code
                // that renders list at chunks[0] and detail at chunks[1] still works.
                let repacked: std::rc::Rc<[Rect]> = std::rc::Rc::from(vec![split[0], split[2]]);
                (repacked, Some(split[1]))
            } else {
                (
                    Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                        .split(outer[0]),
                    None,
                )
            };
            app.list_area = Some(chunks[0]);

            let items: Vec<ListItem> = app
                .filtered_view
                .iter()
                .enumerate()
                .filter_map(|(view_idx, &orig_idx)| {
                    let s = app.steps.get(orig_idx)?;
                    let is_error = is_error_result(s);
                    let is_batched = app.batch_flags.get(orig_idx).copied().unwrap_or(false);
                    let color = if is_error {
                        Color::Red
                    } else {
                        kind_color(s.kind)
                    };
                    let mut style = Style::default().fg(color);
                    if is_error {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    let is_match = app.search_matches.binary_search(&view_idx).is_ok();
                    if is_match {
                        style = style.bg(SEARCH_HIT_BG).add_modifier(Modifier::BOLD);
                    } else if app.show_heatmap {
                        if let Some(color) =
                            app.heatmap.get(orig_idx).copied().and_then(density_color)
                        {
                            style = style.bg(color);
                        }
                    } else if app.bg_flags.get(orig_idx).copied().unwrap_or(false) {
                        style = style.bg(ALT_BG);
                    }
                    // Two-char prefix column. Annotations take priority
                    // over the batch marker — they're more load-bearing
                    // user signal (persistent, explicit) than the
                    // derived structural batch indicator.
                    let has_note = app.annotations.has(orig_idx);
                    let (prefix, prefix_style) = if has_note {
                        ("* ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))
                    } else if is_batched {
                        ("║ ", Style::default().fg(Color::DarkGray))
                    } else {
                        ("  ", Style::default().fg(Color::DarkGray))
                    };
                    Some(ListItem::new(Line::from(vec![
                        Span::styled(prefix, prefix_style),
                        Span::styled(s.label.as_str(), style),
                    ])))
                })
                .collect();

            let total = app.visible_count();
            let current = app.list_state.selected().map_or(0, |i| i + 1);
            let mut title_parts: Vec<String> = vec![format!(" agx — {current}/{total}")];
            if let Some(q) = &app.filter {
                title_parts.push(format!("[filter: {q}]"));
            }
            if let Some(q) = &app.search {
                let hits = app.search_matches.len();
                title_parts.push(format!("[search: {q} · {hits}]"));
            }
            // Fork-root count — shown only when >0 so linear sessions
            // (the common case) don't gain a noise segment.
            if !app.fork_indices.is_empty() {
                title_parts.push(format!("[forks: {} · b]", app.fork_indices.len()));
            }
            title_parts.push("[? help] ".to_string());
            let title = title_parts.join("   ");

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title))
                .highlight_style(
                    Style::default()
                        .add_modifier(Modifier::REVERSED)
                        .add_modifier(Modifier::BOLD),
                );

            f.render_stateful_widget(list, chunks[0], &mut app.list_state);

            // Conversation pane (only in three-pane layout).
            if let Some(conv_area) = conv_chunk {
                let conv_items: Vec<ListItem> = app
                    .conversation_indices
                    .iter()
                    .filter_map(|&orig| {
                        let s = app.steps.get(orig)?;
                        let color = kind_color(s.kind);
                        let prefix = match s.kind {
                            StepKind::UserText => "user  ",
                            StepKind::AssistantText => "asst  ",
                            _ => "      ",
                        };
                        let mut text = String::with_capacity(prefix.len() + 200);
                        text.push_str(prefix);
                        for ch in s.detail.chars().take(200) {
                            text.push(if ch == '\n' { ' ' } else { ch });
                        }
                        Some(ListItem::new(Line::from(vec![Span::styled(
                            text,
                            Style::default().fg(color),
                        )])))
                    })
                    .collect();
                let conv_title = format!(" conversation ({}) ", app.conversation_indices.len());
                let conv_list = List::new(conv_items)
                    .block(Block::default().borders(Borders::ALL).title(conv_title))
                    .highlight_style(
                        Style::default()
                            .add_modifier(Modifier::REVERSED)
                            .add_modifier(Modifier::BOLD),
                    );
                f.render_stateful_widget(conv_list, conv_area, &mut app.conversation_list_state);
            }

            let selected_orig = app
                .list_state
                .selected()
                .and_then(|i| app.filtered_view.get(i).copied());
            let (detail_text, detail_kind) = match selected_orig
                .and_then(|orig| app.steps.get(orig).map(|s| (orig, s)))
            {
                None => (String::new(), None),
                Some((orig, s)) => {
                    let mut text = s.detail.clone();
                    // Prepend a metadata block when this step carries
                    // duration / model / usage / annotation. Skipped
                    // entirely when none of those are known.
                    let mut meta: Vec<String> = Vec::new();
                    if let Some(note) = app.annotations.get(orig) {
                        meta.push(format!("[note: {}]", note.text));
                    }
                    if let Some(ms) = s.duration_ms {
                        meta.push(format!("[{} since previous step]", format_duration_ms(ms)));
                    }
                    if let Some(m) = &s.model {
                        meta.push(format!("[model: {m}]"));
                    }
                    let has_tokens = s.tokens_in.is_some()
                        || s.tokens_out.is_some()
                        || s.cache_read.is_some()
                        || s.cache_create.is_some();
                    if has_tokens {
                        let parts: Vec<String> = [
                            ("in", s.tokens_in),
                            ("out", s.tokens_out),
                            ("cache_read", s.cache_read),
                            ("cache_create", s.cache_create),
                        ]
                        .iter()
                        .filter_map(|(label, v)| v.map(|n| format!("{label}: {n}")))
                        .collect();
                        meta.push(format!("[tokens — {}]", parts.join(", ")));
                    }
                    if !app.no_cost
                        && let Some(c) = s.cost_usd()
                    {
                        meta.push(format!("[estimated cost: ${c:.4} USD]"));
                    }
                    if !meta.is_empty() {
                        text = format!("{}\n\n{}", meta.join("\n"), text);
                    }
                    (text, Some(s.kind))
                }
            };

            let detail_title = match detail_kind {
                Some(StepKind::UserText) => " user ",
                Some(StepKind::AssistantText) => " assistant ",
                Some(StepKind::ToolUse) => " tool_use ",
                Some(StepKind::ToolResult) => " tool_result ",
                // Future `#[non_exhaustive]` variant — fall back to
                // the generic title until an explicit arm lands.
                Some(_) | None => " detail ",
            };

            let detail_widget = Paragraph::new(detail_text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(detail_title)
                        .border_style(
                            Style::default().fg(detail_kind.map_or(Color::Gray, kind_color)),
                        ),
                )
                .wrap(Wrap { trim: false });

            f.render_widget(detail_widget, chunks[1]);

            // Bottom bar: pending hint, input line, status msg, or scrubbing gauge.
            if let Some(pending) = app.pending {
                let hint = match pending {
                    PendingKey::SetMark => "set mark: press a-z to bookmark current step",
                    PendingKey::JumpMark => "jump to mark: press a-z to navigate",
                };
                let line = Paragraph::new(Line::from(vec![Span::styled(
                    hint,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]));
                f.render_widget(line, outer[1]);
            } else {
                match &app.input_mode {
                    Some(InputMode::Command(buf)) => {
                        let line = Paragraph::new(Line::from(vec![
                            Span::styled(":", Style::default().fg(Color::Yellow)),
                            Span::raw(buf.as_str()),
                            Span::styled(
                                "█",
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::SLOW_BLINK),
                            ),
                        ]));
                        f.render_widget(line, outer[1]);
                    }
                    Some(InputMode::Filter(buf)) => {
                        let line = Paragraph::new(Line::from(vec![
                            Span::styled(
                                "filter> ",
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(buf.as_str()),
                            Span::styled(
                                "█",
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::SLOW_BLINK),
                            ),
                        ]));
                        f.render_widget(line, outer[1]);
                    }
                    Some(InputMode::Search(buf)) => {
                        let line = Paragraph::new(Line::from(vec![
                            Span::styled(
                                "/",
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(buf.as_str()),
                            Span::styled(
                                "█",
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::SLOW_BLINK),
                            ),
                        ]));
                        f.render_widget(line, outer[1]);
                    }
                    Some(InputMode::Annotation { step_idx, buffer }) => {
                        let line = Paragraph::new(Line::from(vec![
                            Span::styled(
                                format!("note step {}> ", step_idx + 1),
                                Style::default()
                                    .fg(Color::Magenta)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(buffer.as_str()),
                            Span::styled(
                                "█",
                                Style::default()
                                    .fg(Color::Magenta)
                                    .add_modifier(Modifier::SLOW_BLINK),
                            ),
                        ]));
                        f.render_widget(line, outer[1]);
                    }
                    None => {
                        if let Some(msg) = &app.status_msg {
                            let line = Paragraph::new(Line::from(vec![Span::styled(
                                msg.as_str(),
                                Style::default().fg(Color::Red),
                            )]));
                            f.render_widget(line, outer[1]);
                        } else {
                            let ratio = if total == 0 {
                                0.0
                            } else {
                                #[allow(clippy::cast_precision_loss)]
                                let r = current as f64 / total as f64;
                                r.clamp(0.0, 1.0)
                            };
                            let mut parts = vec![format!("{current}/{total}")];
                            if let Some(q) = &app.filter {
                                parts.push(format!("filter: {q}"));
                            }
                            if let Some(q) = &app.search {
                                parts.push(format!("search: {q} ({})", app.search_matches.len()));
                            }
                            if !app.count_buffer.is_empty() {
                                parts.push(format!("×{}", app.count_buffer));
                            }
                            if !app.no_cost
                                && let Some(c) = app.session_totals.cost_usd
                            {
                                parts.push(format!("cost: ${c:.4}"));
                            }
                            let label = parts.join("  ");
                            let gauge = Gauge::default()
                                .gauge_style(
                                    Style::default()
                                        .fg(Color::Cyan)
                                        .bg(Color::Reset)
                                        .add_modifier(Modifier::BOLD),
                                )
                                .ratio(ratio)
                                .label(label);
                            f.render_widget(gauge, outer[1]);
                        }
                    }
                }
            }

            if app.show_help {
                let help_lines = vec![
                    Line::from(Span::styled(
                        "agx — step-through debugger for AI agent traces",
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Navigation",
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from("  ↓ / j           next step"),
                    Line::from("  ↑ / k           prev step"),
                    Line::from("  PgDn / d        jump 10 steps forward"),
                    Line::from("  PgUp / u        jump 10 steps back"),
                    Line::from("  Home / g        first step"),
                    Line::from("  End  / G        last step"),
                    Line::from("  :N              jump to visible row N"),
                    Line::from("  :@<duration>    jump to first step ≥ offset from session start (1h30m, 5m, 90s)"),
                    Line::from("  <N><motion>     vim count prefix (3j, 5k, 2d, 42G, ...)"),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Filter",
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from("  f               open filter prompt (hides non-matching rows)"),
                    Line::from("  (empty enter)   clear current filter"),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Search",
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from("  /               open search prompt (highlights matches)"),
                    Line::from("  //query         semantic search (opt-in; --features embedding-search)"),
                    Line::from("  n               next match"),
                    Line::from("  N               prev match"),
                    Line::from("  (empty enter)   clear current search"),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Bookmarks",
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from("  m<char>         set bookmark at current step"),
                    Line::from("  '<char>         jump to bookmark"),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Other",
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from("  a               add / edit / clear annotation on current step (saved to ~/.agx/notes/)"),
                    Line::from("  A               list all annotations (Enter jumps to step, Esc closes)"),
                    Line::from("  b               list all fork roots (Claude Code edit/resume branches)"),
                    Line::from("  y               copy current step to clipboard"),
                    Line::from("  h               toggle heatmap mode (tool-call density)"),
                    Line::from("  ? / F1          toggle this help"),
                    Line::from("  s               toggle tool usage stats overlay"),
                    Line::from("  Tab             toggle 3-pane / 2-pane layout"),
                    Line::from("  mouse click     select row in timeline"),
                    Line::from("  mouse scroll    prev / next step"),
                    Line::from("  q / Esc         quit"),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Color legend",
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled("cyan   ", Style::default().fg(Color::Cyan)),
                        Span::raw("user message"),
                    ]),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled("green  ", Style::default().fg(Color::Green)),
                        Span::raw("assistant message"),
                    ]),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled("yellow ", Style::default().fg(Color::Yellow)),
                        Span::raw("tool_use (alternating bg per call)"),
                    ]),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled("magenta", Style::default().fg(Color::Magenta)),
                        Span::raw(" tool_result (alternating bg per result)"),
                    ]),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            "red    ",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("error (failed tool call, heuristic)"),
                    ]),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled("║      ", Style::default().fg(Color::DarkGray)),
                        Span::raw("part of a batched parallel tool call"),
                    ]),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            "*      ",
                            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("has an annotation (see `a` keybinding)"),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Press any key to dismiss",
                        Style::default().fg(Color::DarkGray),
                    )),
                ];

                let help_height = u16::try_from(help_lines.len())
                    .unwrap_or(u16::MAX)
                    .saturating_add(2);
                let help_area = centered_rect(HELP_POPUP_WIDTH, help_height, f.area());

                let help_widget = Paragraph::new(help_lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" help ")
                        .border_style(Style::default().fg(Color::White)),
                );

                f.render_widget(Clear, help_area);
                f.render_widget(help_widget, help_area);
            }

            if app.show_stats {
                let mut lines = vec![
                    Line::from(Span::styled(
                        "agx — tool usage statistics",
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(format!(
                        "Total steps: {}   Unique tools: {}",
                        app.steps.len(),
                        app.tool_stats.len()
                    )),
                ];
                if app.session_totals.has_tokens() {
                    lines.push(Line::from(format!(
                        "Tokens — in: {}, out: {}, cache_read: {}, cache_create: {}",
                        app.session_totals.tokens_in,
                        app.session_totals.tokens_out,
                        app.session_totals.cache_read,
                        app.session_totals.cache_create,
                    )));
                    if !app.session_totals.unique_models.is_empty() {
                        lines.push(Line::from(format!(
                            "Models: {}",
                            app.session_totals.unique_models.join(", ")
                        )));
                    }
                    if !app.no_cost {
                        match app.session_totals.cost_usd {
                            Some(c) => {
                                lines.push(Line::from(format!("Estimated cost: ${c:.4} USD")))
                            }
                            None => lines.push(Line::from(Span::styled(
                                "Estimated cost: (no pricing entry for model)",
                                Style::default().fg(Color::DarkGray),
                            ))),
                        }
                    }
                }
                lines.extend([
                    Line::from(""),
                    Line::from(Span::styled(
                        format!(
                            "{:<22} {:>6} {:>8} {:>8} {:>9}",
                            "Tool", "uses", "results", "errors", "err%"
                        ),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                ]);
                for s in app.tool_stats.iter().take(18) {
                    let err_pct = match s.error_rate() {
                        Some(r) => format!("{:>7.1}%", r * 100.0),
                        None => "      -".to_string(),
                    };
                    let err_color = if s.error_count > 0 {
                        Color::Red
                    } else {
                        Color::White
                    };
                    lines.push(Line::from(vec![Span::styled(
                        format!(
                            "{:<22} {:>6} {:>8} {:>8} {:>9}",
                            truncate(&s.name, 22),
                            s.use_count,
                            s.result_count,
                            s.error_count,
                            err_pct
                        ),
                        Style::default().fg(err_color),
                    )]));
                }
                if app.tool_stats.len() > 18 {
                    lines.push(Line::from(Span::styled(
                        format!("... ({} more not shown)", app.tool_stats.len() - 18),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Press any key to dismiss",
                    Style::default().fg(Color::DarkGray),
                )));

                let height = u16::try_from(lines.len())
                    .unwrap_or(u16::MAX)
                    .saturating_add(2);
                let area = centered_rect(70, height, f.area());
                let widget = Paragraph::new(lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" stats ")
                        .border_style(Style::default().fg(Color::White)),
                );
                f.render_widget(Clear, area);
                f.render_widget(widget, area);
            }

            if app.show_annotations {
                // Collect once into an owned vec so the ListState navigation
                // and the row rendering see identical ordering.
                let notes: Vec<(usize, String, String)> = app
                    .annotations
                    .iter()
                    .map(|(idx, note)| {
                        let label = app
                            .steps
                            .get(idx)
                            .map(|s| s.label.clone())
                            .unwrap_or_else(|| "(missing step)".into());
                        (idx, label, note.text.clone())
                    })
                    .collect();
                let count = notes.len();
                let items: Vec<ListItem> = if count == 0 {
                    vec![ListItem::new(Line::from(Span::styled(
                        "  (no annotations — press `a` on a step to add one)",
                        Style::default().fg(Color::DarkGray),
                    )))]
                } else {
                    notes
                        .iter()
                        .map(|(idx, label, text)| {
                            let preview = truncate(text, 60);
                            ListItem::new(vec![
                                Line::from(vec![
                                    Span::styled(
                                        format!("step {:>4}  ", idx + 1),
                                        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                                    ),
                                    Span::styled(
                                        truncate(label, 50),
                                        Style::default().fg(Color::White),
                                    ),
                                ]),
                                Line::from(Span::styled(
                                    format!("           {preview}"),
                                    Style::default().fg(Color::Gray),
                                )),
                            ])
                        })
                        .collect()
                };
                let title = format!(" annotations ({count}) — Enter jumps · Esc closes ");
                // 4 lines of chrome (border top + border bottom + header + footer hint) +
                // 2 lines per note (label + preview); 1 line when empty.
                let body_lines = if count == 0 { 1 } else { count * 2 };
                let height = u16::try_from(body_lines + 4)
                    .unwrap_or(u16::MAX)
                    .clamp(6, 20);
                let area = centered_rect(80, height, f.area());
                let list = List::new(items)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(title)
                            .border_style(Style::default().fg(Color::Magenta)),
                    )
                    .highlight_style(
                        Style::default()
                            .add_modifier(Modifier::REVERSED)
                            .add_modifier(Modifier::BOLD),
                    );
                f.render_widget(Clear, area);
                f.render_stateful_widget(list, area, &mut app.annotations_list_state);
            }

            if app.show_forks {
                let count = app.fork_indices.len();
                let items: Vec<ListItem> = if count == 0 {
                    vec![ListItem::new(Line::from(Span::styled(
                        "  (no forks — this session is linear)",
                        Style::default().fg(Color::DarkGray),
                    )))]
                } else {
                    app.fork_indices
                        .iter()
                        .map(|&orig_idx| {
                            let label = app
                                .steps
                                .get(orig_idx)
                                .map(|s| truncate(&s.label, 60))
                                .unwrap_or_else(|| "(missing step)".into());
                            ListItem::new(Line::from(vec![
                                Span::styled(
                                    format!("step {:>4}  ", orig_idx + 1),
                                    Style::default()
                                        .fg(Color::DarkGray)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(label, Style::default().fg(Color::White)),
                            ]))
                        })
                        .collect()
                };
                let title = format!(" forks ({count}) — Enter jumps · Esc closes ");
                // 4 lines of chrome + 1 line per fork; 1 line when empty.
                let body_lines = if count == 0 { 1 } else { count };
                let height = u16::try_from(body_lines + 4)
                    .unwrap_or(u16::MAX)
                    .clamp(6, 20);
                let area = centered_rect(78, height, f.area());
                let list = List::new(items)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(title)
                            .border_style(Style::default().fg(Color::DarkGray)),
                    )
                    .highlight_style(
                        Style::default()
                            .add_modifier(Modifier::REVERSED)
                            .add_modifier(Modifier::BOLD),
                    );
                f.render_widget(Clear, area);
                f.render_stateful_widget(list, area, &mut app.forks_list_state);
            }
        })?;

        let poll_timeout = if reload_fn.is_some() {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(60)
        };
        if !event::poll(poll_timeout)? {
            continue;
        }
        let ev = event::read()?;
        if let Event::Mouse(mouse) = ev {
            match mouse.kind {
                MouseEventKind::ScrollUp => app.prev(),
                MouseEventKind::ScrollDown => app.next(),
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(area) = app.list_area
                        && mouse.row > area.y
                        && mouse.row < area.y + area.height - 1
                        && mouse.column > area.x
                        && mouse.column < area.x + area.width - 1
                    {
                        // Inside the list, excluding the border.
                        let row_within_list = usize::from(mouse.row - area.y - 1);
                        let view_idx = app.list_state.offset() + row_within_list;
                        app.click_to_select(view_idx);
                    }
                }
                _ => {}
            }
            continue;
        }
        if let Event::Key(key) = ev
            && key.kind == KeyEventKind::Press
        {
            // Help overlay: any key dismisses.
            if app.show_help {
                app.show_help = false;
                continue;
            }

            // Stats overlay: any key dismisses.
            if app.show_stats {
                app.show_stats = false;
                continue;
            }

            // Annotations overlay: navigable — j/k/arrows move, Enter
            // jumps to the selected note's step, any other key closes.
            if app.show_annotations {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.annotations_cursor_move(1),
                    KeyCode::Up | KeyCode::Char('k') => app.annotations_cursor_move(-1),
                    KeyCode::Enter => app.jump_to_selected_annotation(),
                    _ => app.show_annotations = false,
                }
                continue;
            }

            // Fork-list overlay: same nav contract as annotations.
            if app.show_forks {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => app.forks_cursor_move(1),
                    KeyCode::Up | KeyCode::Char('k') => app.forks_cursor_move(-1),
                    KeyCode::Enter => app.jump_to_selected_fork(),
                    _ => app.show_forks = false,
                }
                continue;
            }

            // Pending bookmark key (m<char> or '<char>): consume the next event.
            if let Some(pending) = app.pending {
                if let KeyCode::Char(c) = key.code {
                    match pending {
                        PendingKey::SetMark => app.set_mark(c),
                        PendingKey::JumpMark => app.jump_to_mark(c),
                    }
                }
                app.cancel_pending();
                continue;
            }

            // Input mode (command / filter / search): its own keybinding scope.
            if app.input_mode.is_some() {
                match key.code {
                    KeyCode::Esc => {
                        app.input_mode = None;
                        app.status_msg = None;
                    }
                    KeyCode::Enter => {
                        let mode = app.input_mode.take();
                        match mode {
                            Some(InputMode::Command(buf)) => app.execute_command(&buf),
                            Some(InputMode::Filter(buf)) => app.apply_filter(&buf),
                            Some(InputMode::Search(buf)) => app.apply_search(&buf),
                            Some(InputMode::Annotation { step_idx, buffer }) => {
                                app.save_annotation(step_idx, &buffer);
                            }
                            None => {}
                        }
                    }
                    KeyCode::Backspace => {
                        if let Some(
                            InputMode::Command(buf)
                            | InputMode::Filter(buf)
                            | InputMode::Search(buf)
                            | InputMode::Annotation { buffer: buf, .. },
                        ) = &mut app.input_mode
                        {
                            buf.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Some(
                            InputMode::Command(buf)
                            | InputMode::Filter(buf)
                            | InputMode::Search(buf)
                            | InputMode::Annotation { buffer: buf, .. },
                        ) = &mut app.input_mode
                        {
                            buf.push(c);
                        }
                    }
                    _ => {}
                }
                continue;
            }

            // Normal mode.
            app.status_msg = None;

            // Vim-style count prefix: digits 1-9 always start a count; 0 joins
            // an existing count (otherwise it's just an unbound key).
            if let KeyCode::Char(c @ '0'..='9') = key.code
                && (c != '0' || app.has_count())
            {
                app.append_count_digit(c);
                continue;
            }

            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('?') | KeyCode::F(1) => {
                    app.clear_count();
                    app.toggle_help();
                }
                KeyCode::Char(':') => {
                    app.clear_count();
                    app.enter_command_mode();
                }
                KeyCode::Char('f') => {
                    app.clear_count();
                    app.enter_filter_mode();
                }
                KeyCode::Char('/') => {
                    app.clear_count();
                    app.enter_search_mode();
                }
                KeyCode::Char('a') => {
                    app.clear_count();
                    app.enter_annotation_mode();
                }
                KeyCode::Char('A') => {
                    app.clear_count();
                    app.toggle_annotations_list();
                }
                KeyCode::Char('R') => {
                    // Phase 5.4 — experimental tool-call replay.
                    // Requires `--experimental-replay` and (for
                    // Bash) `--allow-shell-replay`. Per-invocation
                    // confirm via `y` in the status bar.
                    app.clear_count();
                    if let Some(view_idx) = app.list_state.selected()
                        && let Some(&orig) = app.filtered_view.get(view_idx)
                        && let Some(step) = app.steps.get(orig)
                    {
                        match crate::replay::classify(step, &replay) {
                            crate::replay::ReplayIntent::NeedsConfirm { input } => {
                                let preview = input.chars().take(60).collect::<String>();
                                app.status_msg = Some(format!(
                                    "replay step {}: `{preview}` — press y to run, Esc to cancel",
                                    orig + 1
                                ));
                                pending_replay = Some((orig, input));
                            }
                            crate::replay::ReplayIntent::FlagMissing { hint } => {
                                app.status_msg = Some(format!("replay blocked: {hint}"));
                            }
                            crate::replay::ReplayIntent::NotReplayable { reason } => {
                                app.status_msg = Some(format!("replay skipped: {reason}"));
                            }
                        }
                    }
                }
                KeyCode::Char('y') if pending_replay.is_some() => {
                    let (step_idx, input) = pending_replay.take().expect("checked");
                    app.status_msg = Some(format!("running replay for step {}…", step_idx + 1));
                    match crate::replay::execute_shell(&input) {
                        Ok(output) => {
                            let exit = output
                                .exit_code
                                .map_or_else(|| "signal".to_string(), |c| c.to_string());
                            // Best-effort log. A write failure
                            // shouldn't block the user from seeing
                            // the result in the status bar.
                            let log_result = if let Some(path) = session_path
                                && let Some(step) = app.steps.get(step_idx)
                            {
                                crate::replay::log_replay(path, step_idx, step, &input, &output)
                            } else {
                                Ok(())
                            };
                            let log_note = match log_result {
                                Ok(()) => "logged",
                                Err(_) => "log failed",
                            };
                            app.status_msg = Some(format!(
                                "replay exit={exit} in {}ms ({log_note}); {}B stdout",
                                output.duration_ms,
                                output.stdout.len(),
                            ));
                        }
                        Err(e) => {
                            app.status_msg = Some(format!("replay failed: {e}"));
                        }
                    }
                }
                KeyCode::Char('b') => {
                    app.clear_count();
                    app.toggle_forks_list();
                }
                KeyCode::Char('n') => {
                    let n = app.take_count();
                    for _ in 0..n {
                        app.next_match();
                    }
                }
                KeyCode::Char('N') => {
                    let n = app.take_count();
                    for _ in 0..n {
                        app.prev_match();
                    }
                }
                KeyCode::Char('y') => {
                    app.clear_count();
                    app.copy_current_step();
                }
                KeyCode::Char('h') => {
                    app.clear_count();
                    app.toggle_heatmap();
                }
                KeyCode::Char('s') => {
                    app.clear_count();
                    app.toggle_stats();
                }
                KeyCode::Char('m') => {
                    app.clear_count();
                    app.begin_set_mark();
                }
                KeyCode::Char('\'') => {
                    app.clear_count();
                    app.begin_jump_mark();
                }
                KeyCode::Tab => {
                    app.clear_count();
                    app.toggle_layout();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let n = app.take_count();
                    for _ in 0..n {
                        app.next();
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let n = app.take_count();
                    for _ in 0..n {
                        app.prev();
                    }
                }
                KeyCode::PageDown | KeyCode::Char('d') => {
                    let n = app.take_count();
                    app.page_down(PAGE_STEP * n);
                }
                KeyCode::PageUp | KeyCode::Char('u') => {
                    let n = app.take_count();
                    app.page_up(PAGE_STEP * n);
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    app.clear_count();
                    app.home();
                }
                KeyCode::End | KeyCode::Char('G') => {
                    // 5G in vim = jump to line 5; plain G = last line.
                    if app.has_count() {
                        let n = app.take_count();
                        app.goto_step(n);
                    } else {
                        app.end();
                    }
                }
                _ => app.clear_count(),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{assistant_text_step, tool_result_step, tool_use_step, user_text_step};

    #[test]
    fn bg_flags_alternate_on_tool_use_and_tool_result() {
        let steps = vec![
            user_text_step("hi"),
            tool_use_step("t1", "Read", "{}"),
            tool_result_step("t1", "ok", Some("Read"), Some("{}")),
            tool_use_step("t2", "Bash", "{}"),
            tool_result_step("t2", "ok", Some("Bash"), Some("{}")),
            tool_use_step("t3", "Edit", "{}"),
            tool_result_step("t3", "ok", Some("Edit"), Some("{}")),
            assistant_text_step("done"),
        ];
        let flags = compute_bg_flags(&steps);
        assert!(!flags[0]);
        assert!(!flags[1]);
        assert!(!flags[2]);
        assert!(flags[3]);
        assert!(flags[4]);
        assert!(!flags[5]);
        assert!(!flags[6]);
        assert!(!flags[7]);
    }

    #[test]
    fn bg_flags_empty_for_empty_steps() {
        let flags = compute_bg_flags(&[]);
        assert!(flags.is_empty());
    }

    fn sample_steps() -> Vec<Step> {
        vec![
            user_text_step("write a fibonacci function"),
            tool_use_step("t1", "Read", "{}"),
            tool_result_step(
                "t1",
                "def fib(n):\n    return ...",
                Some("Read"),
                Some("{}"),
            ),
            tool_use_step("t2", "Bash", "{}"),
            tool_result_step("t2", "0 1 1 2 3 5", Some("Bash"), Some("{}")),
            assistant_text_step("done"),
        ]
    }

    #[test]
    fn goto_step_selects_valid_index() {
        let mut app = App::new(sample_steps(), false);
        app.goto_step(2);
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn goto_step_clamps_out_of_bounds() {
        let mut app = App::new(sample_steps(), false);
        app.goto_step(999);
        assert_eq!(app.list_state.selected(), Some(5));
    }

    #[test]
    fn goto_step_rejects_zero() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(0));
        app.goto_step(0);
        assert_eq!(app.list_state.selected(), Some(0));
        assert!(app.status_msg.as_ref().unwrap().contains(">= 1"));
    }

    #[test]
    fn execute_command_parses_number() {
        let mut app = App::new(sample_steps(), false);
        app.execute_command("3");
        assert_eq!(app.list_state.selected(), Some(2));
    }

    #[test]
    fn execute_command_ignores_empty_input() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(0));
        app.execute_command("   ");
        assert_eq!(app.list_state.selected(), Some(0));
        assert!(app.status_msg.is_none());
    }

    #[test]
    fn execute_command_reports_unknown() {
        let mut app = App::new(sample_steps(), false);
        app.execute_command("nope");
        assert!(app.status_msg.as_ref().unwrap().contains("unknown"));
    }

    #[test]
    fn execute_command_at_duration_jumps_by_time_offset() {
        let mut steps = sample_steps();
        // Give the first three steps real timestamps 0 / 5s / 10s
        // relative to a synthetic session start.
        let base = 1_000_000u64;
        steps[0].timestamp_ms = Some(base);
        steps[1].timestamp_ms = Some(base + 5_000);
        steps[2].timestamp_ms = Some(base + 10_000);
        let mut app = App::new(steps, false);
        app.execute_command("@5s");
        // Step 1 is at +5s → its 0-based index is 1.
        assert_eq!(app.list_state.selected(), Some(1));
        app.execute_command("@10s");
        assert_eq!(app.list_state.selected(), Some(2));
    }

    #[test]
    fn execute_command_at_duration_without_timestamps_is_informative() {
        // sample_steps() has no timestamps by default — the jump
        // should not silently succeed; it should surface a message.
        let mut app = App::new(sample_steps(), false);
        app.execute_command("@1h");
        let msg = app.status_msg.as_ref().expect("expected a status message");
        assert!(
            msg.contains("no step timestamps") || msg.contains("unavailable"),
            "unexpected status: {msg}"
        );
    }

    #[test]
    fn execute_command_at_duration_past_end_reports_no_match() {
        let mut steps = sample_steps();
        steps[0].timestamp_ms = Some(1_000_000);
        let mut app = App::new(steps, false);
        app.execute_command("@1h");
        // No step has timestamp >= start + 1h → status message, no
        // selection change.
        let msg = app.status_msg.as_ref().expect("expected a status message");
        assert!(msg.contains("no step at-or-after"), "unexpected: {msg}");
    }

    #[test]
    fn execute_command_rejects_bad_duration_spelling() {
        let mut steps = sample_steps();
        steps[0].timestamp_ms = Some(1_000_000);
        let mut app = App::new(steps, false);
        app.execute_command("@5x");
        assert!(app.status_msg.as_ref().unwrap().contains("bad time offset"));
    }

    #[test]
    fn apply_filter_by_tool_name_substring_case_insensitive() {
        let mut app = App::new(sample_steps(), false);
        app.apply_filter("read");
        assert_eq!(app.visible_count(), 2);
        assert_eq!(app.filter.as_deref(), Some("read"));
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[test]
    fn apply_filter_by_kind_prefix() {
        let mut app = App::new(sample_steps(), false);
        app.apply_filter("[tool]");
        assert_eq!(app.visible_count(), 2);
    }

    #[test]
    fn apply_filter_empty_clears_existing_filter() {
        let mut app = App::new(sample_steps(), false);
        app.apply_filter("Read");
        assert_eq!(app.visible_count(), 2);
        app.apply_filter("");
        assert_eq!(app.visible_count(), 6);
        assert!(app.filter.is_none());
    }

    #[test]
    fn apply_filter_no_matches_keeps_previous_view_and_sets_error() {
        let mut app = App::new(sample_steps(), false);
        app.apply_filter("nonexistent");
        assert_eq!(app.visible_count(), 6);
        assert!(app.filter.is_none());
        assert!(app.status_msg.as_ref().unwrap().contains("no matches"));
    }

    #[test]
    fn clear_filter_restores_full_view() {
        let mut app = App::new(sample_steps(), false);
        app.apply_filter("Read");
        app.clear_filter();
        assert_eq!(app.visible_count(), 6);
        assert!(app.filter.is_none());
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[test]
    fn navigation_under_filter_operates_on_filtered_view() {
        let mut app = App::new(sample_steps(), false);
        app.apply_filter("[tool]");
        assert_eq!(app.visible_count(), 2);
        assert_eq!(app.list_state.selected(), Some(0));
        app.next();
        assert_eq!(app.list_state.selected(), Some(1));
        app.next();
        assert_eq!(app.list_state.selected(), Some(1));
        app.home();
        assert_eq!(app.list_state.selected(), Some(0));
        app.end();
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn goto_step_under_filter_uses_visible_positions() {
        let mut app = App::new(sample_steps(), false);
        app.apply_filter("[tool]");
        app.goto_step(2);
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn apply_search_finds_matches_in_label_and_detail() {
        let mut app = App::new(sample_steps(), false);
        app.apply_search("fib");
        // "fibonacci" in user text + "fib(n)" in Read result = 2 matches
        assert_eq!(app.search_matches.len(), 2);
        assert_eq!(app.search.as_deref(), Some("fib"));
    }

    #[test]
    fn apply_search_case_insensitive() {
        let mut app = App::new(sample_steps(), false);
        app.apply_search("FIBONACCI");
        assert_eq!(app.search_matches.len(), 1);
    }

    #[test]
    fn apply_search_jumps_to_first_match_at_or_after_current() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(2));
        app.apply_search("fib");
        // Current is step 2 (tool_use Read). Matches: step 0 (user text) and step 2 (Read result).
        // Jump should go to step 2 (the at-or-after match)
        assert_eq!(app.list_state.selected(), Some(2));
    }

    #[test]
    fn apply_search_empty_clears() {
        let mut app = App::new(sample_steps(), false);
        app.apply_search("fib");
        assert!(!app.search_matches.is_empty());
        app.apply_search("");
        assert!(app.search.is_none());
        assert!(app.search_matches.is_empty());
    }

    #[test]
    fn apply_search_no_matches_sets_error() {
        let mut app = App::new(sample_steps(), false);
        app.apply_search("zzzzz");
        assert!(app.search.is_none());
        assert!(app.search_matches.is_empty());
        assert!(app.status_msg.as_ref().unwrap().contains("no matches"));
    }

    #[test]
    fn next_match_advances_and_wraps() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(0));
        app.apply_search("fib"); // matches 0 and 2
        assert_eq!(app.list_state.selected(), Some(0));
        app.next_match();
        assert_eq!(app.list_state.selected(), Some(2));
        app.next_match(); // wrap to first
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[test]
    fn prev_match_goes_back_and_wraps() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(0));
        app.apply_search("fib");
        assert_eq!(app.list_state.selected(), Some(0));
        app.prev_match(); // wrap to last match
        assert_eq!(app.list_state.selected(), Some(2));
        app.prev_match();
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[test]
    fn search_respects_active_filter() {
        let mut app = App::new(sample_steps(), false);
        app.apply_filter("[tool]"); // leaves only steps 1 and 3 in filtered_view
        app.apply_search("Read"); // should find step 1 (Read tool_use), position 0 in filtered_view
        assert_eq!(app.search_matches.len(), 1);
        assert_eq!(app.search_matches[0], 0);
    }

    #[test]
    fn clear_search_removes_highlights() {
        let mut app = App::new(sample_steps(), false);
        app.apply_search("fib");
        assert!(!app.search_matches.is_empty());
        app.clear_search();
        assert!(app.search.is_none());
        assert!(app.search_matches.is_empty());
    }

    #[test]
    fn search_matches_recompute_when_filter_changes() {
        let mut app = App::new(sample_steps(), false);
        app.apply_search("fib"); // matches 2 steps in unfiltered view
        assert_eq!(app.search_matches.len(), 2);
        app.apply_filter("[tool]"); // filtered_view is now just the 2 tool steps
        // "fib" matches step 1 (Read result contains "fib(n)") but Read result is
        // not in [tool] filter (it's [result]), and user text not in filter either.
        // So 0 matches now.
        assert_eq!(app.search_matches.len(), 0);
    }

    #[test]
    fn set_mark_stores_current_step_by_char() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(3));
        app.set_mark('a');
        assert_eq!(app.bookmarks.get(&'a').copied(), Some(3));
        assert!(app.status_msg.as_ref().unwrap().contains("set at step 4"));
    }

    #[test]
    fn jump_to_mark_restores_position() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(2));
        app.set_mark('x');
        app.list_state.select(Some(0));
        app.jump_to_mark('x');
        assert_eq!(app.list_state.selected(), Some(2));
    }

    #[test]
    fn jump_to_mark_unknown_char_sets_error() {
        let mut app = App::new(sample_steps(), false);
        app.jump_to_mark('z');
        assert!(app.status_msg.as_ref().unwrap().contains("no bookmark 'z'"));
    }

    #[test]
    fn overwriting_a_bookmark_replaces_position() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(1));
        app.set_mark('a');
        app.list_state.select(Some(4));
        app.set_mark('a');
        assert_eq!(app.bookmarks.get(&'a').copied(), Some(4));
    }

    #[test]
    fn multiple_distinct_bookmarks_coexist() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(1));
        app.set_mark('a');
        app.list_state.select(Some(3));
        app.set_mark('b');
        app.list_state.select(Some(5));
        app.set_mark('c');
        app.list_state.select(Some(0));
        app.jump_to_mark('b');
        assert_eq!(app.list_state.selected(), Some(3));
        app.jump_to_mark('a');
        assert_eq!(app.list_state.selected(), Some(1));
        app.jump_to_mark('c');
        assert_eq!(app.list_state.selected(), Some(5));
    }

    #[test]
    fn bookmark_survives_filter_cycle() {
        let mut app = App::new(sample_steps(), false);
        // Bookmark step 4 (Bash tool_use, original index 3)
        app.list_state.select(Some(3));
        app.set_mark('b');
        // Apply a filter that still includes the bookmarked step
        app.apply_filter("[tool]");
        assert_eq!(app.visible_count(), 2);
        // Bookmark step's original index (3) must be re-findable in filtered_view
        app.list_state.select(Some(0));
        app.jump_to_mark('b');
        // In the filtered view, original step 3 is at filtered position 1
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn jump_to_mark_reports_hidden_by_filter() {
        let mut app = App::new(sample_steps(), false);
        // Bookmark user text at step 0
        app.list_state.select(Some(0));
        app.set_mark('u');
        // Filter away user text
        app.apply_filter("[tool]");
        app.jump_to_mark('u');
        assert!(
            app.status_msg
                .as_ref()
                .unwrap()
                .contains("hidden by filter")
        );
    }

    #[test]
    fn cancel_pending_clears_state() {
        let mut app = App::new(sample_steps(), false);
        app.begin_set_mark();
        assert_eq!(app.pending, Some(PendingKey::SetMark));
        app.cancel_pending();
        assert_eq!(app.pending, None);
    }

    #[test]
    fn click_to_select_sets_index() {
        let mut app = App::new(sample_steps(), false);
        app.click_to_select(3);
        assert_eq!(app.list_state.selected(), Some(3));
    }

    #[test]
    fn click_to_select_clamps_out_of_bounds() {
        let mut app = App::new(sample_steps(), false);
        app.click_to_select(999);
        assert_eq!(app.list_state.selected(), Some(5));
    }

    #[test]
    fn click_to_select_respects_filter() {
        let mut app = App::new(sample_steps(), false);
        app.apply_filter("[tool]");
        assert_eq!(app.visible_count(), 2);
        app.click_to_select(1);
        assert_eq!(app.list_state.selected(), Some(1));
        app.click_to_select(5); // beyond filtered view — clamp
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn conversation_indices_include_only_text_steps() {
        let steps = sample_steps();
        // sample_steps: user, tool_use, tool_result, tool_use, tool_result, assistant
        let indices = compute_conversation_indices(&steps);
        assert_eq!(indices, vec![0, 5]);
    }

    #[test]
    fn conversation_indices_empty_for_no_text_steps() {
        let steps = vec![
            tool_use_step("t1", "Read", "{}"),
            tool_result_step("t1", "ok", Some("Read"), Some("{}")),
        ];
        let indices = compute_conversation_indices(&steps);
        assert!(indices.is_empty());
    }

    #[test]
    fn sync_conversation_cursor_on_user_step_selects_user() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(0));
        app.sync_conversation_cursor();
        assert_eq!(app.conversation_list_state.selected(), Some(0));
    }

    #[test]
    fn sync_conversation_cursor_on_tool_step_selects_prior_text() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(3)); // tool_use Bash
        app.sync_conversation_cursor();
        // orig=3; rposition finds largest i<=3 in [0, 5] -> 0 (user)
        assert_eq!(app.conversation_list_state.selected(), Some(0));
    }

    #[test]
    fn sync_conversation_cursor_on_final_assistant() {
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(5));
        app.sync_conversation_cursor();
        assert_eq!(app.conversation_list_state.selected(), Some(1));
    }

    #[test]
    fn toggle_layout_flips_three_pane_flag() {
        let mut app = App::new(sample_steps(), false);
        assert!(app.three_pane);
        app.toggle_layout();
        assert!(!app.three_pane);
        app.toggle_layout();
        assert!(app.three_pane);
    }

    #[test]
    fn batch_flags_mark_consecutive_tool_uses() {
        // Three tool_uses in a row followed by three tool_results — a parallel batch.
        let steps = vec![
            user_text_step("dispatch parallel"),
            tool_use_step("t1", "Agent", "{}"),
            tool_use_step("t2", "Agent", "{}"),
            tool_use_step("t3", "Agent", "{}"),
            tool_result_step("t1", "done", Some("Agent"), Some("{}")),
            tool_result_step("t2", "done", Some("Agent"), Some("{}")),
            tool_result_step("t3", "done", Some("Agent"), Some("{}")),
            assistant_text_step("synthesis"),
        ];
        let flags = compute_batch_flags(&steps);
        assert!(!flags[0]); // user text
        assert!(flags[1]); // tool_use 1 (batched)
        assert!(flags[2]); // tool_use 2 (batched)
        assert!(flags[3]); // tool_use 3 (batched)
        assert!(flags[4]); // tool_result 1 (batched)
        assert!(flags[5]); // tool_result 2 (batched)
        assert!(flags[6]); // tool_result 3 (batched)
        assert!(!flags[7]); // assistant text
    }

    #[test]
    fn batch_flags_ignore_single_tool_calls() {
        // Alternating tool_use / tool_result pattern = no batch.
        let steps = vec![
            tool_use_step("t1", "Read", "{}"),
            tool_result_step("t1", "ok", Some("Read"), Some("{}")),
            tool_use_step("t2", "Bash", "{}"),
            tool_result_step("t2", "ok", Some("Bash"), Some("{}")),
        ];
        let flags = compute_batch_flags(&steps);
        assert!(!flags[0]);
        assert!(!flags[1]);
        assert!(!flags[2]);
        assert!(!flags[3]);
    }

    #[test]
    fn batch_flags_separate_runs_correctly() {
        // First run of 2 tool_uses, then text break, then another run of 2.
        let steps = vec![
            tool_use_step("t1", "Read", "{}"),
            tool_use_step("t2", "Read", "{}"),
            assistant_text_step("..."),
            tool_use_step("t3", "Bash", "{}"),
            tool_use_step("t4", "Bash", "{}"),
        ];
        let flags = compute_batch_flags(&steps);
        assert!(flags[0]);
        assert!(flags[1]);
        assert!(!flags[2]);
        assert!(flags[3]);
        assert!(flags[4]);
    }

    #[test]
    fn batch_flags_empty_for_no_steps() {
        let flags = compute_batch_flags(&[]);
        assert!(flags.is_empty());
    }

    #[test]
    fn count_buffer_accumulates_digits() {
        let mut app = App::new(sample_steps(), false);
        app.append_count_digit('1');
        app.append_count_digit('2');
        app.append_count_digit('3');
        assert_eq!(app.count_buffer, "123");
    }

    #[test]
    fn count_buffer_rejects_non_digits() {
        let mut app = App::new(sample_steps(), false);
        app.append_count_digit('a');
        assert_eq!(app.count_buffer, "");
    }

    #[test]
    fn count_buffer_caps_length() {
        let mut app = App::new(sample_steps(), false);
        for _ in 0..10 {
            app.append_count_digit('9');
        }
        assert_eq!(app.count_buffer.len(), 6);
    }

    #[test]
    fn take_count_returns_one_when_empty() {
        let mut app = App::new(sample_steps(), false);
        assert_eq!(app.take_count(), 1);
    }

    #[test]
    fn take_count_parses_and_clears() {
        let mut app = App::new(sample_steps(), false);
        app.append_count_digit('3');
        app.append_count_digit('7');
        assert_eq!(app.take_count(), 37);
        assert!(app.count_buffer.is_empty());
    }

    #[test]
    fn take_count_never_returns_zero() {
        let mut app = App::new(sample_steps(), false);
        app.append_count_digit('0');
        // count_buffer == "0" → parses as 0 → clamped to 1
        assert_eq!(app.take_count(), 1);
    }

    #[test]
    fn has_count_reflects_buffer_state() {
        let mut app = App::new(sample_steps(), false);
        assert!(!app.has_count());
        app.append_count_digit('5');
        assert!(app.has_count());
        app.take_count();
        assert!(!app.has_count());
    }

    #[test]
    fn count_prefix_multiplies_next_navigation() {
        // Simulate the runtime loop behavior: after digits are collected,
        // next() should be called take_count() times.
        let mut app = App::new(sample_steps(), false);
        app.list_state.select(Some(0));
        app.append_count_digit('3');
        let n = app.take_count();
        for _ in 0..n {
            app.next();
        }
        assert_eq!(app.list_state.selected(), Some(3));
    }

    #[test]
    fn toggle_annotations_list_opens_and_closes() {
        let mut app = App::new(sample_steps(), false);
        app.annotations.set(1, "note on step 2");
        assert!(!app.show_annotations);
        app.toggle_annotations_list();
        assert!(app.show_annotations);
        assert_eq!(app.annotations_list_state.selected(), Some(0));
        app.toggle_annotations_list();
        assert!(!app.show_annotations);
    }

    #[test]
    fn toggle_annotations_list_with_no_notes_opens_with_empty_selection() {
        let mut app = App::new(sample_steps(), false);
        app.toggle_annotations_list();
        assert!(app.show_annotations);
        assert_eq!(app.annotations_list_state.selected(), None);
    }

    #[test]
    fn annotations_cursor_move_clamps_to_bounds() {
        let mut app = App::new(sample_steps(), false);
        app.annotations.set(0, "a");
        app.annotations.set(2, "c");
        app.annotations.set(4, "e");
        app.toggle_annotations_list();
        assert_eq!(app.annotations_list_state.selected(), Some(0));
        app.annotations_cursor_move(1);
        assert_eq!(app.annotations_list_state.selected(), Some(1));
        app.annotations_cursor_move(10);
        assert_eq!(
            app.annotations_list_state.selected(),
            Some(2),
            "cursor should clamp at the last note"
        );
        app.annotations_cursor_move(-100);
        assert_eq!(app.annotations_list_state.selected(), Some(0));
    }

    #[test]
    fn jump_to_selected_annotation_moves_main_cursor_and_closes_overlay() {
        let mut app = App::new(sample_steps(), false);
        app.annotations.set(3, "revisit this tool call");
        app.toggle_annotations_list();
        app.jump_to_selected_annotation();
        assert!(!app.show_annotations);
        // Step 3 in the underlying steps is 0-indexed → filtered_view
        // is identity by default, so selected() should be 3.
        assert_eq!(app.list_state.selected(), Some(3));
    }

    #[cfg(not(feature = "embedding-search"))]
    #[test]
    fn semantic_prefix_without_feature_shows_rebuild_hint() {
        let mut app = App::new(sample_steps(), false);
        app.apply_search("//fibonacci");
        // Search state untouched; status explains how to enable.
        assert!(app.search.is_none());
        let msg = app.status_msg.as_deref().unwrap_or("");
        assert!(
            msg.contains("--features embedding-search"),
            "status msg should mention the feature flag, got: {msg}"
        );
    }

    #[cfg(not(feature = "embedding-search"))]
    #[test]
    fn semantic_prefix_does_not_clear_existing_string_search() {
        let mut app = App::new(sample_steps(), false);
        app.apply_search("fib"); // valid string-match search
        assert!(!app.search_matches.is_empty());
        app.apply_search("//anything");
        // Existing search preserved (feature-off path only writes status).
        assert_eq!(app.search.as_deref(), Some("fib"));
        assert!(!app.search_matches.is_empty());
    }

    #[test]
    fn empty_semantic_query_reports_error() {
        let mut app = App::new(sample_steps(), false);
        app.apply_search("//   ");
        let msg = app.status_msg.as_deref().unwrap_or("");
        assert!(msg.contains("empty semantic"), "got: {msg}");
    }

    #[test]
    fn apply_initial_step_selects_valid_index() {
        let mut app = App::new(sample_steps(), false);
        app.apply_initial_step(3);
        assert_eq!(app.list_state.selected(), Some(3));
        assert!(app.status_msg.is_none());
    }

    #[test]
    fn apply_initial_step_clamps_out_of_range_with_warning() {
        let mut app = App::new(sample_steps(), false);
        let last = sample_steps().len() - 1;
        app.apply_initial_step(999);
        assert_eq!(app.list_state.selected(), Some(last));
        let msg = app.status_msg.as_deref().unwrap_or("");
        assert!(
            msg.contains("out of range") && msg.contains("clamped"),
            "expected clamp warning, got: {msg}"
        );
    }

    #[test]
    fn apply_initial_step_zero_is_fine() {
        let mut app = App::new(sample_steps(), false);
        app.apply_initial_step(0);
        assert_eq!(app.list_state.selected(), Some(0));
        assert!(app.status_msg.is_none());
    }

    #[test]
    fn apply_initial_step_noop_on_empty_steps() {
        let mut app = App::new(Vec::new(), false);
        app.apply_initial_step(5);
        // Empty session → no selection, no status message (nothing to
        // warn about because the session is simply empty, which the
        // rest of the TUI already handles).
        assert_eq!(app.list_state.selected(), None);
    }

    #[test]
    fn toggle_forks_list_opens_and_closes() {
        let mut app = App::new(sample_steps(), false);
        // Synthetic: mark step 2 as a fork root so the overlay has content.
        app.fork_indices = vec![2];
        assert!(!app.show_forks);
        app.toggle_forks_list();
        assert!(app.show_forks);
        assert_eq!(app.forks_list_state.selected(), Some(0));
        app.toggle_forks_list();
        assert!(!app.show_forks);
    }

    #[test]
    fn toggle_forks_list_with_no_forks_opens_empty() {
        let mut app = App::new(sample_steps(), false);
        // fork_indices is empty by default for synthetic sample_steps.
        app.toggle_forks_list();
        assert!(app.show_forks);
        assert_eq!(app.forks_list_state.selected(), None);
    }

    #[test]
    fn jump_to_selected_fork_moves_main_cursor_and_closes_overlay() {
        let mut app = App::new(sample_steps(), false);
        app.fork_indices = vec![1, 3];
        app.toggle_forks_list();
        // Selected by default = 0 → fork at step 1.
        app.jump_to_selected_fork();
        assert!(!app.show_forks);
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn jump_to_filtered_fork_reports_hidden_via_status() {
        let mut app = App::new(sample_steps(), false);
        app.fork_indices = vec![3];
        // Filter to rows whose label starts with "[user]" — hides step 3.
        app.apply_filter("write");
        app.toggle_forks_list();
        app.jump_to_selected_fork();
        assert!(!app.show_forks);
        assert!(
            app.status_msg
                .as_deref()
                .unwrap_or("")
                .contains("hidden by the active filter"),
            "expected filter-hidden status, got {:?}",
            app.status_msg
        );
    }

    #[test]
    fn apply_initial_step_respects_active_filter() {
        let mut app = App::new(sample_steps(), false);
        // Filter to just tool-use rows (there are 2 in sample_steps).
        app.apply_filter("[tool]");
        assert_eq!(app.filtered_view.len(), 2);
        // Out-of-range relative to filter → clamps to last filtered row.
        app.apply_initial_step(5);
        assert_eq!(app.list_state.selected(), Some(1));
        let msg = app.status_msg.as_deref().unwrap_or("");
        assert!(
            msg.contains("2 steps"),
            "expected filtered count, got: {msg}"
        );
    }

    #[test]
    fn jump_to_filtered_annotation_reports_hidden_via_status() {
        let mut app = App::new(sample_steps(), false);
        app.annotations.set(3, "filtered");
        // Filter to rows that don't contain step 3's label — sample_steps()
        // step 3 is a tool_use for Bash; filter on "write" (user text)
        // to hide it.
        app.apply_filter("write");
        app.toggle_annotations_list();
        app.jump_to_selected_annotation();
        assert!(!app.show_annotations);
        assert!(
            app.status_msg
                .as_deref()
                .unwrap_or("")
                .contains("hidden by the active filter"),
            "expected filter-hidden status, got {:?}",
            app.status_msg
        );
    }
}
