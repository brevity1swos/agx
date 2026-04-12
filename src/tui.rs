use crate::timeline::{Step, StepKind, is_error_result};
use anyhow::Result;
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

const PAGE_STEP: usize = 10;
const HELP_POPUP_WIDTH: u16 = 64;
const ALT_BG: Color = Color::Indexed(236);
const SEARCH_HIT_BG: Color = Color::Indexed(58);

enum InputMode {
    Command(String),
    Filter(String),
    Search(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKey {
    SetMark,
    JumpMark,
}

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
}

impl App {
    pub fn new(steps: Vec<Step>) -> Self {
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
        };
        app.sync_conversation_cursor();
        app
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
        match trimmed.parse::<usize>() {
            Ok(n) => self.goto_step(n),
            Err(_) => {
                self.status_msg = Some(format!("unknown command: :{trimmed}"));
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

    fn clear_search(&mut self) {
        self.search = None;
        self.search_matches.clear();
    }

    fn recompute_search_matches(&mut self) {
        self.search_matches.clear();
        let Some(query) = self.search.as_deref() else {
            return;
        };
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
    match kind {
        StepKind::UserText => Color::Cyan,
        StepKind::AssistantText => Color::Green,
        StepKind::ToolUse => Color::Yellow,
        StepKind::ToolResult => Color::Magenta,
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

pub fn run(steps: Vec<Step>) -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(steps);

    let result = run_loop(&mut terminal, &mut app);
    let _ = terminal.show_cursor();
    result
}

// run_loop is intentionally one function — TUI render + event handling form one
// logical operation per frame; splitting hurts readability more than it helps.
#[allow(clippy::too_many_lines)]
fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
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
                    } else if app.bg_flags.get(orig_idx).copied().unwrap_or(false) {
                        style = style.bg(ALT_BG);
                    }
                    let batch_marker = if is_batched { "║ " } else { "  " };
                    Some(ListItem::new(Line::from(vec![
                        Span::styled(batch_marker, Style::default().fg(Color::DarkGray)),
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

            let (detail_text, detail_kind) = app
                .list_state
                .selected()
                .and_then(|i| app.filtered_view.get(i).copied())
                .and_then(|orig| app.steps.get(orig))
                .map_or_else(
                    || (String::new(), None),
                    |s| (s.detail.clone(), Some(s.kind)),
                );

            let detail_title = match detail_kind {
                Some(StepKind::UserText) => " user ",
                Some(StepKind::AssistantText) => " assistant ",
                Some(StepKind::ToolUse) => " tool_use ",
                Some(StepKind::ToolResult) => " tool_result ",
                None => " detail ",
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
                    Line::from("  ? / F1          toggle this help"),
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
        })?;

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
                            None => {}
                        }
                    }
                    KeyCode::Backspace => {
                        if let Some(
                            InputMode::Command(buf)
                            | InputMode::Filter(buf)
                            | InputMode::Search(buf),
                        ) = &mut app.input_mode
                        {
                            buf.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Some(
                            InputMode::Command(buf)
                            | InputMode::Filter(buf)
                            | InputMode::Search(buf),
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
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('?') | KeyCode::F(1) => app.toggle_help(),
                KeyCode::Char(':') => app.enter_command_mode(),
                KeyCode::Char('f') => app.enter_filter_mode(),
                KeyCode::Char('/') => app.enter_search_mode(),
                KeyCode::Char('n') => app.next_match(),
                KeyCode::Char('N') => app.prev_match(),
                KeyCode::Char('m') => app.begin_set_mark(),
                KeyCode::Char('\'') => app.begin_jump_mark(),
                KeyCode::Tab => app.toggle_layout(),
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.prev(),
                KeyCode::PageDown | KeyCode::Char('d') => app.page_down(PAGE_STEP),
                KeyCode::PageUp | KeyCode::Char('u') => app.page_up(PAGE_STEP),
                KeyCode::Home | KeyCode::Char('g') => app.home(),
                KeyCode::End | KeyCode::Char('G') => app.end(),
                _ => {}
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
        let mut app = App::new(sample_steps());
        app.goto_step(2);
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn goto_step_clamps_out_of_bounds() {
        let mut app = App::new(sample_steps());
        app.goto_step(999);
        assert_eq!(app.list_state.selected(), Some(5));
    }

    #[test]
    fn goto_step_rejects_zero() {
        let mut app = App::new(sample_steps());
        app.list_state.select(Some(0));
        app.goto_step(0);
        assert_eq!(app.list_state.selected(), Some(0));
        assert!(app.status_msg.as_ref().unwrap().contains(">= 1"));
    }

    #[test]
    fn execute_command_parses_number() {
        let mut app = App::new(sample_steps());
        app.execute_command("3");
        assert_eq!(app.list_state.selected(), Some(2));
    }

    #[test]
    fn execute_command_ignores_empty_input() {
        let mut app = App::new(sample_steps());
        app.list_state.select(Some(0));
        app.execute_command("   ");
        assert_eq!(app.list_state.selected(), Some(0));
        assert!(app.status_msg.is_none());
    }

    #[test]
    fn execute_command_reports_unknown() {
        let mut app = App::new(sample_steps());
        app.execute_command("nope");
        assert!(app.status_msg.as_ref().unwrap().contains("unknown"));
    }

    #[test]
    fn apply_filter_by_tool_name_substring_case_insensitive() {
        let mut app = App::new(sample_steps());
        app.apply_filter("read");
        assert_eq!(app.visible_count(), 2);
        assert_eq!(app.filter.as_deref(), Some("read"));
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[test]
    fn apply_filter_by_kind_prefix() {
        let mut app = App::new(sample_steps());
        app.apply_filter("[tool]");
        assert_eq!(app.visible_count(), 2);
    }

    #[test]
    fn apply_filter_empty_clears_existing_filter() {
        let mut app = App::new(sample_steps());
        app.apply_filter("Read");
        assert_eq!(app.visible_count(), 2);
        app.apply_filter("");
        assert_eq!(app.visible_count(), 6);
        assert!(app.filter.is_none());
    }

    #[test]
    fn apply_filter_no_matches_keeps_previous_view_and_sets_error() {
        let mut app = App::new(sample_steps());
        app.apply_filter("nonexistent");
        assert_eq!(app.visible_count(), 6);
        assert!(app.filter.is_none());
        assert!(app.status_msg.as_ref().unwrap().contains("no matches"));
    }

    #[test]
    fn clear_filter_restores_full_view() {
        let mut app = App::new(sample_steps());
        app.apply_filter("Read");
        app.clear_filter();
        assert_eq!(app.visible_count(), 6);
        assert!(app.filter.is_none());
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[test]
    fn navigation_under_filter_operates_on_filtered_view() {
        let mut app = App::new(sample_steps());
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
        let mut app = App::new(sample_steps());
        app.apply_filter("[tool]");
        app.goto_step(2);
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn apply_search_finds_matches_in_label_and_detail() {
        let mut app = App::new(sample_steps());
        app.apply_search("fib");
        // "fibonacci" in user text + "fib(n)" in Read result = 2 matches
        assert_eq!(app.search_matches.len(), 2);
        assert_eq!(app.search.as_deref(), Some("fib"));
    }

    #[test]
    fn apply_search_case_insensitive() {
        let mut app = App::new(sample_steps());
        app.apply_search("FIBONACCI");
        assert_eq!(app.search_matches.len(), 1);
    }

    #[test]
    fn apply_search_jumps_to_first_match_at_or_after_current() {
        let mut app = App::new(sample_steps());
        app.list_state.select(Some(2));
        app.apply_search("fib");
        // Current is step 2 (tool_use Read). Matches: step 0 (user text) and step 2 (Read result).
        // Jump should go to step 2 (the at-or-after match)
        assert_eq!(app.list_state.selected(), Some(2));
    }

    #[test]
    fn apply_search_empty_clears() {
        let mut app = App::new(sample_steps());
        app.apply_search("fib");
        assert!(!app.search_matches.is_empty());
        app.apply_search("");
        assert!(app.search.is_none());
        assert!(app.search_matches.is_empty());
    }

    #[test]
    fn apply_search_no_matches_sets_error() {
        let mut app = App::new(sample_steps());
        app.apply_search("zzzzz");
        assert!(app.search.is_none());
        assert!(app.search_matches.is_empty());
        assert!(app.status_msg.as_ref().unwrap().contains("no matches"));
    }

    #[test]
    fn next_match_advances_and_wraps() {
        let mut app = App::new(sample_steps());
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
        let mut app = App::new(sample_steps());
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
        let mut app = App::new(sample_steps());
        app.apply_filter("[tool]"); // leaves only steps 1 and 3 in filtered_view
        app.apply_search("Read"); // should find step 1 (Read tool_use), position 0 in filtered_view
        assert_eq!(app.search_matches.len(), 1);
        assert_eq!(app.search_matches[0], 0);
    }

    #[test]
    fn clear_search_removes_highlights() {
        let mut app = App::new(sample_steps());
        app.apply_search("fib");
        assert!(!app.search_matches.is_empty());
        app.clear_search();
        assert!(app.search.is_none());
        assert!(app.search_matches.is_empty());
    }

    #[test]
    fn search_matches_recompute_when_filter_changes() {
        let mut app = App::new(sample_steps());
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
        let mut app = App::new(sample_steps());
        app.list_state.select(Some(3));
        app.set_mark('a');
        assert_eq!(app.bookmarks.get(&'a').copied(), Some(3));
        assert!(app.status_msg.as_ref().unwrap().contains("set at step 4"));
    }

    #[test]
    fn jump_to_mark_restores_position() {
        let mut app = App::new(sample_steps());
        app.list_state.select(Some(2));
        app.set_mark('x');
        app.list_state.select(Some(0));
        app.jump_to_mark('x');
        assert_eq!(app.list_state.selected(), Some(2));
    }

    #[test]
    fn jump_to_mark_unknown_char_sets_error() {
        let mut app = App::new(sample_steps());
        app.jump_to_mark('z');
        assert!(app.status_msg.as_ref().unwrap().contains("no bookmark 'z'"));
    }

    #[test]
    fn overwriting_a_bookmark_replaces_position() {
        let mut app = App::new(sample_steps());
        app.list_state.select(Some(1));
        app.set_mark('a');
        app.list_state.select(Some(4));
        app.set_mark('a');
        assert_eq!(app.bookmarks.get(&'a').copied(), Some(4));
    }

    #[test]
    fn multiple_distinct_bookmarks_coexist() {
        let mut app = App::new(sample_steps());
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
        let mut app = App::new(sample_steps());
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
        let mut app = App::new(sample_steps());
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
        let mut app = App::new(sample_steps());
        app.begin_set_mark();
        assert_eq!(app.pending, Some(PendingKey::SetMark));
        app.cancel_pending();
        assert_eq!(app.pending, None);
    }

    #[test]
    fn click_to_select_sets_index() {
        let mut app = App::new(sample_steps());
        app.click_to_select(3);
        assert_eq!(app.list_state.selected(), Some(3));
    }

    #[test]
    fn click_to_select_clamps_out_of_bounds() {
        let mut app = App::new(sample_steps());
        app.click_to_select(999);
        assert_eq!(app.list_state.selected(), Some(5));
    }

    #[test]
    fn click_to_select_respects_filter() {
        let mut app = App::new(sample_steps());
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
        let mut app = App::new(sample_steps());
        app.list_state.select(Some(0));
        app.sync_conversation_cursor();
        assert_eq!(app.conversation_list_state.selected(), Some(0));
    }

    #[test]
    fn sync_conversation_cursor_on_tool_step_selects_prior_text() {
        let mut app = App::new(sample_steps());
        app.list_state.select(Some(3)); // tool_use Bash
        app.sync_conversation_cursor();
        // orig=3; rposition finds largest i<=3 in [0, 5] -> 0 (user)
        assert_eq!(app.conversation_list_state.selected(), Some(0));
    }

    #[test]
    fn sync_conversation_cursor_on_final_assistant() {
        let mut app = App::new(sample_steps());
        app.list_state.select(Some(5));
        app.sync_conversation_cursor();
        assert_eq!(app.conversation_list_state.selected(), Some(1));
    }

    #[test]
    fn toggle_layout_flips_three_pane_flag() {
        let mut app = App::new(sample_steps());
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
}
