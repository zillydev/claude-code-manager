use ratatui::style::{Color, Modifier, Style};

pub fn user() -> Style {
    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
}

pub fn assistant() -> Style {
    Style::default()
        .fg(Color::LightGreen)
        .add_modifier(Modifier::BOLD)
}

pub fn system() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::DIM)
}

/// Part of the theme palette; no view currently renders thinking blocks.
#[allow(dead_code)]
pub fn thinking() -> Style {
    Style::default()
        .fg(Color::Gray)
        .add_modifier(Modifier::ITALIC | Modifier::DIM)
}

pub fn tool_use() -> Style {
    Style::default().fg(Color::Magenta)
}

pub fn tool_result() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub fn meta() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub fn cursor() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

pub fn header() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::LightBlue)
        .add_modifier(Modifier::BOLD)
}

pub fn footer() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::DarkGray)
}
