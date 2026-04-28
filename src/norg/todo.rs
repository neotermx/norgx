/// Cycle the TODO marker on a line: `( )` → `(x)` → `(-)` → `( )`.
/// Returns `Some(new_line)` if the line was a TODO line, `None` otherwise.
/// Handles both `"- ( ) text"` and `"- ( )"` (no trailing text).
pub fn cycle_todo(line: &str) -> Option<String> {
    const PAIRS: &[(&str, &str)] = &[
        ("- ( )", "- (x)"),
        ("- (x)", "- (-)"),
        ("- (-)", "- ( )"),
    ];
    for (from, to) in PAIRS {
        if line == *from {
            return Some(to.to_string());
        }
        if let Some(rest) = line.strip_prefix(&format!("{from} ")) {
            return Some(format!("{to} {rest}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_open_to_done() {
        assert_eq!(cycle_todo("- ( ) buy milk"), Some("- (x) buy milk".into()));
    }

    #[test]
    fn cycle_done_to_pending() {
        assert_eq!(cycle_todo("- (x) buy milk"), Some("- (-) buy milk".into()));
    }

    #[test]
    fn cycle_pending_to_open() {
        assert_eq!(cycle_todo("- (-) buy milk"), Some("- ( ) buy milk".into()));
    }

    #[test]
    fn cycle_without_trailing_text() {
        assert_eq!(cycle_todo("- ( )"), Some("- (x)".into()));
        assert_eq!(cycle_todo("- (x)"), Some("- (-)".into()));
        assert_eq!(cycle_todo("- (-)"), Some("- ( )".into()));
    }

    #[test]
    fn non_todo_line() {
        assert_eq!(cycle_todo("* Heading"), None);
        assert_eq!(cycle_todo("just text"), None);
    }
}
