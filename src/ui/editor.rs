use std::collections::{HashMap, HashSet};

use unicode_width::UnicodeWidthChar as _;

use ratatui::{
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use crate::config::UiConfig;
use crate::input::Focus;
use crate::norg::fold_body_range;
use crate::norg::parser::{parse_inline, parse_line, BlockToken, InlineToken};

// ── Non-configurable palette ──────────────────────────────────────────────────

const BG_DIM: Color = Color::Rgb(30, 32, 38);
const CODE_BG: Color = Color::Rgb(24, 26, 32);
const CURSOR_LINE_BG: Color = Color::Rgb(40, 44, 52);
const SEL_BG: Color = Color::Rgb(73, 85, 102);
const BORDER: Color = Color::Rgb(60, 65, 75);
const TEXT_DIM: Color = Color::Rgb(120, 125, 140);
const SEARCH_BAR_BG: Color = Color::Rgb(22, 24, 30);
const SEARCH_MATCH_BG: Color = Color::Rgb(90, 70, 0);
const SEARCH_CURRENT_BG: Color = Color::Rgb(200, 155, 0);
const FOLD_BG: Color = Color::Rgb(35, 38, 46);

// ── Render context (per-frame config snapshot) ────────────────────────────────

struct RenderCtx<'a> {
    tab_width: usize,
    line_numbers: bool,
    relative_line_numbers: bool,
    cursor_source_row: usize,
    conceal_headings: bool,
    conceal_todos: bool,
    conceal_links: bool,
    cursor_fg: Color,
    cursor_bg: Color,
    ui: &'a UiConfig,
    search_query_len: usize,
    search_matches: Vec<(usize, usize)>,
    search_current: usize,
    show_todo_progress: bool,
    /// Maps heading row → (done_count, total_count) for todos under that heading.
    todo_counts: HashMap<usize, (usize, usize)>,
    /// False for non-.norg files — disables all Norg-specific rendering.
    is_norg: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let (ar, ag, ab) = app.config.ui.accent_rgb();
    let accent = Color::Rgb(ar, ag, ab);

    if app.show_splash {
        super::splash::render(f, app, area);
        return;
    }


    // Snapshot search state as owned data before any borrows of app.
    let search_active = app.search_active;
    let search_query_len = app.search_query.chars().count();
    // Only pass matches while search is open; clear highlights once dismissed.
    let search_matches = if search_active { app.search_matches.clone() } else { Vec::new() };
    let search_current = app.search_current;

    let title = build_title(app);
    let border_color = if app.focus() == &Focus::Editor { accent } else { BORDER };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title)
        .title_style(Style::default().fg(accent).add_modifier(Modifier::BOLD));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let search_bar_rows: u16 = if search_active && inner.height > 1 { 1 } else { 0 };
    let editor_height = inner.height.saturating_sub(search_bar_rows) as usize;
    let width = inner.width as usize;
    if editor_height == 0 || width == 0 {
        return;
    }

    // Cursor info — short-lived borrows; results are owned values.
    let (cursor_row, cursor_char_col) = app.textarea.cursor();
    let total_lines = app.textarea.lines().len();

    let tab_width_val = app.config.editor.tab_width as usize;
    let line_numbers = app.config.editor.line_numbers;
    let relative_line_numbers = app.config.editor.relative_line_numbers;
    let line_wrap = app.config.editor.line_wrap;
    let lnum_len = if line_numbers { num_digits(total_lines.max(1)) } else { 0 };
    let lnum_total = if line_numbers { lnum_len + 2 } else { 0 };

    let content_width = width.saturating_sub(lnum_total);
    if content_width == 0 {
        return;
    }

    let cursor_display_col = app
        .textarea
        .lines()
        .get(cursor_row)
        .map(|s| char_to_display_col(s, cursor_char_col, tab_width_val))
        .unwrap_or(0);

    // Build display map before mutations so we can compute display_cursor_row.
    let display_map = {
        let lines = app.textarea.lines();
        let base = build_display_map(lines, &app.folded_headings);
        if line_wrap {
            expand_wrapped_lines(base, lines, content_width, tab_width_val)
        } else {
            base
        }
    };

    let display_cursor_row = if line_wrap {
        display_map.iter().position(|d| match d {
            DisplayRow::Actual(r) => *r == cursor_row && cursor_display_col < content_width,
            DisplayRow::Continuation { source_row, display_start, indent_cols } => {
                let seg_w = content_width.saturating_sub(*indent_cols).max(1);
                *source_row == cursor_row
                    && cursor_display_col >= *display_start
                    && cursor_display_col < *display_start + seg_w
            }
            _ => false,
        }).unwrap_or(0)
    } else {
        display_map.iter().position(|d| matches!(d, DisplayRow::Actual(r) if *r == cursor_row)).unwrap_or(0)
    };

    // Mutations must happen before ctx borrows app.config.ui.
    app.row_offset = scroll_to(app.row_offset, display_cursor_row, editor_height);
    if line_wrap {
        app.col_offset = 0;
    } else {
        app.col_offset = scroll_to(app.col_offset, cursor_display_col, content_width);
    }

    let row_offset = app.row_offset;
    let col_offset = app.col_offset;

    let todo_counts = if app.is_norg { compute_todo_counts(app.textarea.lines()) } else { HashMap::new() };

    let ctx = RenderCtx {
        tab_width: tab_width_val,
        line_numbers,
        relative_line_numbers,
        cursor_source_row: cursor_row,
        conceal_headings: app.config.editor.conceal_headings,
        conceal_todos: app.config.editor.conceal_todos,
        conceal_links: app.config.editor.conceal_links,
        cursor_fg: BG_DIM,
        cursor_bg: accent,
        ui: &app.config.ui,
        search_query_len,
        search_matches,
        search_current,
        show_todo_progress: app.config.editor.show_todo_progress,
        todo_counts,
        is_norg: app.is_norg,
    };

    let code_lines = precompute_code_lines(app.textarea.lines());
    let selection = app.textarea.selection_range();
    let all_lines = app.textarea.lines();

    let editor_area = Rect { height: editor_height as u16, ..inner };

    let rendered: Vec<Line> = (0..editor_height)
        .map(|i| {
            match display_map.get(row_offset + i) {
                None => Line::from(Span::styled(
                    " ".repeat(width),
                    Style::default().bg(BG_DIM),
                )),
                Some(DisplayRow::Actual(row)) => {
                    let row = *row;
                    let text = all_lines[row].as_str();
                    let is_cursor = row == cursor_row;
                    let in_code = code_lines.get(row).copied().unwrap_or(false);
                    let bg = if is_cursor {
                        CURSOR_LINE_BG
                    } else if in_code {
                        CODE_BG
                    } else {
                        BG_DIM
                    };
                    build_line(
                        row,
                        text,
                        bg,
                        if is_cursor { Some(cursor_char_col) } else { None },
                        col_offset,
                        content_width,
                        in_code,
                        is_cursor,
                        selection,
                        lnum_len,
                        &ctx,
                    )
                }
                Some(DisplayRow::Continuation { source_row, display_start, indent_cols }) => {
                    let source_row = *source_row;
                    let display_start = *display_start;
                    let indent_cols = *indent_cols;
                    let text = all_lines[source_row].as_str();
                    let is_cursor = source_row == cursor_row;
                    let in_code = code_lines.get(source_row).copied().unwrap_or(false);
                    let bg = if is_cursor { CURSOR_LINE_BG } else if in_code { CODE_BG } else { BG_DIM };
                    let seg_width = content_width.saturating_sub(indent_cols).max(1);
                    let cursor_in_seg = is_cursor
                        && cursor_display_col >= display_start
                        && cursor_display_col < display_start + seg_width;

                    let mut row_spans: Vec<Span> = Vec::new();
                    if ctx.line_numbers {
                        row_spans.push(Span::styled(
                            format!("{:>width$} ", "↳", width = lnum_len + 1),
                            Style::default().fg(TEXT_DIM).bg(bg),
                        ));
                    }
                    if indent_cols > 0 {
                        row_spans.push(Span::styled(
                            " ".repeat(indent_cols),
                            Style::default().bg(bg),
                        ));
                    }
                    let mut spans = content_spans(source_row, text, bg, in_code, is_cursor, &ctx);
                    spans = apply_search_highlights(spans, source_row, &ctx.search_matches, ctx.search_current, ctx.search_query_len);
                    if let Some(sel) = selection {
                        spans = apply_selection(spans, source_row, sel, text.chars().count());
                    }
                    if cursor_in_seg {
                        spans = apply_cursor(spans, cursor_char_col, &ctx);
                    }
                    row_spans.extend(clip(spans, display_start, seg_width, ctx.tab_width));
                    Line::from(row_spans)
                }
                Some(DisplayRow::FoldIndicator { count }) => {
                    build_fold_indicator(*count, lnum_len, accent, &ctx)
                }
            }
        })
        .collect();

    f.render_widget(
        Paragraph::new(rendered).style(Style::default().bg(BG_DIM)),
        editor_area,
    );

    if search_active && search_bar_rows > 0 {
        let bar_area = Rect { y: inner.y + editor_height as u16, height: 1, ..inner };
        render_search_bar(f, app, bar_area, accent);
        let qw = app.search_query.chars().count() as u16;
        f.set_cursor_position(Position { x: inner.x + 4 + qw, y: bar_area.y });
    } else if display_cursor_row >= row_offset
        && display_cursor_row < row_offset + editor_height
    {
        let screen_y = inner.y + (display_cursor_row - row_offset) as u16;
        let cursor_vis_col = if line_wrap {
            display_map.iter().find_map(|d| match d {
                DisplayRow::Actual(r) if *r == cursor_row && cursor_display_col < content_width => {
                    Some(cursor_display_col)
                }
                DisplayRow::Continuation { source_row, display_start, indent_cols }
                    if *source_row == cursor_row && cursor_display_col >= *display_start => {
                    Some(indent_cols + (cursor_display_col - *display_start))
                }
                _ => None,
            }).unwrap_or(cursor_display_col % content_width)
        } else {
            cursor_display_col.saturating_sub(col_offset)
        };
        let screen_x = inner.x + lnum_total as u16 + cursor_vis_col as u16;
        f.set_cursor_position(Position { x: screen_x, y: screen_y });
    }
}

