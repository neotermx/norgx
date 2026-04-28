use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;

const BAR_BG: Color = Color::Rgb(22, 24, 30);
const TEXT_DIM: Color = Color::Rgb(120, 125, 140);

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(1),      // left: mode + file + message
            Constraint::Length(10),  // center: Ctrl+/ hint
            Constraint::Length(22),  // right: word count + row:col
        ])
        .split(area);

    render_left(f, app, sections[0]);
    render_hint(f, app, sections[1]);
    render_right(f, app, sections[2]);
}

fn render_left(f: &mut Frame, app: &App, area: Rect) {
    let (ar, ag, ab) = app.config.ui.accent_rgb();
    let accent = Color::Rgb(ar, ag, ab);
    let file_display = app
        .file_path
        .as_ref()
        .and_then(|p| p.to_str())
        .unwrap_or("[no file]");

    let dirty_marker = if app.modified { " ●" } else { "" };

    let line = Line::from(vec![
        // Mode pill
        Span::styled(
            " NORGX",
            Style::default()
                .fg(Color::Black)
                .bg(accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", Style::default().bg(BAR_BG)),
        // File path
        Span::styled(
            format!("{file_display}{dirty_marker}"),
            Style::default().fg(Color::White).bg(BAR_BG),
        ),
        Span::styled("  ", Style::default().bg(BAR_BG)),
        // Status message
        Span::styled(
            app.status_msg.clone(),
            Style::default().fg(TEXT_DIM).bg(BAR_BG),
        ),
    ]);

    f.render_widget(Paragraph::new(line).style(Style::default().bg(BAR_BG)), area);
}

fn render_hint(f: &mut Frame, app: &App, area: Rect) {
    let (ar, ag, ab) = app.config.ui.accent_rgb();
    let accent = Color::Rgb(ar, ag, ab);

    let line = Line::from(vec![
        Span::styled("Ctrl+", Style::default().fg(TEXT_DIM).bg(BAR_BG)),
        Span::styled("/",     Style::default().fg(accent).bg(BAR_BG)),
        Span::styled(" keys", Style::default().fg(TEXT_DIM).bg(BAR_BG)),
    ]);

    f.render_widget(Paragraph::new(line).style(Style::default().bg(BAR_BG)), area);
}

fn render_right(f: &mut Frame, app: &App, area: Rect) {
    let (ar, ag, ab) = app.config.ui.accent_rgb();
    let accent = Color::Rgb(ar, ag, ab);
    let (row, col) = app.textarea.cursor();

    let words: usize = app.textarea.lines().iter()
        .map(|l| l.split_whitespace().count())
        .sum();
    let mins = (words / 200).max(1);
    let read_time = if words < 200 {
        format!("<1m")
    } else {
        format!("{mins}m")
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" {words}w {read_time}  "),
            Style::default().fg(TEXT_DIM).bg(BAR_BG),
        ),
        Span::styled(
            format!("{}:{} ", row + 1, col + 1),
            Style::default().fg(accent).bg(BAR_BG),
        ),
    ]);

    f.render_widget(
        Paragraph::new(line)
            .style(Style::default().bg(BAR_BG))
            .alignment(ratatui::layout::Alignment::Right),
        area,
    );
}
