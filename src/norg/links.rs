use std::path::{Path, PathBuf};

/// Return the path string inside the first `{:...:}` span whose character
/// range contains `col`.  Returns `None` if the cursor is not on any link.
pub fn find_link_at_col(line: &str, col: usize) -> Option<String> {
    let mut search_from = 0usize;
    while let Some(open_off) = line[search_from..].find("{:") {
        let open_byte = search_from + open_off;
        let inner_start = open_byte + 2;
        if let Some(close_off) = line[inner_start..].find(":}") {
            let close_byte = inner_start + close_off;
            let span_end_byte = close_byte + 2;
            let span_start_char = line[..open_byte].chars().count();
            let span_end_char = span_start_char + line[open_byte..span_end_byte].chars().count();
            if col >= span_start_char && col < span_end_char {
                return Some(line[inner_start..close_byte].to_string());
            }
            search_from = span_end_byte;
        } else {
            break; // unclosed link — stop
        }
    }
    None
}

/// Resolve a Norg link path to a `PathBuf`, adding `.norg` if needed.
///
/// Resolution rules (matches standard Neorg behaviour):
/// 1. Tilde-expand; if absolute, use as-is.
/// 2. `$/...` — explicit workspace-root prefix (strip `$/`, treat as bare).
/// 3. `./` or `../` prefix — file-relative only.
/// 4. Bare path — try workspace root first, then current file's directory.
///
/// Returns the resolved path even if it does not exist yet (caller creates it).
pub fn resolve_link_path(link: &str, current_file: Option<&Path>, workspace_root: &Path) -> PathBuf {
    // Strip Neorg's explicit workspace prefix `$/`.
    let link = link.strip_prefix("$/").unwrap_or(link);

    let base = crate::config::expand_tilde(link);

    if base.is_absolute() {
        return with_ext_if_missing(&base);
    }

    // Explicit file-relative prefix — only look next to current file.
    if link.starts_with("./") || link.starts_with("../") {
        if let Some(dir) = current_file.and_then(|f| f.parent()) {
            return with_ext_if_missing(&dir.join(&base));
        }
        return with_ext_if_missing(&base);
    }

    // Bare path: workspace root first (standard Neorg), then current file's dir.
    let from_root = with_ext_if_missing(&workspace_root.join(&base));
    if from_root.exists() {
        return from_root;
    }

    if let Some(dir) = current_file.and_then(|f| f.parent()) {
        let from_file = with_ext_if_missing(&dir.join(&base));
        if from_file.exists() {
            return from_file;
        }
    }

    from_root
}

/// If `path` has no extension and no existing file matches, return
/// `path.with_extension("norg")`.  Otherwise return `path` unchanged.
fn with_ext_if_missing(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    let with_ext = path.with_extension("norg");
    if with_ext.exists() {
        return with_ext;
    }
    if path.extension().is_none() {
        with_ext
    } else {
        path.to_path_buf()
    }
}
