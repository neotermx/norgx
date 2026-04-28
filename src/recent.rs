use std::collections::VecDeque;
use std::path::PathBuf;

const MAX_RECENT: usize = 5;

pub struct RecentFiles {
    pub files: VecDeque<PathBuf>,
}

impl RecentFiles {
    pub fn load() -> Self {
        Self { files: load_from_disk() }
    }

    /// Add `path` to the front, deduplicating and capping at MAX_RECENT.
    pub fn push(&mut self, path: &PathBuf) {
        let resolved = path
            .canonicalize()
            .unwrap_or_else(|_| {
                if path.is_absolute() {
                    path.clone()
                } else {
                    std::env::current_dir()
                        .map(|cwd| cwd.join(path))
                        .unwrap_or_else(|_| path.clone())
                }
            });
        self.files.retain(|p| p != &resolved);
        self.files.push_front(resolved);
        while self.files.len() > MAX_RECENT {
            self.files.pop_back();
        }
        let _ = self.save();
    }

    fn save(&self) -> Option<()> {
        let path = recent_path()?;
        std::fs::create_dir_all(path.parent()?).ok()?;
        let content: String = self.files
            .iter()
            .filter_map(|p| p.to_str())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, content).ok()
    }
}

fn load_from_disk() -> VecDeque<PathBuf> {
    let Some(path) = recent_path() else { return VecDeque::new() };
    let Ok(text) = std::fs::read_to_string(path) else { return VecDeque::new() };
    text.lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .take(MAX_RECENT)
        .collect()
}

fn recent_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("norgx").join("recent"))
}
