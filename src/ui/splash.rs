use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::app::App;

const ART: &str = include_str!("../../ascii.txt");
const ART_HEIGHT: u16 = 5;

const BG:  Color = Color::Rgb(30, 32, 38);
const DIM: Color = Color::Rgb(120, 125, 140);

pub fn render(f: &mut Frame, app: &App, area: Rect) {

    let (ar, ag, ab) = app.config.ui.accent_rgb();
    let accent = Color::Rgb(ar, ag, ab);

    // Fill background.
    f.render_widget(Block::default().style(Style::default().bg(BG)), area);

    // Vertical layout: top padding · art · gap · hint line · bottom padding.
    let content_h = ART_HEIGHT + 3; // art + 2-line gap + 1-line hints
    let vpad = area.height.saturating_sub(content_h) / 2;

    let chunks = Layout::vertical([
        Constraint::Length(vpad),
        Constraint::Length(ART_HEIGHT),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);

    // ASCII art — each line styled with the accent colour.
    let art_lines: Vec<Line> = ART
        .lines()
        .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(accent))))
        .collect();

    f.render_widget(
        Paragraph::new(art_lines)
            .alignment(Alignment::Center)
            .style(Style::default().bg(BG)),
        chunks[1],
    );

    // Keybind hints.
    let key  = Style::default().fg(accent).add_modifier(Modifier::BOLD);
    let dim  = Style::default().fg(DIM);

    let hints = Line::from(vec![
        Span::styled("[", dim),
        Span::styled("n", key),
        Span::styled("] new norg", dim),
        Span::styled("     ", dim),
        Span::styled("[", dim),
        Span::styled("o", key),
        Span::styled("] open norg", dim),
        Span::styled("     ", dim),
        Span::styled("[", dim),
        Span::styled("c", key),
        Span::styled("] config", dim),
    ]);

    f.render_widget(
        Paragraph::new(hints)
            .alignment(Alignment::Center)
            .style(Style::default().bg(BG)),
        chunks[3],
    );
}