// ── Search bar ────────────────────────────────────────────────────────────────

fn render_search_bar(f: &mut Frame, app: &App, area: Rect, accent: Color) {
    let query = &app.search_query;
    let match_count = app.search_matches.len();
    let current = if match_count > 0 { app.search_current + 1 } else { 0 };

    let counter = if match_count == 0 && !query.is_empty() {
        " (no matches)".to_string()
    } else if match_count > 0 {
        format!(" [{}/{}]", current, match_count)
    } else {
        String::new()
    };

    let spans = vec![
        Span::styled("  / ", Style::default().fg(accent).add_modifier(Modifier::BOLD).bg(SEARCH_BAR_BG)),
        Span::styled(query.clone(), Style::default().fg(Color::White).bg(SEARCH_BAR_BG)),
        Span::styled("▌", Style::default().fg(accent).bg(SEARCH_BAR_BG)),
        Span::styled(counter, Style::default().fg(TEXT_DIM).bg(SEARCH_BAR_BG)),
    ];

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(SEARCH_BAR_BG)),
        area,
    );
}

// ── Line builder ──────────────────────────────────────────────────────────────

fn build_line(
    row: usize,
    text: &str,
    bg: Color,
    cursor_col: Option<usize>,
    col_offset: usize,
    content_width: usize,
    in_code: bool,
    is_cursor: bool,
    selection: Option<((usize, usize), (usize, usize))>,
    lnum_len: usize,
    ctx: &RenderCtx,
) -> Line<'static> {
    let mut line_spans: Vec<Span> = Vec::new();

    if ctx.line_numbers {
        let gutter = if ctx.relative_line_numbers && row != ctx.cursor_source_row {
            let dist = (row as isize - ctx.cursor_source_row as isize).unsigned_abs();
            format!("{:>width$} ", dist, width = lnum_len + 1)
        } else {
            format!("{:>width$} ", row + 1, width = lnum_len + 1)
        };
        line_spans.push(Span::styled(gutter, Style::default().fg(TEXT_DIM).bg(bg)));
    }

    let mut spans = content_spans(row, text, bg, in_code, is_cursor, ctx);

    spans = apply_search_highlights(spans, row, &ctx.search_matches, ctx.search_current, ctx.search_query_len);

    if let Some(sel) = selection {
        spans = apply_selection(spans, row, sel, text.chars().count());
    }

    if let Some(col) = cursor_col {
        spans = apply_cursor(spans, col, ctx);
    }

    line_spans.extend(clip(spans, col_offset, content_width, ctx.tab_width));
    Line::from(line_spans)
}

