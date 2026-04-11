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
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use std::io;

const PAGE_STEP: usize = 10;
const HELP_POPUP_WIDTH: u16 = 64;

pub struct App {
    steps: Vec<Step>,
    list_state: ListState,
    show_help: bool,
}

impl App {
    pub fn new(steps: Vec<Step>) -> Self {
        let mut list_state = ListState::default();
        if !steps.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            steps,
            list_state,
            show_help: false,
        }
    }

    fn next(&mut self) {
        if self.steps.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + 1).min(self.steps.len() - 1);
        self.list_state.select(Some(next));
    }

    fn prev(&mut self) {
        if self.steps.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.saturating_sub(1)));
    }

    fn page_down(&mut self, n: usize) {
        if self.steps.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + n).min(self.steps.len() - 1);
        self.list_state.select(Some(next));
    }

    fn page_up(&mut self, n: usize) {
        if self.steps.is_empty() {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        self.list_state.select(Some(i.saturating_sub(n)));
    }

    fn home(&mut self) {
        if !self.steps.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    fn end(&mut self) {
        if !self.steps.is_empty() {
            self.list_state.select(Some(self.steps.len() - 1));
        }
    }

    fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }
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
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(f.area());

            let items: Vec<ListItem> = app
                .steps
                .iter()
                .map(|s| {
                    let color = kind_color(s.kind);
                    ListItem::new(Line::from(vec![Span::styled(
                        s.label.as_str(),
                        Style::default().fg(color),
                    )]))
                })
                .collect();

            let total = app.steps.len();
            let current = app.list_state.selected().map_or(0, |i| i + 1);
            let title = format!(" agx — {current}/{total}   [? help] ");

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
                .and_then(|i| app.steps.get(i))
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
                        Span::raw("tool_use (assistant calls a tool)"),
                    ]),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled("magenta", Style::default().fg(Color::Magenta)),
                        Span::raw(" tool_result (output back to assistant)"),
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
            if app.show_help {
                app.show_help = false;
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('?') | KeyCode::F(1) => app.toggle_help(),
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
