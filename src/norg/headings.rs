use super::parser::{parse_line, BlockToken};

/// Returns the heading level (1–6) of `line`, or 0 if it is not a heading.
pub fn heading_level_of(line: &str) -> u8 {
    match parse_line(line) {
        BlockToken::Heading { level, .. } => level,
        _ => 0,
    }
}

/// Returns the range of body-line indices owned by the heading at `heading_row`.
/// The heading line itself is not included. Empty range when there is no body.
pub fn fold_body_range(lines: &[&str], heading_row: usize) -> std::ops::Range<usize> {
    let level = heading_level_of(lines.get(heading_row).copied().unwrap_or(""));
    if level == 0 {
        return heading_row..heading_row;
    }
    let start = heading_row + 1;
    if start >= lines.len() {
        return start..start;
    }
    let body_len = lines[start..]
        .iter()
        .position(|l| {
            let lvl = heading_level_of(l);
            lvl > 0 && lvl <= level
        })
        .unwrap_or(lines.len() - start);
    start..start + body_len
}

/// Returns the line index of the next heading after `current_row`, wrapping around.
/// Returns `None` if there are no headings in the buffer.
pub fn next_heading(lines: &[&str], current_row: usize) -> Option<usize> {
    let len = lines.len();
    for offset in 1..=len {
        let idx = (current_row + offset) % len;
        if matches!(parse_line(lines[idx]), BlockToken::Heading { .. }) {
            return Some(idx);
        }
    }
    None
}

/// Returns the line index of the previous heading before `current_row`, wrapping around.
/// Returns `None` if there are no headings in the buffer.
pub fn prev_heading(lines: &[&str], current_row: usize) -> Option<usize> {
    let len = lines.len();
    for offset in 1..=len {
        let idx = (current_row + len - offset) % len;
        if matches!(parse_line(lines[idx]), BlockToken::Heading { .. }) {
            return Some(idx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: &[&str] = &[
        "* First",
        "some text",
        "** Second",
        "more text",
        "* Third",
    ];

    #[test]
    fn next_from_start() {
        assert_eq!(next_heading(LINES, 0), Some(2));
    }

    #[test]
    fn next_wraps_around() {
        assert_eq!(next_heading(LINES, 4), Some(0));
    }

    #[test]
    fn prev_from_end() {
        assert_eq!(prev_heading(LINES, 4), Some(2));
    }

    #[test]
    fn prev_wraps_around() {
        assert_eq!(prev_heading(LINES, 0), Some(4));
    }

    #[test]
    fn no_headings() {
        assert_eq!(next_heading(&["plain", "text"], 0), None);
    }
}