// ── Content span builders ─────────────────────────────────────────────────────

fn content_spans(
    row: usize,
    text: &str,
    bg: Color,
    in_code: bool,
    is_cursor: bool,
    ctx: &RenderCtx,
) -> Vec<(String, Style)> {
    if !ctx.is_norg {
        return vec![(text.to_string(), Style::default().fg(Color::White).bg(bg))];
    }

    match parse_line(text) {
        BlockToken::Heading { level, text: heading_text } => {
            let (r, g, b) = ctx.ui.heading_color_rgb(level);
            let heading_fg = Color::Rgb(r, g, b);
            let heading_style = Style::default()
                .fg(heading_fg)
                .add_modifier(Modifier::BOLD)
                .bg(bg);

            let prefix = if is_cursor || !ctx.conceal_headings {
                format!("{} ", "*".repeat(level as usize))
            } else {
                format!(
                    "{}{} ",
                    " ".repeat(level as usize - 1),
                    ctx.ui.heading_glyph_char(level),
                )
            };

            let mut spans = vec![(prefix, heading_style)];
            // Parse heading body for inline tokens (links, bold, italic) so
            // link concealment applies inside headings too.
            for (s, style) in inline_spans(&heading_text, bg, Some(heading_fg), is_cursor, ctx) {
                spans.push((s, style.add_modifier(Modifier::BOLD)));
            }
            // Append todo progress summary if this heading has todo children.
            if ctx.show_todo_progress {
                if let Some(&(done, total)) = ctx.todo_counts.get(&row) {
                    if total > 0 {
                        spans.push((
                            format!(" ({done}/{total})"),
                            Style::default().fg(TEXT_DIM).bg(bg),
                        ));
                    }
                }
            }
            spans
        }
        BlockToken::TodoOpen(_) => vec![(
            if ctx.conceal_todos { conceal_todo(text) } else { text.to_string() },
            Style::default().fg(Color::Yellow).bg(bg),
        )],
        BlockToken::TodoDone(_) => vec![(
            if ctx.conceal_todos { conceal_todo(text) } else { text.to_string() },
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::DIM)
                .bg(bg),
        )],
        BlockToken::TodoPending(_) => vec![(
            if ctx.conceal_todos { conceal_todo(text) } else { text.to_string() },
            Style::default().fg(Color::Rgb(200, 120, 50)).bg(bg),
        )],
        BlockToken::CodeFenceOpen { .. } | BlockToken::CodeFenceClose => {
            vec![(text.to_string(), Style::default().fg(TEXT_DIM).bg(bg))]
        }
        _ if in_code => {
            vec![(text.to_string(), Style::default().fg(Color::White).bg(bg))]
        }
        _ => inline_spans(text, bg, None, is_cursor, ctx),
    }
}

