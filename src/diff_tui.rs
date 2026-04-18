//! Two-pane diff TUI. Consumes the pure alignment from
//! `diff_align::align` and renders it as two synchronized lists with
//! one `AlignRow` per display line. Color coding:
//!
//! - Match    — default foreground, both sides populated
//! - Differ   — yellow (structure aligns, content drifted)
//! - LeftOnly — left side dim, right side shows the gray "(absent)"
//! - RightOnly — symmetric
//!
//! Navigation mirrors the per-session and corpus TUIs — j/k/g/G/
//! Home/End/PgUp/PgDn with a ?/F1 help overlay and q/Esc quit.
//!
//! Synchronized scrolling trick: we share one `ListState` across both
//! panes' `render_stateful_widget` calls. The two panes are the same
//! height (horizontal split of a single vertical slot), so ratatui's
//! "keep selected visible" offset adjustment produces identical
//! offsets on both sides — the panes scroll together for free without
//! any manual top/height bookkeeping.

use crate::diff_align::{AlignKind, AlignRow, align};
use crate::timeline::{SessionTotals, Step, compute_session_totals};
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
use std::path::Path;
use std::time::Duration;

const PAGE_STEP: usize = 10;
const HELP_POPUP_WIDTH: u16 = 56;

struct App<'a> {
    left: &'a [Step],
    right: &'a [Step],
    rows: Vec<AlignRow>,
    list_state: ListState,
    show_help: bool,
    left_totals: SessionTotals,
    right_totals: SessionTotals,
    left_label: String,
    right_label: String,
    no_cost: bool,
}

impl<'a> App<'a> {
    fn new(
        left: &'a [Step],
        right: &'a [Step],
        left_path: &Path,
        right_path: &Path,
        left_format: &str,
        right_format: &str,
        no_cost: bool,
    ) -> Self {
        let rows = align(left, right);
        let mut list_state = ListState::default();
        if !rows.is_empty() {
            list_state.select(Some(0));
        }
        App {
            left,
            right,
            rows,
            list_state,
            show_help: false,
            left_totals: compute_session_totals(left),
            right_totals: compute_session_totals(right),
            left_label: format!(
                "{} · {}",
                left_format,
                left_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| left_path.display().to_string())
            ),
            right_label: format!(
                "{} · {}",
                right_format,
                right_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| right_path.display().to_string())
            ),
            no_cost,
        }
    }

    fn next(&mut self, n: usize) {
        if self.rows.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + n).min(self.rows.len() - 1);
        self.list_state.select(Some(next));
    }

    fn prev(&mut self, n: usize) {
        if self.rows.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.saturating_sub(n)));
    }

    fn home(&mut self) {
        if !self.rows.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    fn end(&mut self) {
        if !self.rows.is_empty() {
            self.list_state.select(Some(self.rows.len() - 1));
        }
    }
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

/// Launch the diff TUI. Caller supplies the already-loaded step vecs
/// so `main.rs` can reuse the existing `load_session` path and error
/// handling. Returns when the user quits with q or Esc.
pub fn run(
    left: &[Step],
    right: &[Step],
    left_path: &Path,
    right_path: &Path,
    left_format: &str,
    right_format: &str,
    no_cost: bool,
) -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(
        left,
        right,
        left_path,
        right_path,
        left_format,
        right_format,
        no_cost,
    );

    event_loop(&mut terminal, &mut app)?;
    let _ = terminal.show_cursor();
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn event_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
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

            // Header: side labels + per-side totals + alignment summary.
            let counts = count_align_kinds(&app.rows);
            let header = format!(
                " agx diff — A: {} ({}{})   ↔   B: {} ({}{})   [{} match · {} differ · {} only-A · {} only-B]",
                app.left_label,
                format_totals_short(&app.left_totals),
                if !app.no_cost {
                    format_cost(&app.left_totals)
                } else {
                    String::new()
                },
                app.right_label,
                format_totals_short(&app.right_totals),
                if !app.no_cost {
                    format_cost(&app.right_totals)
                } else {
                    String::new()
                },
                counts.matches,
                counts.differs,
                counts.left_only,
                counts.right_only,
            );
            f.render_widget(
                Paragraph::new(header).style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                outer[0],
            );

            // Two-pane body.
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(outer[1]);

            let left_items = build_items(&app.rows, Side::Left, app.left, app.right);
            let right_items = build_items(&app.rows, Side::Right, app.left, app.right);

            let left_list = List::new(left_items)
                .block(Block::default().borders(Borders::ALL).title(" A "))
                .highlight_style(
                    Style::default()
                        .add_modifier(Modifier::REVERSED)
                        .add_modifier(Modifier::BOLD),
                );
            let right_list = List::new(right_items)
                .block(Block::default().borders(Borders::ALL).title(" B "))
                .highlight_style(
                    Style::default()
                        .add_modifier(Modifier::REVERSED)
                        .add_modifier(Modifier::BOLD),
                );

            // Render both panes with the same ListState. Because the
            // two panes are horizontally split from a single vertical
            // slot they share a height, and ratatui's "keep selected
            // visible" offset math produces the same offset both times
            // → the panes scroll in lockstep with no manual
            // bookkeeping.
            f.render_stateful_widget(left_list, panes[0], &mut app.list_state);
            f.render_stateful_widget(right_list, panes[1], &mut app.list_state);

            let hints = "j/k navigate · g/G first/last · PgUp/PgDn page · ? help · q quit";
            f.render_widget(
                Paragraph::new(hints).style(Style::default().fg(Color::DarkGray)),
                outer[2],
            );

            if app.show_help {
                render_help(f);
            }
        })?;

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
            app.show_help = false;
            continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Char('?') | KeyCode::F(1) => app.show_help = true,
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

