use serde::Deserialize;
use std::path::PathBuf;

// ── Top-level config ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Absolute or `~/`-prefixed path to the notes directory.
    pub workspace_root: Option<String>,
    pub editor: EditorConfig,
    pub ui: UiConfig,
    pub journal: JournalConfig,
    pub pdf: PdfConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            workspace_root: None,
            editor: EditorConfig::default(),
            ui: UiConfig::default(),
            journal: JournalConfig::default(),
            pdf: PdfConfig::default(),
        }
    }
}

impl Config {
    /// Load from `~/.config/nota/config.toml`, falling back to defaults silently
    /// if the file does not exist. Prints a warning on parse error.
    pub fn load() -> Self {
        let Some(path) = config_path() else { return Self::default() };
        let Ok(text) = std::fs::read_to_string(&path) else { return Self::default() };
        match toml::from_str(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("norgx: config error in {}: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn workspace_root_path(&self) -> Option<PathBuf> {
        self.workspace_root.as_deref().map(expand_tilde)
    }
}

// ── Editor ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    pub line_numbers: bool,
    pub relative_line_numbers: bool,
    pub tab_width: u8,
    pub conceal_headings: bool,
    pub conceal_todos: bool,
    pub conceal_links: bool,
    pub line_wrap: bool,
    pub show_todo_progress: bool,
    /// Auto-save interval in seconds. None disables auto-save.
    pub auto_save_secs: Option<u64>,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            line_numbers: true,
            relative_line_numbers: false,
            tab_width: 4,
            conceal_headings: true,
            conceal_todos: true,
            conceal_links: true,
            line_wrap: false,
            show_todo_progress: true,
            auto_save_secs: None,
        }
    }
}

// ── UI ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Hex color string, e.g. `"#61AFEF"`. Used for borders, pill, cursor.
    pub accent_color: String,
    /// Width of the file-tree sidebar in terminal columns.
    pub file_tree_width: u16,
    /// Six hex colors, one per heading level 1–6.
    pub heading_colors: Vec<String>,
    /// Six single-character strings, one glyph per heading level 1–6.
    pub heading_glyphs: Vec<String>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            accent_color: "#61AFEF".into(),
            file_tree_width: 30,
            heading_colors: vec![
                "#61AFEF".into(), // H1 — blue
                "#98C379".into(), // H2 — green
                "#E5C07B".into(), // H3 — gold
                "#E06C75".into(), // H4 — red
                "#56B6C2".into(), // H5 — cyan
                "#C678DD".into(), // H6 — purple
            ],
            heading_glyphs: vec![
                "◉".into(), "◎".into(), "○".into(),
                "✺".into(), "▶".into(), "↳".into(),
            ],
        }
    }
}

impl UiConfig {
    pub fn accent_rgb(&self) -> (u8, u8, u8) {
        parse_hex_color(&self.accent_color).unwrap_or((97, 175, 239))
    }

    pub fn heading_color_rgb(&self, level: u8) -> (u8, u8, u8) {
        let idx = (level as usize).saturating_sub(1).min(5);
        self.heading_colors
            .get(idx)
            .and_then(|s| parse_hex_color(s))
            .unwrap_or_else(|| default_heading_rgb(level))
    }

    pub fn heading_glyph_char(&self, level: u8) -> char {
        let idx = (level as usize).saturating_sub(1).min(5);
        self.heading_glyphs
            .get(idx)
            .and_then(|s| s.chars().next())
            .unwrap_or_else(|| default_heading_glyph(level))
    }
}

// ── Journal ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct JournalConfig {
    /// Override the journal directory. Defaults to `{workspace_root}/journal/`.
    pub dir: Option<String>,
    /// strftime-style filename format, e.g. `"%Y-%m-%d"` → `2026-04-27.norg`.
    pub date_format: String,
    /// Template inserted into new journal entries. `{date}` is replaced with
    /// the formatted date. Defaults to `"* {date}\n\n"`.
    pub template: Option<String>,
}

impl Default for JournalConfig {
    fn default() -> Self {
        Self {
            dir: None,
            date_format: "%Y-%m-%d".into(),
            template: None,
        }
    }
}

impl JournalConfig {
    pub fn dir_path(&self, workspace_root: &PathBuf) -> PathBuf {
        self.dir
            .as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| workspace_root.join("journal"))
    }
}

// ── PDF export ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PdfConfig {
    /// Command used to open the generated PDF (default: `xdg-open`).
    pub viewer: String,
    /// Font name passed to xelatex.
    pub font: String,
    /// Body font size in pt.
    pub font_size: f32,
    /// Page margin string understood by LaTeX geometry, e.g. `"1in"`.
    pub margin: String,
}

impl Default for PdfConfig {
    fn default() -> Self {
        Self {
            viewer: "xdg-open".into(),
            font: "JetBrainsMono Nerd Font".into(),
            font_size: 10.5,
            margin: "1in".into(),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("norgx").join("config.toml"))
}

/// Expand a leading `~/` to the user's home directory.
pub fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(s)
}

/// Parse a `#RRGGBB` hex color string into `(r, g, b)`.
pub fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

fn default_heading_rgb(level: u8) -> (u8, u8, u8) {
    match level {
        1 => (97, 175, 239),
        2 => (152, 195, 121),
        3 => (229, 192, 123),
        4 => (224, 108, 117),
        5 => (86, 182, 194),
        _ => (198, 120, 221),
    }
}

fn default_heading_glyph(level: u8) -> char {
    match level {
        1 => '◉',
        2 => '◎',
        3 => '○',
        4 => '✺',
        5 => '▶',
        _ => '↳',
    }
}