/// Parses inline tokens and returns styled `(text, style)` pairs.
/// Delimiters (`*`, `/`) are kept as dimmed spans so cursor alignment is preserved.
/// Links with labels are concealed to `[label]` on non-cursor lines when enabled.
fn inline_spans(text: &str, bg: Color, base_fg: Option<Color>, is_cursor: bool, ctx: &RenderCtx) -> Vec<(String, Style)> {
    let plain = Style::default().fg(base_fg.unwrap_or(Color::White)).bg(bg);
    let delim = Style::default().fg(TEXT_DIM).bg(bg);
    let bold = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
        .bg(bg);
    let italic = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::ITALIC)
        .bg(bg);
    let link = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::UNDERLINED)
        .bg(bg);

    parse_inline(text)
        .into_iter()
        .flat_map(|tok| match tok {
            InlineToken::Plain(s) => vec![(s, plain)],
            InlineToken::Bold(s) => {
                vec![("*".to_string(), delim), (s, bold), ("*".to_string(), delim)]
            }
            InlineToken::Italic(s) => {
                vec![("/".to_string(), delim), (s, italic), ("/".to_string(), delim)]
            }
            InlineToken::Link { path, label } => {
                if ctx.conceal_links && !is_cursor {
                    match label {
                        // Show only [label]; {:path:} chars are omitted from display.
                        // Column math for search/selection may drift on these lines, but
                        // the cursor line always shows full text so editing is unaffected.
                        Some(lbl) => vec![(format!("[{lbl}]"), link)],
                        None => vec![(format!("{{:{path}:}}"), link)],
                    }
                } else {
                    match label {
                        Some(lbl) => vec![(format!("{{:{path}:}}[{lbl}]"), link)],
                        None => vec![(format!("{{:{path}:}}"), link)],
                    }
                }
            }
        })
        .collect()
}

