use crate::timeline::{Step, StepKind};
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
use ratatui::widgets::{Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap};
use std::io;

const PAGE_STEP: usize = 10;
const HELP_POPUP_WIDTH: u16 = 64;
const ALT_BG: Color = Color::Indexed(236);

enum InputMode {
    Command(String),
    Filter(String),
}

pub struct App {
    steps: Vec<Step>,
    list_state: ListState,
    bg_flags: Vec<bool>,
    filtered_view: Vec<usize>,
    filter: Option<String>,
    input_mode: Option<InputMode>,
    show_help: bool,
    status_msg: Option<String>,
}

impl App {
    pub fn new(steps: Vec<Step>) -> Self {
        let mut list_state = ListState::default();
        if !steps.is_empty() {
            list_state.select(Some(0));
        }
        let bg_flags = compute_bg_flags(&steps);
        let filtered_view: Vec<usize> = (0..steps.len()).collect();
        Self {
            steps,
            list_state,
            bg_flags,
            filtered_view,
            filter: None,
            input_mode: None,
            show_help: false,
            status_msg: None,
        }
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
    }

    fn clear_filter(&mut self) {
        self.filter = None;
        self.filtered_view = (0..self.steps.len()).collect();
        if self.filtered_view.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }
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
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

pub fn run(steps: Vec<Step>) -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
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
            // Outer layout: main area + 1-row status/command bar at the bottom.
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(f.area());

            // Main area split into list (40%) and detail (60%).
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(outer[0]);

            let items: Vec<ListItem> = app
                .filtered_view
                .iter()
                .filter_map(|&orig_idx| {
                    let s = app.steps.get(orig_idx)?;
                    let color = kind_color(s.kind);
                    let mut style = Style::default().fg(color);
                    if app.bg_flags.get(orig_idx).copied().unwrap_or(false) {
                        style = style.bg(ALT_BG);
                    }
                    Some(ListItem::new(Line::from(vec![Span::styled(
                        s.label.as_str(),
                        style,
                    )])))
                })
                .collect();

            let total = app.visible_count();
            let current = app.list_state.selected().map_or(0, |i| i + 1);
            let title = if let Some(q) = &app.filter {
                format!(" agx — {current}/{total}   [filter: {q}]   [? help] ")
            } else {
                format!(" agx — {current}/{total}   [? help] ")
            };

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(title))
                .highlight_style(
                    Style::default()
                        .add_modifier(Modifier::REVERSED)
                        .add_modifier(Modifier::BOLD),
                );

            f.render_stateful_widget(list, chunks[0], &mut app.list_state);

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

            // Bottom bar: input line (command / filter), or status msg, or scrubbing gauge.
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
                        let label = if let Some(q) = &app.filter {
                            format!("{current}/{total}  (filter: {q})")
                        } else {
                            format!("{current}/{total}")
                        };
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
                    Line::from("  f               open filter prompt"),
                    Line::from("  (empty enter)   clear current filter"),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Other",
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from("  ? / F1          toggle this help"),
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

        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            // Help overlay: any key dismisses.
            if app.show_help {
                app.show_help = false;
                continue;
            }

            // Input mode (command / filter): its own keybinding scope.
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
                            None => {}
                        }
                    }
                    KeyCode::Backspace => {
                        if let Some(InputMode::Command(buf) | InputMode::Filter(buf)) =
                            &mut app.input_mode
                        {
                            buf.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Some(InputMode::Command(buf) | InputMode::Filter(buf)) =
                            &mut app.input_mode
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
            tool_result_step("t1", "def fib...", Some("Read"), Some("{}")),
            tool_use_step("t2", "Bash", "{}"),
            tool_result_step("t2", "0 1 1 2 3 5", Some("Bash"), Some("{}")),
            assistant_text_step("done"),
        ]
    }

    #[test]
    fn goto_step_selects_valid_index() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.goto_step(2);
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn goto_step_clamps_out_of_bounds() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.goto_step(999);
        assert_eq!(app.list_state.selected(), Some(5));
    }

    #[test]
    fn goto_step_rejects_zero() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.list_state.select(Some(0));
        app.goto_step(0);
        assert_eq!(app.list_state.selected(), Some(0));
        assert!(app.status_msg.as_ref().unwrap().contains(">= 1"));
    }

    #[test]
    fn execute_command_parses_number() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.execute_command("3");
        assert_eq!(app.list_state.selected(), Some(2));
    }

    #[test]
    fn execute_command_ignores_empty_input() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.list_state.select(Some(0));
        app.execute_command("   ");
        assert_eq!(app.list_state.selected(), Some(0));
        assert!(app.status_msg.is_none());
    }

    #[test]
    fn execute_command_reports_unknown() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.execute_command("nope");
        assert!(app.status_msg.as_ref().unwrap().contains("unknown"));
    }

    #[test]
    fn apply_filter_by_tool_name_substring_case_insensitive() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.apply_filter("read");
        // Matches step 2 (tool_use Read) and step 3 (tool_result Read)
        assert_eq!(app.visible_count(), 2);
        assert_eq!(app.filter.as_deref(), Some("read"));
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[test]
    fn apply_filter_by_kind_prefix() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.apply_filter("[tool]");
        // Matches both [tool] steps
        assert_eq!(app.visible_count(), 2);
    }

    #[test]
    fn apply_filter_empty_clears_existing_filter() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.apply_filter("Read");
        assert_eq!(app.visible_count(), 2);
        app.apply_filter("");
        assert_eq!(app.visible_count(), 6);
        assert!(app.filter.is_none());
    }

    #[test]
    fn apply_filter_no_matches_keeps_previous_view_and_sets_error() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.apply_filter("nonexistent");
        assert_eq!(app.visible_count(), 6); // unchanged
        assert!(app.filter.is_none()); // filter not installed
        assert!(app.status_msg.as_ref().unwrap().contains("no matches"));
    }

    #[test]
    fn clear_filter_restores_full_view() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.apply_filter("Read");
        app.clear_filter();
        assert_eq!(app.visible_count(), 6);
        assert!(app.filter.is_none());
        assert_eq!(app.list_state.selected(), Some(0));
    }

    #[test]
    fn navigation_under_filter_operates_on_filtered_view() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.apply_filter("[tool]");
        assert_eq!(app.visible_count(), 2);
        assert_eq!(app.list_state.selected(), Some(0));
        app.next();
        assert_eq!(app.list_state.selected(), Some(1));
        app.next(); // clamps to last
        assert_eq!(app.list_state.selected(), Some(1));
        app.home();
        assert_eq!(app.list_state.selected(), Some(0));
        app.end();
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn goto_step_under_filter_uses_visible_positions() {
        let steps = sample_steps();
        let mut app = App::new(steps);
        app.apply_filter("[tool]");
        app.goto_step(2); // 2nd visible row
        assert_eq!(app.list_state.selected(), Some(1));
    }
}
