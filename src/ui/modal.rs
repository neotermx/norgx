use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use std::path::PathBuf;

use crate::app::{App, Modal};

const BG:     Color = Color::Rgb(22,  24,  30);
const BORDER: Color = Color::Rgb(97,  175, 239);
const TEXT:   Color = Color::Rgb(200, 200, 210);
const DIM:    Color = Color::Rgb(120, 125, 140);
const GREEN:  Color = Color::Rgb(152, 195, 121);
const RED:    Color = Color::Rgb(224, 108, 117);
const YELLOW: Color = Color::Rgb(229, 192, 123);

pub fn render(f: &mut Frame, app: &App) {
    let Some(modal) = &app.modal else { return };
    let area = f.area();
    match modal {
        Modal::SaveBeforeQuit  => render_quit_confirm(f, area),
        Modal::SaveAs { input, cursor }   => render_input(f, "Save As",    "Enter the file path:",            input, *cursor, false, area),
        Modal::OpenFile { input, cursor, selected } => render_input(f, "Open File", "Enter a file path to open:", input, *cursor, *selected, area),
        Modal::NewFile { input, cursor }  => render_input(f, "New File",   "Enter a path for the new file:",  input, *cursor, false, area),
        Modal::PdfPath { input, bw } => render_pdf_path(f, input, *bw, area),
        Modal::Keybinds        => render_keybinds(f, area),
        Modal::SaveBeforeNew     => render_save_before_new(f, area),
        Modal::RenameFile { path, input, cursor } => {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            render_input(f, "Rename File", &format!("New name for '{name}':"), input, *cursor, false, area)
        }
        Modal::DeleteConfirm(path) => render_delete_confirm(f, path, area),
        Modal::Error(msg) => render_error(f, msg, area),
    }
}

// ── Save-before-quit (yes/no/cancel) ─────────────────────────────────────────

