//! Interactive corpus TUI. Two-pane layout: session list on the left,
//! selected-session summary on the right, corpus totals in the header,
//! keybinding hints in the footer. Driven from `agx corpus --tui <dir>`.
//!
//! Design notes:
//!
//! - **Raw mode owned by this module.** `run` sets up and tears down its
//!   own `TerminalGuard`. When the user drills into a session with
//!   Enter, the outer loop in `main.rs` tears down this TUI first, then
//!   the per-session `tui::run` sets up its own raw mode. After the
//!   per-session TUI exits, control returns here and we re-enter.
//!   Raw mode is process-global, not stackable.
//!
//! - **Sort cycle, not sort menu.** `s` cycles through mtime / cost /
//!   errors / tokens / format-then-name. Each mode is descending by
//!   the primary metric with a stable tie-break. Rendering labels the
//!   current mode in the header so users can see where they are.
//!
//! - **Keybindings mirror the per-session TUI.** j/k/g/G/Home/End/PgUp/
//!   PgDn behave the same way; ? / F1 open help; q / Esc exit. Enter
//!   (only corpus-specific) drills in. s (only corpus-specific) cycles
//!   sort. Reduces cognitive load for users who bounce between views.

use crate::corpus::{CorpusStats, ParsedSession};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

const PAGE_STEP: usize = 10;
const HELP_POPUP_WIDTH: u16 = 52;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortMode {
    MtimeDesc,
    CostDesc,
    ErrorsDesc,
    TokensDesc,
    FormatName,
}

impl SortMode {
    fn cycle(self) -> Self {
        match self {
            SortMode::MtimeDesc => SortMode::CostDesc,
            SortMode::CostDesc => SortMode::ErrorsDesc,
            SortMode::ErrorsDesc => SortMode::TokensDesc,
            SortMode::TokensDesc => SortMode::FormatName,
            SortMode::FormatName => SortMode::MtimeDesc,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SortMode::MtimeDesc => "mtime ↓",
            SortMode::CostDesc => "cost ↓",
            SortMode::ErrorsDesc => "errors ↓",
            SortMode::TokensDesc => "tokens ↓",
            SortMode::FormatName => "format / name",
        }
    }
}

struct App {
    sessions: Vec<ParsedSession>,
    list_state: ListState,
    sort: SortMode,
    show_help: bool,
    no_cost: bool,
    /// Pre-formatted header string (corpus totals) — cached since the
    /// underlying CorpusStats doesn't change during a TUI session.
    header: String,
}

impl App {
    fn new(mut sessions: Vec<ParsedSession>, stats: &CorpusStats, no_cost: bool) -> Self {
        sort_sessions(&mut sessions, SortMode::MtimeDesc);
        let mut list_state = ListState::default();
        if !sessions.is_empty() {
            list_state.select(Some(0));
        }
        App {
            sessions,
            list_state,
            sort: SortMode::MtimeDesc,
            show_help: false,
            no_cost,
            header: format_header(stats, no_cost),
        }
    }

    fn selected(&self) -> Option<&ParsedSession> {
        self.list_state
            .selected()
            .and_then(|i| self.sessions.get(i))
    }

    fn len(&self) -> usize {
        self.sessions.len()
    }

    fn next(&mut self, n: usize) {
        if self.sessions.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + n).min(self.sessions.len() - 1);
        self.list_state.select(Some(next));
    }

    fn prev(&mut self, n: usize) {
        if self.sessions.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.saturating_sub(n)));
    }

    fn home(&mut self) {
        if !self.sessions.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    fn end(&mut self) {
        if !self.sessions.is_empty() {
            self.list_state.select(Some(self.sessions.len() - 1));
        }
    }

    fn cycle_sort(&mut self) {
        // Preserve the currently-selected session's identity across
        // re-sorts — less jarring than "selection jumps to row 0 on
        // every sort".
        let selected_path = self.selected().map(|s| s.path.clone());
        self.sort = self.sort.cycle();
        sort_sessions(&mut self.sessions, self.sort);
        if let Some(path) = selected_path
            && let Some(new_idx) = self.sessions.iter().position(|s| s.path == path)
        {
            self.list_state.select(Some(new_idx));
        }
    }
}