// ── Span transformers ─────────────────────────────────────────────────────────

fn apply_cursor(
    spans: Vec<(String, Style)>,
    cursor_col: usize,
    ctx: &RenderCtx,
) -> Vec<(String, Style)> {
    let cursor_style = Style::new().fg(ctx.cursor_fg).bg(ctx.cursor_bg);
    let mut result = Vec::with_capacity(spans.len() + 2);
    let mut col = 0;
    let mut placed = false;

    for (text, style) in spans {
        let char_count = text.chars().count();
        let span_start = col;
        let span_end = col + char_count;

        if !placed && cursor_col >= span_start && cursor_col < span_end {
            let local = cursor_col - span_start;
            let chars: Vec<(usize, char)> = text.char_indices().collect();
            let (byte_start, ch) = chars[local];
            let byte_end = byte_start + ch.len_utf8();

            if byte_start > 0 {
                result.push((text[..byte_start].to_string(), style));
            }
            result.push((ch.to_string(), cursor_style));
            if byte_end < text.len() {
                result.push((text[byte_end..].to_string(), style));
            }
            placed = true;
        } else {
            result.push((text, style));
        }

        col = span_end;
    }

    if !placed {
        result.push((" ".to_string(), cursor_style));
    }

    result
}

fn apply_range_highlight(
    spans: Vec<(String, Style)>,
    hl_start: usize,
    hl_end: usize,
    bg: Color,
) -> Vec<(String, Style)> {
    if hl_start >= hl_end {
        return spans;
    }
    let mut result = Vec::with_capacity(spans.len() + 4);
    let mut col = 0;
    for (text, style) in spans {
        let char_count = text.chars().count();
        let span_start = col;
        let span_end = col + char_count;
        col = span_end;
        let overlap_start = hl_start.max(span_start);
        let overlap_end = hl_end.min(span_end);
        if overlap_start >= overlap_end {
            result.push((text, style));
            continue;
        }
        let hl_style = style.bg(bg);
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let text_len = text.len();
        let pre = overlap_start - span_start;
        let hl_len = overlap_end - overlap_start;
        let pre_byte = if pre == 0 { 0 } else { chars[pre].0 };
        let hl_byte_end = if pre + hl_len >= chars.len() { text_len } else { chars[pre + hl_len].0 };
        if pre_byte > 0 {
            result.push((text[..pre_byte].to_string(), style));
        }
        result.push((text[pre_byte..hl_byte_end].to_string(), hl_style));
        if hl_byte_end < text_len {
            result.push((text[hl_byte_end..].to_string(), style));
        }
    }
    result
}

fn apply_search_highlights(
    spans: Vec<(String, Style)>,
    row: usize,
    matches: &[(usize, usize)],
    current: usize,
    query_len: usize,
) -> Vec<(String, Style)> {
    if query_len == 0 || matches.is_empty() {
        return spans;
    }
    let mut result = spans;
    for (i, &(r, c)) in matches.iter().enumerate() {
        if r != row {
            continue;
        }
        let bg = if i == current { SEARCH_CURRENT_BG } else { SEARCH_MATCH_BG };
        result = apply_range_highlight(result, c, c + query_len, bg);
    }
    result
}

