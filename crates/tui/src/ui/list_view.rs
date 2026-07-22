use std::time::{Duration, SystemTime};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use claude_code_manager_core::session::summary::{compact_tokens, relative_time, SessionSummary};
use crate::ui::theme;

pub enum ListAction {
    None,
    Quit,
    Refresh,
    Open(usize),
}

#[derive(Clone, Copy, PartialEq)]
enum SortField {
    Updated,
    Project,
    Title,
    Tokens,
}

impl SortField {
    fn next(self) -> Self {
        match self {
            Self::Updated => Self::Project,
            Self::Project => Self::Title,
            Self::Title => Self::Tokens,
            Self::Tokens => Self::Updated,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Updated => "updated",
            Self::Project => "project",
            Self::Title => "title",
            Self::Tokens => "tokens",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DateRange {
    All,
    Today,
    Week,
    Month,
}

impl DateRange {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Today,
            Self::Today => Self::Week,
            Self::Week => Self::Month,
            Self::Month => Self::All,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Today => "today",
            Self::Week => "week",
            Self::Month => "month",
        }
    }
    fn max_age(self) -> Option<Duration> {
        match self {
            Self::All => None,
            Self::Today => Some(Duration::from_secs(86400)),
            Self::Week => Some(Duration::from_secs(604800)),
            Self::Month => Some(Duration::from_secs(2592000)),
        }
    }
}

pub struct ListView {
    all: Vec<SessionSummary>,
    visible: Vec<usize>,
    cursor: usize,
    table_state: TableState,
    filter: String,
    filter_focus: bool,
    sort_field: SortField,
    sort_desc: bool,
    project_filter: Option<String>,
    date_range: DateRange,
}

impl ListView {
    pub fn new(all: Vec<SessionSummary>) -> Self {
        let visible: Vec<usize> = (0..all.len()).collect();
        let mut table_state = TableState::default();
        if !visible.is_empty() {
            table_state.select(Some(0));
        }
        Self {
            all,
            visible,
            cursor: 0,
            table_state,
            filter: String::new(),
            filter_focus: false,
            sort_field: SortField::Updated,
            sort_desc: true,
            project_filter: None,
            date_range: DateRange::All,
        }
    }

    pub fn replace(&mut self, all: Vec<SessionSummary>) {
        self.all = all;
        self.recompute();
    }

