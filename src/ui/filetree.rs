use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use crate::input::Focus;

const BG: Color = Color::Rgb(30, 32, 38);
const BORDER: Color = Color::Rgb(60, 65, 75);
const TEXT_DIM: Color = Color::Rgb(120, 125, 140);
const SEL_BG: Color = Color::Rgb(40, 44, 52);

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let (ar, ag, ab) = app.config.ui.accent_rgb();
    let accent = Color::Rgb(ar, ag, ab);
    let focused = app.focus() == &Focus::FileTree;
    let border_color = if focused { accent } else { BORDER };

    let (title, title_style) = if app.file_tree.fuzzy_active {
        (
            " Search ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            " Files ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title)
        .title_style(title_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.file_tree.fuzzy_active {
        render_fuzzy(f, app, inner, accent);
    } else {
        render_tree(f, app, inner, accent);
    }
}

// ── Normal tree view ──────────────────────────────────────────────────────────

fn render_tree(f: &mut Frame, app: &App, area: Rect, accent: Color) {
    let entries = app.file_tree.entries();
    let recent = &app.file_tree.recent;
    let selected = app.file_tree.selected;
    let height = area.height as usize;
    let inner_width = area.width as usize;
    let rlen = recent.len();

    if entries.is_empty() && recent.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No .norg files",
                Style::default().fg(TEXT_DIM).bg(BG),
            )))
            .style(Style::default().bg(BG)),
            area,
        );
        return;
    }

    // Total visual lines including non-selectable header and separator rows.
    let total_visual = if rlen > 0 {
        1 + rlen + 1 + entries.len()  // "Recent" header + files + separator + tree
    } else {
        entries.len()
    };

    // Visual row of the currently selected item.
    let visual_selected = if rlen > 0 {
        if selected < rlen { 1 + selected } else { 1 + rlen + 1 + (selected - rlen) }
    } else {
        selected
    };

    let scroll = if total_visual <= height {
        0
    } else {
        let half = height / 2;
        let max = total_visual.saturating_sub(height);
        visual_selected.saturating_sub(half).min(max)
    };

    let mut lines: Vec<Line> = Vec::new();

    // ── Recent section ────────────────────────────────────────────────────────
    if rlen > 0 {
        lines.push(Line::from(Span::styled(
            "  \u{F017} Recent",
            Style::default().fg(TEXT_DIM).add_modifier(Modifier::BOLD).bg(BG),
        )));

        for (i, path) in recent.iter().enumerate() {
            let is_sel = i == selected;
            let bg = if is_sel { SEL_BG } else { BG };

            let display = path
                .strip_prefix(&app.file_tree.root)
                .ok()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string()
                });

            // "  ▐ " or "    " (2) + icon (2 wide + 1 space) = 5 cols prefix
            let name_width = inner_width.saturating_sub(5);
            let name: String = if is_sel {
                let overflow = display.chars().count().saturating_sub(name_width);
                let offset = marquee_offset(app.marquee_offset, overflow);
                display.chars().skip(offset).take(name_width).collect()
            } else {
                display.chars().take(name_width).collect()
            };

            lines.push(Line::from(vec![
                if is_sel {
                    Span::styled("▐ ", Style::default().fg(accent).bg(bg))
                } else {
                    Span::styled("  ", Style::default().bg(bg))
                },
                Span::styled(
                    format!("\u{E847} {name}"),
                    if is_sel {
                        Style::default().fg(accent).add_modifier(Modifier::BOLD).bg(bg)
                    } else {
                        Style::default().fg(TEXT_DIM).bg(bg)
                    },
                ),
            ]));
        }

        // Separator between recent and workspace tree.
        let sep_w = inner_width.saturating_sub(2);
        lines.push(Line::from(Span::styled(
            format!("  {}", "─".repeat(sep_w)),
            Style::default().fg(BORDER).bg(BG),
        )));
    }

    // ── Workspace tree ────────────────────────────────────────────────────────
    for (i, entry) in entries.iter().enumerate() {
        let sel_idx = rlen + i;
        let is_sel = sel_idx == selected;
        let bg = if is_sel { SEL_BG } else { BG };

        let indent = "  ".repeat(entry.depth);
        let icon = if entry.is_dir {
            if app.file_tree.is_expanded(&entry.path) { "\u{EAF7} " } else { "\u{F024B} " }
        } else {
            "\u{E847} "
        };

        let prefix = indent.len() + 2 + 3;
        let name_width = inner_width.saturating_sub(prefix);
        let name: String = if is_sel {
            let overflow = entry.name.chars().count().saturating_sub(name_width);
            let offset = marquee_offset(app.marquee_offset, overflow);
            entry.name.chars().skip(offset).take(name_width).collect()
        } else {
            entry.name.chars().take(name_width).collect()
        };

        let text_style = if is_sel {
            Style::default().fg(accent).add_modifier(Modifier::BOLD).bg(bg)
        } else if entry.is_dir {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD).bg(bg)
        } else {
            Style::default().fg(TEXT_DIM).bg(bg)
        };

        lines.push(Line::from(vec![
            Span::styled(indent, Style::default().bg(bg)),
            if is_sel {
                Span::styled("▐ ", Style::default().fg(accent).bg(bg))
            } else {
                Span::styled("  ", Style::default().bg(bg))
            },
            Span::styled(format!("{icon}{name}"), text_style),
        ]));
    }

    f.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(BG))
            .scroll((scroll as u16, 0)),
        area,
    );
}