fn format_header(stats: &CorpusStats, no_cost: bool) -> String {
    let mut parts = vec![format!("{} sessions", stats.parse_success_count)];
    if stats.parse_error_count > 0 {
        parts.push(format!("{} errored", stats.parse_error_count));
    }
    parts.push(format!("{} steps", stats.total_steps));
    parts.push(format!(
        "{} in / {} out tokens",
        stats.total_tokens_in, stats.total_tokens_out
    ));
    if !no_cost && let Some(c) = stats.total_cost_usd {
        parts.push(format!("${c:.4}"));
    }
    parts.join(" · ")
}

fn sort_sessions(sessions: &mut [ParsedSession], mode: SortMode) {
    match mode {
        SortMode::MtimeDesc => {
            sessions.sort_by(|a, b| {
                b.mtime_secs
                    .cmp(&a.mtime_secs)
                    .then_with(|| a.path.cmp(&b.path))
            });
        }
        SortMode::CostDesc => {
            sessions.sort_by(|a, b| {
                cost_key(b)
                    .partial_cmp(&cost_key(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.path.cmp(&b.path))
            });
        }
        SortMode::ErrorsDesc => {
            sessions.sort_by(|a, b| {
                error_total(b)
                    .cmp(&error_total(a))
                    .then_with(|| a.path.cmp(&b.path))
            });
        }
        SortMode::TokensDesc => {
            sessions.sort_by(|a, b| {
                token_total(b)
                    .cmp(&token_total(a))
                    .then_with(|| a.path.cmp(&b.path))
            });
        }
        SortMode::FormatName => {
            sessions.sort_by(|a, b| {
                a.format
                    .to_string()
                    .cmp(&b.format.to_string())
                    .then_with(|| a.path.cmp(&b.path))
            });
        }
    }
}

fn cost_key(s: &ParsedSession) -> f64 {
    s.totals.cost_usd.unwrap_or(0.0)
}

fn token_total(s: &ParsedSession) -> u64 {
    s.totals.tokens_in + s.totals.tokens_out
}

fn error_total(s: &ParsedSession) -> usize {
    s.tool_stats.iter().map(|t| t.error_count).sum()
}

/// Format a mtime as a short relative string. Uses the same bucketing as
/// `browser::format_relative_time` so the two views feel consistent.
fn format_relative(mtime: Option<u64>) -> String {
    let Some(m) = mtime else {
        return "?".into();
    };
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
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

fn short_path(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

/// Run the corpus TUI. Returns `Ok(Some(path))` if the user drilled into
/// a specific session (caller re-runs after the per-session TUI exits),
/// `Ok(None)` if the user quit outright.
///
/// The outer loop in `main.rs` re-invokes this after drill-in, so a
/// single call handles one "browse → pick / quit" cycle.
pub fn run(sessions: Vec<ParsedSession>, stats: &CorpusStats, no_cost: bool) -> Result<()> {
    if sessions.is_empty() {
        eprintln!("agx corpus: no sessions to display");
        return Ok(());
    }
    // Outer loop: corpus TUI → drill-in → corpus TUI → drill-in → ...
    // Raw mode is owned per-iteration so each TUI (corpus + per-session)
    // manages its own lifecycle cleanly.
    let mut current_sessions = sessions;
    loop {
        let selected = run_once(&mut current_sessions, stats, no_cost)?;
        let Some(path) = selected else {
            return Ok(());
        };
        // Drill into per-session TUI. If loading fails (format drift
        // between discovery and now, file deleted, etc.), print the
        // error to stderr and return to the corpus view rather than
        // crashing out.
        match crate::loader::load_session(&path) {
            Ok(steps) => {
                crate::tui::run(
                    steps,
                    None,
                    no_cost,
                    Some(&path),
                    None,
                    crate::tui::NotifyConfig::default(),
                )?;
            }
            Err(e) => {
                eprintln!("agx: failed to open {}: {}", path.display(), e);
            }
        }
    }
}

fn run_once(
    sessions: &mut Vec<ParsedSession>,
    stats: &CorpusStats,
    no_cost: bool,
) -> Result<Option<PathBuf>> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    // Move sessions into the App, then restore them into the caller's
    // slot before returning so the outer loop can keep using them.
    let taken = std::mem::take(sessions);
    let mut app = App::new(taken, stats, no_cost);

    let drilled = event_loop(&mut terminal, &mut app)?;
    let _ = terminal.show_cursor();

    // Hand sessions back to the caller (preserving current sort order).
    *sessions = app.sessions;
    Ok(drilled)
}

#[allow(clippy::too_many_lines)]
fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<Option<PathBuf>> {
    loop {
        terminal.draw(|f| {
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ])
                .split(f.area());

            // Header: corpus totals + current sort mode.
            let header_text = format!(
                " agx corpus — {}   [sort: {}]",
                app.header,
                app.sort.label()
            );
            f.render_widget(
                Paragraph::new(header_text).style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                outer[0],
            );

            // Main: two-pane split.
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(outer[1]);

            let items: Vec<ListItem> = app
                .sessions
                .iter()
                .map(|s| {
                    let mut line = vec![
                        Span::styled(
                            format!("[{}] ", format_tag(&s.format.to_string())),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(short_path(&s.path)),
                    ];
                    let relative = format_relative(s.mtime_secs);
                    line.push(Span::styled(
                        format!("  · {relative}"),
                        Style::default().fg(Color::DarkGray),
                    ));
                    if let Some(c) = s.totals.cost_usd
                        && !app.no_cost
                    {
                        line.push(Span::styled(
                            format!("  · ${c:.4}"),
                            Style::default().fg(Color::Yellow),
                        ));
                    }
                    let errs: usize = s.tool_stats.iter().map(|t| t.error_count).sum();
                    if errs > 0 {
                        line.push(Span::styled(
                            format!("  · {errs} err"),
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ));
                    }
                    ListItem::new(Line::from(line))
                })
                .collect();

            let list_title = format!(" sessions ({}) ", app.len());
            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(list_title))
                .highlight_style(
                    Style::default()
                        .add_modifier(Modifier::REVERSED)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_stateful_widget(list, panes[0], &mut app.list_state);

            // Right pane: selected-session summary.
            let detail_lines = detail_lines(app.selected(), app.no_cost);
            f.render_widget(
                Paragraph::new(detail_lines)
                    .block(Block::default().borders(Borders::ALL).title(" detail "))
                    .wrap(Wrap { trim: false }),
                panes[1],
            );

            // Footer: key hints.
            let hints = "j/k navigate · Enter drill · s sort · ? help · q quit";
            f.render_widget(
                Paragraph::new(hints).style(Style::default().fg(Color::DarkGray)),
                outer[2],
            );

            // Help overlay (on top of everything).
            if app.show_help {
                render_help(f);
            }
        })?;

        // Event pump. One-second poll so we return from the loop
        // reasonably quickly if something upstream wants to tear down.
        if !event::poll(Duration::from_secs(60))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if app.show_help {
            // Any key dismisses help.
            app.show_help = false;
            continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
            KeyCode::Char('?') | KeyCode::F(1) => app.show_help = true,
            KeyCode::Char('s') => app.cycle_sort(),
            KeyCode::Enter => {
                if let Some(s) = app.selected() {
                    return Ok(Some(s.path.clone()));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => app.next(1),
            KeyCode::Up | KeyCode::Char('k') => app.prev(1),
            KeyCode::PageDown | KeyCode::Char('d') => app.next(PAGE_STEP),
            KeyCode::PageUp | KeyCode::Char('u') => app.prev(PAGE_STEP),
            KeyCode::Home | KeyCode::Char('g') => app.home(),
            KeyCode::End | KeyCode::Char('G') => app.end(),
            _ => {}
        }
    }
}

fn detail_lines(selected: Option<&ParsedSession>, no_cost: bool) -> Vec<Line<'static>> {
    let Some(s) = selected else {
        return vec![Line::from("(no session selected)")];
    };
    let mut out: Vec<Line<'static>> = Vec::new();
    out.push(Line::from(Span::styled(
        format!("{}", s.path.display()),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    out.push(Line::from(format!("format: {}", s.format)));
    out.push(Line::from(format!(
        "mtime: {}",
        format_relative(s.mtime_secs)
    )));
    out.push(Line::from(""));
    out.push(Line::from(format!(
        "steps: {}  ({} in / {} out tokens)",
        s.step_count, s.totals.tokens_in, s.totals.tokens_out
    )));
    if s.totals.cache_read > 0 || s.totals.cache_create > 0 {
        out.push(Line::from(format!(
            "cache: {} read / {} create",
            s.totals.cache_read, s.totals.cache_create
        )));
    }
    if !no_cost && let Some(c) = s.totals.cost_usd {
        out.push(Line::from(Span::styled(
            format!("estimated cost: ${c:.4} USD"),
            Style::default().fg(Color::Yellow),
        )));
    }
    if !s.totals.unique_models.is_empty() {
        out.push(Line::from(format!(
            "models: {}",
            s.totals.unique_models.join(", ")
        )));
    }
    if !s.tool_stats.is_empty() {
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "Top tools:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for t in s.tool_stats.iter().take(8) {
            let err = if t.error_count > 0 {
                format!("  ({} err)", t.error_count)
            } else {
                String::new()
            };
            out.push(Line::from(format!(
                "  {:<18} {:>3} uses{}",
                t.name, t.use_count, err
            )));
        }
    }
    out.push(Line::from(""));
    out.push(Line::from(Span::styled(
        "Press Enter to open this session in the step-through TUI.",
        Style::default().fg(Color::DarkGray),
    )));
    out
}

fn render_help(f: &mut ratatui::Frame) {
    let lines = vec![
        Line::from(Span::styled(
            "agx corpus — keybindings",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  ↓ / j           next session"),
        Line::from("  ↑ / k           prev session"),
        Line::from("  PgDn / d        jump 10 forward"),
        Line::from("  PgUp / u        jump 10 back"),
        Line::from("  Home / g        first"),
        Line::from("  End  / G        last"),
        Line::from("  Enter           drill into session TUI"),
        Line::from("  s               cycle sort mode"),
        Line::from("  ? / F1          toggle this help"),
        Line::from("  q / Esc         quit"),
        Line::from(""),
        Line::from("Sort cycle: mtime ↓ → cost ↓ → errors ↓ → tokens ↓ → format/name"),
        Line::from(""),
        Line::from(Span::styled(
            "Press any key to dismiss",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let area = centered_rect(HELP_POPUP_WIDTH, height, f.area());
    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" help ")
            .border_style(Style::default().fg(Color::White)),
    );
    f.render_widget(Clear, area);
    f.render_widget(widget, area);
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

fn format_tag(fmt: &str) -> String {
    // Short tags to fit in the list row without wrapping.
    match fmt {
        "Claude Code" => "Claude".into(),
        "Codex CLI" => "Codex".into(),
        "Gemini CLI" => "Gemini".into(),
        "Generic conversation" => "Generic".into(),
        "LangChain / LangSmith" => "LChain".into(),
        "OpenTelemetry GenAI (JSON)" => "OTelJS".into(),
        "OpenTelemetry GenAI (protobuf)" => "OTelPB".into(),
        "Vercel AI SDK" => "Vercel".into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::ParsedSession;
    use crate::format::Format;
    use crate::timeline::{SessionTotals, ToolStats};

    fn mk(
        path: &str,
        mtime: Option<u64>,
        cost: Option<f64>,
        errs: usize,
        tokens: u64,
    ) -> ParsedSession {
        ParsedSession {
            path: PathBuf::from(path),
            format: Format::ClaudeCode,
            step_count: 1,
            totals: SessionTotals {
                tokens_in: tokens,
                tokens_out: 0,
                cache_read: 0,
                cache_create: 0,
                cost_usd: cost,
                unique_models: Vec::new(),
            },
            tool_stats: if errs > 0 {
                vec![ToolStats {
                    name: "Bash".into(),
                    use_count: errs,
                    result_count: errs,
                    error_count: errs,
                }]
            } else {
                Vec::new()
            },
            mtime_secs: mtime,
            annotation_count: 0,
            fork_root_count: 0,
        }
    }

    #[test]
    fn sort_mode_cycle_visits_all_five_then_wraps() {
        let mut s = SortMode::MtimeDesc;
        let order = [
            SortMode::CostDesc,
            SortMode::ErrorsDesc,
            SortMode::TokensDesc,
            SortMode::FormatName,
            SortMode::MtimeDesc,
        ];
        for expected in order {
            s = s.cycle();
            assert_eq!(s, expected);
        }
    }

    #[test]
    fn sort_mtime_desc_puts_newest_first() {
        let mut v = vec![
            mk("a", Some(100), None, 0, 0),
            mk("b", Some(300), None, 0, 0),
            mk("c", Some(200), None, 0, 0),
        ];
        sort_sessions(&mut v, SortMode::MtimeDesc);
        assert_eq!(v[0].path, PathBuf::from("b"));
        assert_eq!(v[1].path, PathBuf::from("c"));
        assert_eq!(v[2].path, PathBuf::from("a"));
    }

    #[test]
    fn sort_mtime_none_lands_at_bottom() {
        // `Option::cmp` orders None < Some; descending reverses that so
        // None mtimes go last — correct for "newest-first" semantics.
        let mut v = vec![mk("a", None, None, 0, 0), mk("b", Some(300), None, 0, 0)];
        sort_sessions(&mut v, SortMode::MtimeDesc);
        assert_eq!(v[0].path, PathBuf::from("b"));
        assert_eq!(v[1].path, PathBuf::from("a"));
    }

    #[test]
    fn sort_cost_desc_puts_expensive_first() {
        let mut v = vec![
            mk("a", None, Some(0.01), 0, 0),
            mk("b", None, Some(1.00), 0, 0),
            mk("c", None, None, 0, 0),
        ];
        sort_sessions(&mut v, SortMode::CostDesc);
        assert_eq!(v[0].path, PathBuf::from("b"));
        assert_eq!(v[1].path, PathBuf::from("a"));
        assert_eq!(v[2].path, PathBuf::from("c"));
    }

    #[test]
    fn sort_errors_desc_puts_most_errored_first() {
        let mut v = vec![
            mk("a", None, None, 0, 0),
            mk("b", None, None, 3, 0),
            mk("c", None, None, 1, 0),
        ];
        sort_sessions(&mut v, SortMode::ErrorsDesc);
        assert_eq!(v[0].path, PathBuf::from("b"));
        assert_eq!(v[1].path, PathBuf::from("c"));
        assert_eq!(v[2].path, PathBuf::from("a"));
    }

    #[test]
    fn sort_tokens_desc_sums_in_and_out() {
        let mut v = vec![
            mk("a", None, None, 0, 50),
            mk("b", None, None, 0, 300),
            mk("c", None, None, 0, 100),
        ];
        sort_sessions(&mut v, SortMode::TokensDesc);
        assert_eq!(v[0].path, PathBuf::from("b"));
        assert_eq!(v[1].path, PathBuf::from("c"));
        assert_eq!(v[2].path, PathBuf::from("a"));
    }

    #[test]
    fn sort_alphabetic_tie_break() {
        // Identical metric, path alphabetical.
        let mut v = vec![
            mk("zebra", Some(100), None, 0, 0),
            mk("apple", Some(100), None, 0, 0),
            mk("mango", Some(100), None, 0, 0),
        ];
        sort_sessions(&mut v, SortMode::MtimeDesc);
        assert_eq!(v[0].path, PathBuf::from("apple"));
        assert_eq!(v[1].path, PathBuf::from("mango"));
        assert_eq!(v[2].path, PathBuf::from("zebra"));
    }

    #[test]
    fn app_selection_survives_sort_cycle() {
        let stats = CorpusStats {
            parse_success_count: 3,
            ..CorpusStats::default()
        };
        let sessions = vec![
            mk("a", Some(300), Some(0.01), 0, 0),
            mk("b", Some(100), Some(1.00), 0, 0),
            mk("c", Some(200), Some(0.50), 0, 0),
        ];
        let mut app = App::new(sessions, &stats, false);
        // After mtime-desc sort: [a(300), c(200), b(100)]. Select index 1 (c).
        app.list_state.select(Some(1));
        let pre = app.selected().map(|s| s.path.clone());
        assert_eq!(pre, Some(PathBuf::from("c")));
        // Cycle to cost desc: [b(1.00), c(0.50), a(0.01)]. Selection should
        // still point to `c` even though it's now index 1.
        app.cycle_sort();
        let post = app.selected().map(|s| s.path.clone());
        assert_eq!(post, Some(PathBuf::from("c")));
    }

    #[test]
    fn format_tag_trims_long_format_names() {
        assert_eq!(format_tag("OpenTelemetry GenAI (JSON)"), "OTelJS");
        assert_eq!(format_tag("LangChain / LangSmith"), "LChain");
    }
}