    pub fn focused_summary(&self) -> Option<&SessionSummary> {
        self.visible.get(self.cursor).and_then(|&i| self.all.get(i))
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ListAction {
        if self.filter_focus {
            return self.handle_filter_key(key);
        }
        match key.code {
            KeyCode::Char('q') => ListAction::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ListAction::Quit
            }
            KeyCode::Char('r') => ListAction::Refresh,
            KeyCode::Char('/') => {
                self.filter_focus = true;
                ListAction::None
            }
            KeyCode::Char('s') => {
                self.sort_field = self.sort_field.next();
                self.sort_desc = true;
                self.recompute();
                ListAction::None
            }
            KeyCode::Char('S') => {
                self.sort_desc = !self.sort_desc;
                self.recompute();
                ListAction::None
            }
            KeyCode::Char('p') => {
                self.cycle_project_filter();
                self.recompute();
                ListAction::None
            }
            KeyCode::Char('d') => {
                self.date_range = self.date_range.next();
                self.recompute();
                ListAction::None
            }
            KeyCode::Esc => {
                if !self.filter.is_empty() || self.project_filter.is_some() || self.date_range != DateRange::All {
                    self.filter.clear();
                    self.project_filter = None;
                    self.date_range = DateRange::All;
                    self.recompute();
                }
                ListAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_cursor(1);
                ListAction::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_cursor(-1);
                ListAction::None
            }
            KeyCode::PageDown => {
                self.move_cursor(10);
                ListAction::None
            }
            KeyCode::PageUp => {
                self.move_cursor(-10);
                ListAction::None
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.cursor = 0;
                self.table_state
                    .select(if self.visible.is_empty() { None } else { Some(0) });
                ListAction::None
            }
            KeyCode::End | KeyCode::Char('G') => {
                if !self.visible.is_empty() {
                    self.cursor = self.visible.len() - 1;
                    self.table_state.select(Some(self.cursor));
                }
                ListAction::None
            }
            KeyCode::Enter => {
                if let Some(&i) = self.visible.get(self.cursor) {
                    ListAction::Open(i)
                } else {
                    ListAction::None
                }
            }
            _ => ListAction::None,
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> ListAction {
        match key.code {
            KeyCode::Esc => {
                self.filter_focus = false;
                self.filter.clear();
                self.recompute();
                ListAction::None
            }
            KeyCode::Enter => {
                self.filter_focus = false;
                ListAction::None
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.recompute();
                ListAction::None
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.recompute();
                ListAction::None
            }
            _ => ListAction::None,
        }
    }

    fn cycle_project_filter(&mut self) {
        let mut projects: Vec<String> = self
            .all
            .iter()
            .map(|s| s.project_label.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        projects.sort();

        match &self.project_filter {
            None if !projects.is_empty() => {
                self.project_filter = Some(projects[0].clone());
            }
            Some(current) => {
                let idx = projects.iter().position(|p| p == current).unwrap_or(0);
                if idx + 1 < projects.len() {
                    self.project_filter = Some(projects[idx + 1].clone());
                } else {
                    self.project_filter = None;
                }
            }
            _ => {}
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        if self.visible.is_empty() {
            return;
        }
        let len = self.visible.len() as i32;
        let mut next = self.cursor as i32 + delta;
        if next < 0 {
            next = 0;
        } else if next >= len {
            next = len - 1;
        }
        self.cursor = next as usize;
        self.table_state.select(Some(self.cursor));
    }

    fn recompute(&mut self) {
        let q = self.filter.to_lowercase();
        let now = SystemTime::now();
        let prev_focused = self.focused_summary().map(|s| s.id);

        // Filter
        self.visible = self
            .all
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                // Text filter
                if !q.is_empty() {
                    let hay = format!("{} {}", s.project_label, s.title).to_lowercase();
                    if !hay.contains(&q) {
                        return false;
                    }
                }
                // Project filter
                if let Some(pf) = &self.project_filter {
                    if &s.project_label != pf {
                        return false;
                    }
                }
                // Date range
                if let Some(max_age) = self.date_range.max_age() {
                    if let Ok(age) = now.duration_since(s.mtime) {
                        if age > max_age {
                            return false;
                        }
                    }
                }
                true
            })
            .map(|(i, _)| i)
            .collect();

        // Sort
        let sf = self.sort_field;
        let desc = self.sort_desc;
        self.visible.sort_by(|&a, &b| {
            let sa = &self.all[a];
            let sb = &self.all[b];
            let cmp = match sf {
                SortField::Updated => sa.mtime.cmp(&sb.mtime),
                SortField::Project => sa.project_label.cmp(&sb.project_label),
                SortField::Title => sa.title.cmp(&sb.title),
                SortField::Tokens => sa.total_tokens.cmp(&sb.total_tokens),
            };
            if desc { cmp.reverse() } else { cmp }
        });

        // Restore cursor
        if let Some(id) = prev_focused {
            if let Some(pos) = self.visible.iter().position(|&i| self.all[i].id == id) {
                self.cursor = pos;
                self.table_state.select(Some(pos));
                return;
            }
        }
        self.cursor = 0;
        self.table_state
            .select(if self.visible.is_empty() { None } else { Some(0) });
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // header
                Constraint::Length(1), // active filters
                Constraint::Min(1),    // table
                Constraint::Length(1), // filter input
                Constraint::Length(1), // keybinds
            ])
            .split(area);

        // Header
        let header = Paragraph::new(Line::from(vec![
            Span::raw(" claude-code-manager "),
            Span::styled(
                format!(
                    "({}/{} sessions)",
                    self.visible.len(),
                    self.all.len()
                ),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ]))
        .style(theme::header());
        f.render_widget(header, chunks[0]);

        // Active filters bar
        let mut filter_spans: Vec<Span> = vec![Span::raw(" ")];
        filter_spans.push(Span::styled(
            format!("sort:{}", self.sort_field.label()),
            Style::default().fg(ratatui::style::Color::Cyan),
        ));
        filter_spans.push(Span::styled(
            if self.sort_desc { "↓" } else { "↑" },
            Style::default().fg(ratatui::style::Color::Cyan),
        ));
        if let Some(pf) = &self.project_filter {
            filter_spans.push(Span::raw("  "));
            filter_spans.push(Span::styled(
                format!("project:{}", pf),
                Style::default().fg(ratatui::style::Color::Magenta),
            ));
        }
        if self.date_range != DateRange::All {
            filter_spans.push(Span::raw("  "));
            filter_spans.push(Span::styled(
                format!("range:{}", self.date_range.label()),
                Style::default().fg(ratatui::style::Color::Yellow),
            ));
        }
        let filters_p = Paragraph::new(Line::from(filter_spans)).style(theme::footer());
        f.render_widget(filters_p, chunks[1]);

        // Table
        let project_w: u16 = 22;
        let updated_w: u16 = 8;
        let tokens_w: u16 = 8;
        let header_row = Row::new(vec!["Project", "Title", "Updated", "Tokens"])
            .style(Style::default().add_modifier(Modifier::BOLD));
        let rows: Vec<Row> = self
            .visible
            .iter()
            .filter_map(|&i| self.all.get(i))
            .map(|s| {
                Row::new(vec![
                    truncate(&s.project_label, project_w as usize),
                    s.title.clone(),
                    relative_time(s.mtime),
                    compact_tokens(s.total_tokens),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(project_w),
            Constraint::Min(20),
            Constraint::Length(updated_w),
            Constraint::Length(tokens_w),
        ];
        let table = Table::new(rows, widths)
            .header(header_row)
            .row_highlight_style(theme::cursor())
            .highlight_symbol("▸ ")
            .block(Block::default().borders(Borders::NONE));
        f.render_stateful_widget(table, chunks[2], &mut self.table_state);

        // Filter input line
        let filter_text = if self.filter_focus {
            format!("/{}_", self.filter)
        } else if !self.filter.is_empty() {
            format!("filter: {}", self.filter)
        } else {
            String::new()
        };
        let filter_p = Paragraph::new(filter_text).style(theme::footer());
        f.render_widget(filter_p, chunks[3]);

        // Keybinds
        let keys = if self.filter_focus {
            " type to filter   Esc cancel   Enter accept "
        } else {
            " ↑↓/jk move  Enter open  / filter  s sort  S reverse  p project  d date  r refresh  Esc clear  q quit "
        };
        let keys_p = Paragraph::new(keys).style(theme::footer());
        f.render_widget(keys_p, chunks[4]);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max <= 1 {
        "…".to_string()
    } else {
        let head: String = s.chars().take(max - 1).collect();
        format!("{}…", head)
    }
}