#[derive(Copy, Clone)]
enum Side {
    Left,
    Right,
}

fn build_items<'a>(
    rows: &[AlignRow],
    side: Side,
    left: &'a [Step],
    right: &'a [Step],
) -> Vec<ListItem<'a>> {
    rows.iter()
        .map(|row| {
            let (step_idx, steps) = match side {
                Side::Left => (row.left, left),
                Side::Right => (row.right, right),
            };
            let (prefix, color) = row_style(row.kind, side);
            match step_idx {
                Some(i) => {
                    let label = steps
                        .get(i)
                        .map(|s| s.label.clone())
                        .unwrap_or_else(|| "(?)".into());
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(color)),
                        Span::styled(label, Style::default().fg(color)),
                    ]))
                }
                None => ListItem::new(Line::from(Span::styled(
                    "  (absent)",
                    Style::default().fg(Color::DarkGray),
                ))),
            }
        })
        .collect()
}

/// Return the two-char prefix plus the foreground color for a row on
/// the given side. Prefixes are ASCII so terminals without nerd-font
/// support still render correctly (terminal-native principle).
fn row_style(kind: AlignKind, side: Side) -> (&'static str, Color) {
    match (kind, side) {
        (AlignKind::Match, _) => ("= ", Color::Green),
        (AlignKind::Differ, _) => ("~ ", Color::Yellow),
        (AlignKind::LeftOnly, Side::Left) => ("- ", Color::Red),
        (AlignKind::LeftOnly, Side::Right) => ("  ", Color::DarkGray),
        (AlignKind::RightOnly, Side::Left) => ("  ", Color::DarkGray),
        (AlignKind::RightOnly, Side::Right) => ("+ ", Color::Green),
    }
}

#[derive(Default)]
struct AlignCounts {
    matches: usize,
    differs: usize,
    left_only: usize,
    right_only: usize,
}

fn count_align_kinds(rows: &[AlignRow]) -> AlignCounts {
    let mut c = AlignCounts::default();
    for row in rows {
        match row.kind {
            AlignKind::Match => c.matches += 1,
            AlignKind::Differ => c.differs += 1,
            AlignKind::LeftOnly => c.left_only += 1,
            AlignKind::RightOnly => c.right_only += 1,
        }
    }
    c
}

fn format_totals_short(t: &SessionTotals) -> String {
    format!("{}/{} tok", t.tokens_in, t.tokens_out)
}

fn format_cost(t: &SessionTotals) -> String {
    match t.cost_usd {
        Some(c) => format!(" · ${c:.4}"),
        None => String::new(),
    }
}

