use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const IGNORED: &[&str] = &["target", "node_modules", ".git", ".svn", ".hg", ".DS_Store"];

const SHOWN_EXTENSIONS: &[&str] = &[
    "norg", "txt",
    "toml", "yaml", "yml", "json",
    "md", "ini", "conf", "sh",
];

#[derive(Clone, Debug)]
pub struct TreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
}

pub struct FileTree {
    pub root: PathBuf,
    /// Flat, pre-filtered list of visible entries in display order.
    /// Rebuilt by `refresh()` whenever expand state changes.
    entries: Vec<TreeEntry>,
    expanded: HashMap<PathBuf, bool>,
    pub selected: usize,

    /// All `.norg` files under root, depth-first, regardless of expand state.
    /// Used as the source list for fuzzy search.
    all_files: Vec<TreeEntry>,

    /// Pinned recent files shown above the tree. Indices 0..recent.len()-1 in
    /// `selected` address this list; indices recent.len().. address `entries`.
    pub recent: Vec<PathBuf>,

    pub fuzzy_active: bool,
    pub fuzzy_query: String,
    pub fuzzy_selected: usize,
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        let mut expanded = HashMap::new();
        expanded.insert(root.clone(), true);
        let mut tree = Self {
            root,
            entries: vec![],
            expanded,
            selected: 0,
            all_files: vec![],
            recent: vec![],
            fuzzy_active: false,
            fuzzy_query: String::new(),
            fuzzy_selected: 0,
        };
        tree.refresh();
        tree
    }

    /// Rebuild both the visible entry list and the flat all-files list.
    pub fn refresh(&mut self) {
        self.entries.clear();
        self.scan(&self.root.clone(), 0);
        self.all_files = collect_all_norg_files(&self.root.clone());
        // Clamp selected to valid range.
        let total = self.total_count();
        if total == 0 {
            self.selected = 0;
        } else if self.selected >= total {
            self.selected = total - 1;
        }
        // Clamp fuzzy_selected.
        let count = self.fuzzy_matches_count();
        if self.fuzzy_selected >= count && count > 0 {
            self.fuzzy_selected = count - 1;
        }
    }

    pub fn total_count(&self) -> usize {
        self.recent.len() + self.entries.len()
    }

    /// If the current selection is in the recent section, return that path.
    pub fn recent_section_selected(&self) -> Option<&PathBuf> {
        if self.selected < self.recent.len() {
            self.recent.get(self.selected)
        } else {
            None
        }
    }

    fn scan(&mut self, dir: &Path, depth: usize) {
        let Ok(read_dir) = fs::read_dir(dir) else { return };

        let mut children: Vec<(PathBuf, String, bool)> = read_dir
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    return None;
                }
                let path = e.path();
                let is_dir = path.is_dir();
                if !is_dir {
                    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                    if !SHOWN_EXTENSIONS.contains(&ext) {
                        return None;
                    }
                } else if IGNORED.contains(&name.as_str()) {
                    return None;
                }
                Some((path, name, is_dir))
            })
            .collect();

        children.sort_by(|(_, a, a_dir), (_, b, b_dir)| {
            b_dir.cmp(a_dir).then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
        });

        for (path, name, is_dir) in children {
            let is_expanded = is_dir && self.expanded.get(&path).copied().unwrap_or(false);
            self.entries.push(TreeEntry { path: path.clone(), name, is_dir, depth });
            if is_expanded {
                self.scan(&path, depth + 1);
            }
        }
    }

    // ── Normal tree navigation ────────────────────────────────────────────────

    pub fn entries(&self) -> &[TreeEntry] {
        &self.entries
    }

    /// Returns the tree entry at the current selection, or None if selection
    /// is in the recent section.
    pub fn selected_entry(&self) -> Option<&TreeEntry> {
        let idx = self.selected.checked_sub(self.recent.len())?;
        self.entries.get(idx)
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.get(path).copied().unwrap_or(false)
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.total_count() {
            self.selected += 1;
        }
    }

    pub fn toggle_dir(&mut self) {
        let Some(entry) = self.selected_entry() else { return };
        if !entry.is_dir { return; }
        let path = entry.path.clone();
        if self.expanded.get(&path).copied().unwrap_or(false) {
            self.expanded.remove(&path);
        } else {
            self.expanded.insert(path.clone(), true);
        }
        self.refresh();
        if let Some(idx) = self.entries.iter().position(|e| e.path == path) {
            self.selected = self.recent.len() + idx;
        } else {
            self.selected = self.selected.min(self.total_count().saturating_sub(1));
        }
    }

    /// Expand all ancestor directories of `path` so it is visible in the tree,
    /// then move the selection to it.
    pub fn reveal(&mut self, path: &Path) {
        // Walk from root toward path, expanding each intermediate directory.
        let mut ancestor = self.root.clone();
        if let Ok(rel) = path.strip_prefix(&self.root) {
            for component in rel.components() {
                ancestor = ancestor.join(component);
                if ancestor.is_dir() {
                    self.expanded.insert(ancestor.clone(), true);
                }
            }
        }
        self.refresh();

        // Move selection to the revealed file.
        if let Some(idx) = self.entries.iter().position(|e| e.path == path) {
            self.selected = self.recent.len() + idx;
        }
    }

    // ── Fuzzy search ──────────────────────────────────────────────────────────

    pub fn enter_fuzzy(&mut self) {
        self.fuzzy_active = true;
        self.fuzzy_query.clear();
        self.fuzzy_selected = 0;
    }

    pub fn exit_fuzzy(&mut self) {
        self.fuzzy_active = false;
        self.fuzzy_query.clear();
        self.fuzzy_selected = 0;
    }

    pub fn fuzzy_push(&mut self, c: char) {
        self.fuzzy_query.push(c);
        self.fuzzy_selected = 0;
    }

    pub fn fuzzy_pop(&mut self) {
        self.fuzzy_query.pop();
        self.fuzzy_selected = 0;
    }

    pub fn fuzzy_move_up(&mut self) {
        self.fuzzy_selected = self.fuzzy_selected.saturating_sub(1);
    }

    pub fn fuzzy_move_down(&mut self) {
        let max = self.fuzzy_matches_count().saturating_sub(1);
        self.fuzzy_selected = (self.fuzzy_selected + 1).min(max);
    }

    /// Returns all files whose relative path matches the current query.
    pub fn fuzzy_matches(&self) -> Vec<&TreeEntry> {
        self.all_files
            .iter()
            .filter(|e| {
                let display = relative_path(&self.root, &e.path);
                fuzzy_match(&self.fuzzy_query, &display)
            })
            .collect()
    }

    pub fn fuzzy_selected_entry(&self) -> Option<&TreeEntry> {
        self.fuzzy_matches().into_iter().nth(self.fuzzy_selected)
    }

    fn fuzzy_matches_count(&self) -> usize {
        self.fuzzy_matches().len()
    }
}