// ── Marquee helper ────────────────────────────────────────────────────────────

/// Convert a raw monotonic tick counter into a char offset for the selected
/// entry, implementing: initial pause → scroll left → hold at end → loop.
///
/// - `LEAD`: ticks before scrolling starts  (~1 s at 120 ms/tick)
/// - `TRAIL`: ticks held at the end         (~0.7 s)
fn marquee_offset(raw: u64, overflow: usize) -> usize {
    const LEAD: u64 = 8;
    const TRAIL: u64 = 6;

    if overflow == 0 || raw < LEAD {
        return 0;
    }
    let t = raw - LEAD;
    let cycle = overflow as u64 + TRAIL;
    let pos = t % cycle;
    pos.min(overflow as u64) as usize
}

// ── Fuzzy search view ─────────────────────────────────────────────────────────

fn render_fuzzy(f: &mut Frame, app: &App, area: Rect, accent: Color) {
    let query = &app.file_tree.fuzzy_query;
    let matches = app.file_tree.fuzzy_matches();
    let selected = app.file_tree.fuzzy_selected;
    let height = area.height as usize;
    let width = area.width as usize;

    // Line 0: search input. Line 1: separator. Remaining: results.
    let result_height = height.saturating_sub(2);

    let scroll = if matches.len() <= result_height {
        0
    } else {
        let half = result_height / 2;
        let max = matches.len().saturating_sub(result_height);
        selected.saturating_sub(half).min(max)
    };

    let mut lines: Vec<Line> = Vec::new();

    // Search input line
    lines.push(Line::from(vec![
        Span::styled("  / ", Style::default().fg(accent).add_modifier(Modifier::BOLD).bg(BG)),
        Span::styled(
            format!("{query}▌"),
            Style::default().fg(Color::White).bg(BG),
        ),
    ]));

    // Separator
    let sep_width = width.saturating_sub(2);
    lines.push(Line::from(Span::styled(
        format!("  {}", "─".repeat(sep_width)),
        Style::default().fg(BORDER).bg(BG),
    )));

    if matches.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no matches",
            Style::default().fg(TEXT_DIM).bg(BG),
        )));
    } else {
        for (i, entry) in matches.iter().enumerate().skip(scroll).take(result_height) {
            let is_sel = i == selected;
            let bg = if is_sel { SEL_BG } else { BG };

            // Show relative path from root for disambiguation.
            let display = entry.path
                .strip_prefix(&app.file_tree.root)
                .ok()
                .and_then(|p| p.to_str())
                .unwrap_or(&entry.name)
                .to_string();

            // prefix: "  ▐ " (4 cols) or "    " (4 cols)
            let name_width = width.saturating_sub(4);
            let name: String = display.chars().take(name_width).collect();

            lines.push(Line::from(vec![
                if is_sel {
                    Span::styled("  ▐ ", Style::default().fg(accent).bg(bg))
                } else {
                    Span::styled("    ", Style::default().bg(bg))
                },
                Span::styled(
                    name,
                    if is_sel {
                        Style::default().fg(accent).add_modifier(Modifier::BOLD).bg(bg)
                    } else {
                        Style::default().fg(TEXT_DIM).bg(bg)
                    },
                ),
            ]));
        }
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(BG)),
        area,
    );
}