fn render_help(f: &mut ratatui::Frame) {
    let lines = vec![
        Line::from(Span::styled(
            "agx diff — keybindings",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  ↓ / j           next row"),
        Line::from("  ↑ / k           prev row"),
        Line::from("  PgDn / d        jump 10 forward"),
        Line::from("  PgUp / u        jump 10 back"),
        Line::from("  Home / g        first"),
        Line::from("  End  / G        last"),
        Line::from("  ? / F1          toggle this help"),
        Line::from("  q / Esc         quit"),
        Line::from(""),
        Line::from(Span::styled(
            "Row color legend",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("  = ", Style::default().fg(Color::Green)),
            Span::raw("match: same kind, same tool, identical content"),
        ]),
        Line::from(vec![
            Span::styled("  ~ ", Style::default().fg(Color::Yellow)),
            Span::raw("differ: same kind, same tool, content drifted"),
        ]),
        Line::from(vec![
            Span::styled("  - ", Style::default().fg(Color::Red)),
            Span::raw("only in A (left)"),
        ]),
        Line::from(vec![
            Span::styled("  + ", Style::default().fg(Color::Green)),
            Span::raw("only in B (right)"),
        ]),
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
    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" help ")
                .border_style(Style::default().fg(Color::White)),
        )
        .wrap(Wrap { trim: false });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{tool_use_step, user_text_step};

    #[test]
    fn count_align_kinds_sums_categories() {
        let rows = vec![
            AlignRow {
                left: Some(0),
                right: Some(0),
                kind: AlignKind::Match,
            },
            AlignRow {
                left: Some(1),
                right: Some(1),
                kind: AlignKind::Differ,
            },
            AlignRow {
                left: Some(2),
                right: None,
                kind: AlignKind::LeftOnly,
            },
            AlignRow {
                left: None,
                right: Some(2),
                kind: AlignKind::RightOnly,
            },
            AlignRow {
                left: None,
                right: Some(3),
                kind: AlignKind::RightOnly,
            },
        ];
        let c = count_align_kinds(&rows);
        assert_eq!(c.matches, 1);
        assert_eq!(c.differs, 1);
        assert_eq!(c.left_only, 1);
        assert_eq!(c.right_only, 2);
    }

    #[test]
    fn app_new_selects_first_row_when_nonempty() {
        let left = vec![user_text_step("hi")];
        let right = vec![user_text_step("hi")];
        let app = App::new(
            &left,
            &right,
            Path::new("a.jsonl"),
            Path::new("b.jsonl"),
            "Claude Code",
            "Claude Code",
            false,
        );
        assert_eq!(app.list_state.selected(), Some(0));
        assert_eq!(app.rows.len(), 1);
    }

    #[test]
    fn app_new_leaves_selection_none_when_empty() {
        let app = App::new(
            &[],
            &[],
            Path::new("a.jsonl"),
            Path::new("b.jsonl"),
            "x",
            "y",
            false,
        );
        assert_eq!(app.list_state.selected(), None);
        assert!(app.rows.is_empty());
    }

    #[test]
    fn navigation_clamps_to_bounds() {
        let left = vec![
            user_text_step("a"),
            user_text_step("b"),
            user_text_step("c"),
        ];
        let right = left.clone();
        let mut app = App::new(
            &left,
            &right,
            Path::new("a"),
            Path::new("b"),
            "x",
            "x",
            false,
        );
        // Start at 0, go past the end, should clamp to last.
        app.next(999);
        assert_eq!(app.list_state.selected(), Some(2));
        // Go way back, should clamp to 0.
        app.prev(999);
        assert_eq!(app.list_state.selected(), Some(0));
        // Single step.
        app.next(1);
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn row_style_distinguishes_side_for_one_sided_rows() {
        // LeftOnly should be styled on the left, blank-gray on the right.
        let (prefix_l, color_l) = row_style(AlignKind::LeftOnly, Side::Left);
        let (prefix_r, color_r) = row_style(AlignKind::LeftOnly, Side::Right);
        assert_eq!(prefix_l, "- ");
        assert_eq!(color_l, Color::Red);
        assert_eq!(prefix_r, "  ");
        assert_eq!(color_r, Color::DarkGray);

        // RightOnly: mirror.
        let (prefix_l2, _) = row_style(AlignKind::RightOnly, Side::Left);
        let (prefix_r2, color_r2) = row_style(AlignKind::RightOnly, Side::Right);
        assert_eq!(prefix_l2, "  ");
        assert_eq!(prefix_r2, "+ ");
        assert_eq!(color_r2, Color::Green);
    }

    #[test]
    fn build_items_uses_absent_sentinel_for_gap_side() {
        // RightOnly row → left side should render "(absent)" at DarkGray.
        let left = vec![user_text_step("hi")];
        let right = vec![user_text_step("hi"), tool_use_step("t1", "Bash", "{}")];
        let rows = align(&left, &right);
        // Expect: Match (user), RightOnly (Bash tool_use).
        assert_eq!(rows.len(), 2);
        let items_left = build_items(&rows, Side::Left, &left, &right);
        let items_right = build_items(&rows, Side::Right, &left, &right);
        assert_eq!(items_left.len(), 2);
        assert_eq!(items_right.len(), 2);
        // We can't inspect Line spans easily — the test exists to
        // ensure build_items doesn't panic on gaps and produces the
        // same length on both sides (invariant the two-pane render
        // depends on).
    }
}