// ── Standalone helpers ────────────────────────────────────────────────────────

/// Walk `root` recursively and collect all files with a shown extension.
fn collect_all_norg_files(root: &Path) -> Vec<TreeEntry> {
    let mut result = Vec::new();
    collect_recursive(root, &mut result);
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

fn collect_recursive(dir: &Path, result: &mut Vec<TreeEntry>) {
    let Ok(read_dir) = fs::read_dir(dir) else { return };

    let mut children: Vec<(PathBuf, String, bool)> = read_dir
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path();
            let is_dir = path.is_dir();
            if is_dir {
                if IGNORED.contains(&name.as_str()) {
                    return None;
                }
            } else {
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                if !SHOWN_EXTENSIONS.contains(&ext) {
                    return None;
                }
            }
            Some((path, name, is_dir))
        })
        .collect();

    children.sort_by(|(_, a, _), (_, b, _)| a.to_lowercase().cmp(&b.to_lowercase()));

    for (path, name, is_dir) in children {
        if is_dir {
            collect_recursive(&path, result);
        } else {
            result.push(TreeEntry { path, name, is_dir: false, depth: 0 });
        }
    }
}

/// Strip `root` from `path` and return the relative string, or just the
/// filename if stripping fails.
fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .and_then(|p| p.to_str())
        .unwrap_or_else(|| path.file_name().and_then(|n| n.to_str()).unwrap_or(""))
        .to_string()
}

/// Fuzzy subsequence match — all chars of `query` must appear in order in
/// `target` (case-insensitive).
pub fn fuzzy_match(query: &str, target: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut ti = target.chars().flat_map(|c| c.to_lowercase());
    'outer: for qc in query.chars().flat_map(|c| c.to_lowercase()) {
        loop {
            match ti.next() {
                Some(tc) if tc == qc => continue 'outer,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}