fn apply_selection(
    spans: Vec<(String, Style)>,
    row: usize,
    sel: ((usize, usize), (usize, usize)),
    line_char_count: usize,
) -> Vec<(String, Style)> {
    let ((start_row, start_col), (end_row, end_col)) = sel;

    let (sel_start, sel_end) = if row < start_row || row > end_row {
        return spans;
    } else if start_row == row && end_row == row {
        (start_col, end_col)
    } else if row == start_row {
        (start_col, line_char_count)
    } else if row == end_row {
        (0, end_col)
    } else {
        (0, line_char_count)
    };

    apply_range_highlight(spans, sel_start, sel_end, SEL_BG)
}

/// Clip spans to `[col_offset, col_offset + max_width)`, expanding tabs and
/// padding wide chars that straddle boundaries.
fn clip(
    spans: Vec<(String, Style)>,
    col_offset: usize,
    max_width: usize,
    tab_width: usize,
) -> Vec<Span<'static>> {
    let window_end = col_offset + max_width;
    let mut result = Vec::new();
    let mut display_col = 0usize;

    for (text, style) in spans {
        if display_col >= window_end {
            break;
        }
        let mut visible = String::new();

        for c in text.chars() {
            if display_col >= window_end {
                break;
            }
            let w = char_display_width(c, display_col, tab_width);
            let char_end = display_col + w;

            if char_end <= col_offset {
                display_col = char_end;
                continue;
            }

            let vis_w = char_end.min(window_end) - display_col.max(col_offset);

            if c == '\t' {
                visible.push_str(&" ".repeat(vis_w));
            } else if w > 1 && vis_w < w {
                visible.push_str(&" ".repeat(vis_w));
            } else {
                visible.push(c);
            }

            display_col = char_end;
        }

        if !visible.is_empty() {
            result.push(Span::styled(visible, style));
        }
    }

    result
}

// ── Fold display map ──────────────────────────────────────────────────────────

pub(crate) enum DisplayRow {
    Actual(usize),
    FoldIndicator { count: usize },
    Continuation { source_row: usize, display_start: usize, indent_cols: usize },
}

pub(crate) fn build_display_map(lines: &[String], folded: &HashSet<usize>) -> Vec<DisplayRow> {
    if folded.is_empty() {
        return (0..lines.len()).map(DisplayRow::Actual).collect();
    }
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let mut map = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if folded.contains(&i) {
            let range = fold_body_range(&refs, i);
            map.push(DisplayRow::Actual(i));
            if !range.is_empty() {
                map.push(DisplayRow::FoldIndicator { count: range.len() });
            }
            i = range.end;
        } else {
            map.push(DisplayRow::Actual(i));
            i += 1;
        }
    }
    map
}

pub(crate) fn expand_wrapped_lines(
    map: Vec<DisplayRow>,
    lines: &[String],
    content_width: usize,
    tab_width: usize,
) -> Vec<DisplayRow> {
    if content_width == 0 {
        return map;
    }
    let mut result = Vec::new();
    for dr in map {
        match dr {
            DisplayRow::Actual(row) => {
                result.push(DisplayRow::Actual(row));
                if let Some(line) = lines.get(row) {
                    let total = char_to_display_col(line, line.chars().count(), tab_width);
                    let indent_cols = leading_indent_width(line, tab_width);
                    // Continuation rows are narrower by indent_cols to allow the hanging indent prefix.
                    let seg_w = content_width.saturating_sub(indent_cols).max(1);
                    let mut start = content_width;
                    while start < total {
                        result.push(DisplayRow::Continuation {
                            source_row: row,
                            display_start: start,
                            indent_cols,
                        });
                        start += seg_w;
                    }
                }
            }
            other => result.push(other),
        }
    }
    result
}