fn render_quit_confirm(f: &mut Frame, area: Rect) {
    let popup = centered_rect(54, 7, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Unsaved Changes ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Save changes before quitting?", Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("[Y]", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
            Span::styled("es   ", Style::default().fg(TEXT)),
            Span::styled("[N]", Style::default().fg(RED).add_modifier(Modifier::BOLD)),
            Span::styled("o   ", Style::default().fg(TEXT)),
            Span::styled("[Esc]", Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel", Style::default().fg(DIM)),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Text-input dialogs (Save As / Open File / PDF path) ───────────────────────

fn render_input(f: &mut Frame, title: &str, desc: &str, input: &str, cursor: usize, selected: bool, area: Rect) {
    let popup = centered_rect(66, 9, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let max_chars = inner.width.saturating_sub(10) as usize;
    let chars: Vec<char> = input.chars().collect();
    let char_count = chars.len();
    let cur = cursor.min(char_count);

    // Keep cursor visible: center viewport on cursor.
    let view_start = if char_count <= max_chars || selected {
        0
    } else {
        cur.saturating_sub(max_chars / 2).min(char_count - max_chars)
    };
    let view_end = (view_start + max_chars).min(char_count);

    let mut input_spans: Vec<Span> = vec![
        Span::raw("  "),
        Span::styled("Path: ", Style::default().fg(DIM)),
    ];

    if selected {
        let text: String = chars[view_start..view_end].iter().collect();
        input_spans.push(Span::styled(
            text,
            Style::default().fg(BG).bg(BORDER).add_modifier(Modifier::BOLD),
        ));
    } else if cur < char_count {
        if cur > view_start {
            let before: String = chars[view_start..cur].iter().collect();
            input_spans.push(Span::styled(before, Style::default().fg(Color::White)));
        }
        input_spans.push(Span::styled(
            chars[cur].to_string(),
            Style::default().fg(BG).bg(BORDER),
        ));
        if cur + 1 < view_end {
            let after: String = chars[cur + 1..view_end].iter().collect();
            input_spans.push(Span::styled(after, Style::default().fg(Color::White)));
        }
    } else {
        if view_start < char_count {
            let before: String = chars[view_start..char_count].iter().collect();
            input_spans.push(Span::styled(before, Style::default().fg(Color::White)));
        }
        input_spans.push(Span::styled("▌", Style::default().fg(BORDER)));
    }

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(desc, Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(input_spans),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Enter", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(" confirm   ", Style::default().fg(DIM)),
            Span::styled("Esc", Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel", Style::default().fg(DIM)),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Save before new scratch buffer ───────────────────────────────────────────

fn render_save_before_new(f: &mut Frame, area: Rect) {
    let popup = centered_rect(58, 7, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Unsaved Changes ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Save changes before opening scratch buffer?", Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("[Y]", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
            Span::styled("es   ", Style::default().fg(TEXT)),
            Span::styled("[N]", Style::default().fg(RED).add_modifier(Modifier::BOLD)),
            Span::styled("o — discard   ", Style::default().fg(TEXT)),
            Span::styled("[Esc]", Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel", Style::default().fg(DIM)),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Delete confirmation ───────────────────────────────────────────────────────

fn render_delete_confirm(f: &mut Frame, path: &PathBuf, area: Rect) {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("this file");
    let popup = centered_rect(60, 7, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Delete File ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(RED))
        .style(Style::default().bg(BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("Delete '{name}'? This cannot be undone."), Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("[Y]", Style::default().fg(RED).add_modifier(Modifier::BOLD)),
            Span::styled("es — delete   ", Style::default().fg(TEXT)),
            Span::styled("[Esc]", Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel", Style::default().fg(DIM)),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

// ── PDF path + color/B&W dialog ───────────────────────────────────────────────

fn render_pdf_path(f: &mut Frame, input: &str, bw: bool, area: Rect) {
    let popup = centered_rect(66, 11, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Export PDF ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let max_chars = inner.width.saturating_sub(10) as usize;
    let display: &str = if input.chars().count() > max_chars {
        let skip = input.chars().count() - max_chars;
        &input[input.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0)..]
    } else {
        input
    };

    let (color_style, bw_style) = if bw {
        (Style::default().fg(DIM), Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
    } else {
        (Style::default().fg(Color::White).add_modifier(Modifier::BOLD), Style::default().fg(DIM))
    };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Enter the destination PDF path:", Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Path: ", Style::default().fg(DIM)),
            Span::styled(display, Style::default().fg(Color::White)),
            Span::styled("▌", Style::default().fg(BORDER)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Mode: ", Style::default().fg(DIM)),
            Span::styled("Color", color_style),
            Span::styled("  /  ", Style::default().fg(DIM)),
            Span::styled("Black & White", bw_style),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Tab", Style::default().fg(BORDER).add_modifier(Modifier::BOLD)),
            Span::styled(" toggle mode   ", Style::default().fg(DIM)),
            Span::styled("Enter", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
            Span::styled(" confirm   ", Style::default().fg(DIM)),
            Span::styled("Esc", Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel", Style::default().fg(DIM)),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Keybinds reference ───────────────────────────────────────────────────────

fn render_keybinds(f: &mut Frame, area: Rect) {
    let popup = centered_rect(74, 27, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Keybindings ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let col_w = inner.width / 2;
    let left  = Rect::new(inner.x,         inner.y, col_w, inner.height);
    let right = Rect::new(inner.x + col_w, inner.y, inner.width - col_w, inner.height);

    let key  = Style::default().fg(BORDER).add_modifier(Modifier::BOLD);
    let desc = Style::default().fg(TEXT);
    let head = Style::default().fg(YELLOW).add_modifier(Modifier::BOLD);
    let hint = Style::default().fg(DIM);

    let left_lines = vec![
        Line::from(""),
        Line::from(Span::styled("  File", head)),
        Line::from(vec![Span::styled("  Ctrl+S        ", key), Span::styled("Save",                desc)]),
        Line::from(vec![Span::styled("  Ctrl+Shift+S  ", key), Span::styled("Save As",             desc)]),
        Line::from(vec![Span::styled("  Ctrl+N        ", key), Span::styled("New scratch buffer",  desc)]),
        Line::from(vec![Span::styled("  Ctrl+W        ", key), Span::styled("Close file",          desc)]),
        Line::from(vec![Span::styled("  Ctrl+O        ", key), Span::styled("Open file",           desc)]),
        Line::from(vec![Span::styled("  Ctrl+Q        ", key), Span::styled("Quit",                desc)]),
        Line::from(""),
        Line::from(Span::styled("  Journal & Export", head)),
        Line::from(vec![Span::styled("  Ctrl+J        ", key), Span::styled("Open journal",        desc)]),
        Line::from(vec![Span::styled("  Ctrl+P        ", key), Span::styled("Export PDF",          desc)]),
        Line::from(""),
        Line::from(Span::styled("  View", head)),
        Line::from(vec![Span::styled("  Ctrl+B        ", key), Span::styled("Focus file tree",     desc)]),
        Line::from(vec![Span::styled("  Ctrl+\\       ", key), Span::styled("Collapse/expand tree",desc)]),
        Line::from(vec![Span::styled("  Ctrl+/        ", key), Span::styled("This screen",         desc)]),
        Line::from(vec![Span::styled("  Esc           ", key), Span::styled("Help overlay",        desc)]),
        Line::from(""),
        Line::from(vec![Span::raw("  "), Span::styled("any key  close", hint)]),
    ];

    let right_lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Navigation", head)),
        Line::from(vec![Span::styled("  ]h / [h   ", key), Span::styled("Next / prev heading",  desc)]),
        Line::from(vec![Span::styled("  gf        ", key), Span::styled("Follow link",          desc)]),
        Line::from(vec![Span::styled("  gb        ", key), Span::styled("Go back",              desc)]),
        Line::from(vec![Span::styled("  za        ", key), Span::styled("Toggle fold",          desc)]),
        Line::from(vec![Span::styled("  Ctrl+F    ", key), Span::styled("Search in file",       desc)]),
        Line::from(""),
        Line::from(Span::styled("  Editing", head)),
        Line::from(vec![Span::styled("  Ctrl+T    ", key), Span::styled("Cycle todo state",     desc)]),
        Line::from(vec![Span::styled("  Ctrl+Z    ", key), Span::styled("Undo",                 desc)]),
        Line::from(vec![Span::styled("  Ctrl+Y    ", key), Span::styled("Redo",                 desc)]),
        Line::from(vec![Span::styled("  Ctrl+C    ", key), Span::styled("Copy",                 desc)]),
        Line::from(vec![Span::styled("  Ctrl+X    ", key), Span::styled("Cut",                  desc)]),
        Line::from(vec![Span::styled("  Ctrl+V    ", key), Span::styled("Paste",                desc)]),
        Line::from(""),
        Line::from(Span::styled("  File Tree (when focused)", head)),
        Line::from(vec![Span::styled("  j / k     ", key), Span::styled("Move up / down",       desc)]),
        Line::from(vec![Span::styled("  Enter     ", key), Span::styled("Open selection",        desc)]),
        Line::from(vec![Span::styled("  r         ", key), Span::styled("Rename file",           desc)]),
        Line::from(vec![Span::styled("  d         ", key), Span::styled("Delete file",           desc)]),
        Line::from(vec![Span::styled("  R         ", key), Span::styled("Refresh tree",          desc)]),
        Line::from(vec![Span::styled("  /         ", key), Span::styled("Fuzzy search",          desc)]),
    ];

    f.render_widget(Paragraph::new(left_lines),  left);
    f.render_widget(Paragraph::new(right_lines), right);
}

// ── Error notification ────────────────────────────────────────────────────────

fn render_error(f: &mut Frame, msg: &str, area: Rect) {
    let popup = centered_rect(62, 7, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Error ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(RED))
        .style(Style::default().bg(BG));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(msg, Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("any key", Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)),
            Span::styled(" dismiss", Style::default().fg(DIM)),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Layout helper ─────────────────────────────────────────────────────────────

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
