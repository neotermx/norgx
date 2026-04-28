use anyhow::{Context, Result};
use std::fs;

pub fn run() -> Result<()> {
    let home = dirs::home_dir().context("could not determine home directory")?;

    let notes_dir  = home.join("notes");
    let config_dir = dirs::config_dir()
        .context("could not determine config directory")?
        .join("norgx");
    let config_file  = config_dir.join("config.toml");
    let welcome_file = notes_dir.join("welcome.norg");

    println!();
    println!("  norgx setup");
    println!("  ───────────");

    // ── ~/notes/ ─────────────────────────────────────────────────────────────

    if notes_dir.exists() {
        println!("  [skip]    {} (already exists)", notes_dir.display());
    } else {
        fs::create_dir_all(&notes_dir)
            .with_context(|| format!("could not create {}", notes_dir.display()))?;
        println!("  [created] {}", notes_dir.display());
    }

    // ── welcome.norg ─────────────────────────────────────────────────────────

    if welcome_file.exists() {
        println!("  [skip]    {} (already exists)", welcome_file.display());
    } else {
        fs::write(&welcome_file, WELCOME_NORG)
            .with_context(|| format!("could not write {}", welcome_file.display()))?;
        println!("  [created] {}", welcome_file.display());
    }

    // ── ~/.config/norgx/ ─────────────────────────────────────────────────────

    fs::create_dir_all(&config_dir)
        .with_context(|| format!("could not create {}", config_dir.display()))?;

    if config_file.exists() {
        println!("  [skip]    {} (already exists)", config_file.display());
    } else {
        fs::write(&config_file, CONFIG_TOML)
            .with_context(|| format!("could not write {}", config_file.display()))?;
        println!("  [created] {}", config_file.display());
    }

    println!();
    println!("  All done. Run `norgx` to get started.");
    println!();

    Ok(())
}

// ── Config template ───────────────────────────────────────────────────────────

const CONFIG_TOML: &str = r##"# norgx configuration
# All values are optional — defaults apply when a key is absent.
# Location: ~/.config/norgx/config.toml

# Workspace root shown in the file tree.
# Defaults to ~/notes if it exists, otherwise the current directory.
# workspace_root = "~/notes"

[editor]
# Show line numbers in the editor gutter.
line_numbers = true

# Show line numbers relative to the cursor line (like Vim's relativenumber).
# The cursor line always shows its absolute number.
# relative_line_numbers = false

# Width of a tab character in display columns.
tab_width = 4

# Wrap long lines at the editor boundary instead of scrolling horizontally.
# line_wrap = false

# Replace heading markers (* ** ***) with Unicode glyphs on non-cursor lines.
conceal_headings = true

# Replace todo markers with cleaner symbols on non-cursor lines.
conceal_todos = true

# Replace link markup with just the label on non-cursor lines.
conceal_links = true

# Show todo progress counts on heading lines, e.g. "* My Heading (1/3)".
show_todo_progress = true

# Auto-save interval in seconds. Remove or comment out to disable.
# auto_save_secs = 30

[ui]
# Accent color for borders, cursor, and the status pill (hex).
accent_color = "#61AFEF"

# Width of the file-tree sidebar in terminal columns.
file_tree_width = 30

# Colors for heading levels 1–6 (hex, one per level).
heading_colors = [
  "#61AFEF",  # H1 — blue
  "#98C379",  # H2 — green
  "#E5C07B",  # H3 — gold
  "#E06C75",  # H4 — red
  "#56B6C2",  # H5 — cyan
  "#C678DD",  # H6 — purple
]

# Glyphs displayed before headings when conceal_headings is true.
# Must be exactly 6 single-character entries (one per level).
heading_glyphs = ["◉", "◎", "○", "✺", "▶", "↳"]

[journal]
# Journal directory. Defaults to {workspace_root}/journal/.
# dir = "~/notes/journal"

# Filename date format (strftime syntax).
date_format = "%Y-%m-%d"

# Template inserted into new journal entries.
# {date} is replaced with the formatted date string.
# template = "* {date}\n\n"

[pdf]
# Command used to open the generated PDF after export.
viewer = "xdg-open"

# Font family for the PDF (must be installed; passed to fontspec/xelatex).
font = "JetBrainsMono Nerd Font"

# Body font size in points.
font_size = 10.5

# Page margins in LaTeX geometry format.
margin = "1in"
"##;

// ── Welcome file ──────────────────────────────────────────────────────────────

const WELCOME_NORG: &str = r##"* Welcome to norgx

  norgx is a terminal editor for the Norg file format — think Obsidian, but
  for Norg in the terminal.

  This file lives at ~/notes/welcome.norg. Feel free to edit or delete it.

* The Basics

** Headings

   Use `*` for a top-level heading, `**` for a sub-heading, and so on up to
   level 6. Press `]h` / `[h` to jump between headings.

** Todo Items

   - ( ) This task is open
   - (x) This task is done
   - (-) This task is in progress

   Press `Ctrl+T` on any todo line to cycle through the three states.

** Links

   Link to another file like this: {:notes/other-file:}[label text]

   With the cursor on a link, press `gf` to follow it and `gb` to go back.

** Folding

   Press `za` on a heading line to fold or unfold its contents.

* Keybindings

  Press `Ctrl+/` at any time for the full keybindings reference.

  A few essentials:

  | Key          | Action               |
  | Ctrl+S       | Save                 |
  | Ctrl+O       | Open file            |
  | Ctrl+N       | New scratch buffer   |
  | Ctrl+J       | Open today's journal |
  | Ctrl+P       | Export to PDF        |
  | Ctrl+B       | Focus the file tree  |
  | Ctrl+\       | Toggle file tree     |
  | Ctrl+Q       | Quit                 |

* Configuration

  Edit `~/.config/norgx/config.toml` to customize colors, fonts, the workspace
  root, auto-save interval, and more. Every option is documented with its
  default value in that file.
"##;