fn build_fold_indicator(count: usize, lnum_len: usize, accent: Color, ctx: &RenderCtx) -> Line<'static> {
    let fold_style = Style::default()
        .fg(accent)
        .add_modifier(Modifier::DIM)
        .bg(FOLD_BG);

    let mut spans: Vec<Span> = Vec::new();
    if ctx.line_numbers {
        spans.push(Span::styled(
            format!("{:>width$} ", "·", width = lnum_len + 1),
            Style::default().fg(TEXT_DIM).bg(FOLD_BG),
        ));
    }
    spans.push(Span::styled(
        format!(" ⋯  {} line{} folded", count, if count == 1 { "" } else { "s" }),
        fold_style,
    ));
    Line::from(spans)
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn precompute_code_lines(lines: &[String]) -> Vec<bool> {
    let mut in_code = false;
    lines
        .iter()
        .map(|line| match parse_line(line.as_str()) {
            BlockToken::CodeFenceOpen { .. } => {
                in_code = true;
                true
            }
            BlockToken::CodeFenceClose => {
                in_code = false;
                true
            }
            _ => in_code,
        })
        .collect()
}

/// Maps each heading row to (done_count, total_todo_count) for all todos
/// that fall under it, down to the next heading of equal or higher level.
fn compute_todo_counts(lines: &[impl AsRef<str>]) -> HashMap<usize, (usize, usize)> {
    let mut result: HashMap<usize, (usize, usize)> = HashMap::new();
    let mut heading_stack: Vec<(usize, u8)> = vec![];

    for (row, line) in lines.iter().enumerate() {
        match parse_line(line.as_ref()) {
            BlockToken::Heading { level, .. } => {
                heading_stack.retain(|&(_, l)| l < level);
                heading_stack.push((row, level));
            }
            BlockToken::TodoDone(_) => {
                for &(hrow, _) in &heading_stack {
                    let e = result.entry(hrow).or_default();
                    e.0 += 1;
                    e.1 += 1;
                }
            }
            BlockToken::TodoOpen(_) | BlockToken::TodoPending(_) => {
                for &(hrow, _) in &heading_stack {
                    result.entry(hrow).or_default().1 += 1;
                }
            }
            _ => {}
        }
    }
    result
}

fn conceal_todo(text: &str) -> String {
    text.replacen("- (", "• (", 1).replacen("(x)", "(✓)", 1)
}

pub(crate) fn char_to_display_col(text: &str, char_col: usize, tab_width: usize) -> usize {
    let mut display = 0;
    for (i, c) in text.chars().enumerate() {
        if i == char_col {
            break;
        }
        display += char_display_width(c, display, tab_width);
    }
    display
}

/// Display columns occupied by leading whitespace in `text`.
pub(crate) fn leading_indent_width(text: &str, tab_width: usize) -> usize {
    let mut col = 0;
    for c in text.chars() {
        match c {
            ' ' => col += 1,
            '\t' => col += tab_width - (col % tab_width),
            _ => break,
        }
    }
    col
}

/// Char index of the character whose display column is `target`, clamped to EOL.
pub(crate) fn display_col_to_char_col(text: &str, target: usize, tab_width: usize) -> usize {
    let mut col = 0;
    for (i, c) in text.chars().enumerate() {
        if col >= target {
            return i;
        }
        col += char_display_width(c, col, tab_width);
    }
    text.chars().count()
}

fn char_display_width(c: char, current_display_col: usize, tab_width: usize) -> usize {
    if c == '\t' {
        tab_width - (current_display_col % tab_width)
    } else {
        c.width().unwrap_or(1)
    }
}

fn scroll_to(prev: usize, cursor: usize, size: usize) -> usize {
    if cursor < prev {
        cursor
    } else if prev + size <= cursor {
        cursor + 1 - size
    } else {
        prev
    }
}

fn num_digits(n: usize) -> usize {
    let mut count = 0;
    let mut n = n.max(1);
    while n > 0 {
        count += 1;
        n /= 10;
    }
    count
}

fn build_title(app: &App) -> String {
    let name = app
        .file_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "[no file]".to_string());
    let dirty = if app.modified { " ●" } else { "" };
    format!(" {name}{dirty} ")
}
