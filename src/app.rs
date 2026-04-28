use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal};
use std::{
    collections::HashSet,
    fs, io,
    path::PathBuf,
    time::{Duration, Instant},
};
use tui_textarea::{CursorMove, TextArea};

use chrono::Local;

use crate::config::Config;
use crate::export::latex::export_to_pdf;
use crate::input::{AppAction, Focus, InputState};
use crate::ui::editor::{
    build_display_map, char_to_display_col, display_col_to_char_col,
    expand_wrapped_lines, leading_indent_width, DisplayRow,
};
use crate::norg::{
    cycle_todo, find_link_at_col, fold_body_range, heading_level_of,
    next_heading, prev_heading, resolve_link_path,
};
use crate::recent::RecentFiles;
use crate::ui;
use crate::workspace::FileTree;

/// Floating dialog that overlays the editor and consumes all key input until dismissed.
pub enum Modal {
    SaveBeforeQuit,
    SaveAs { input: String, cursor: usize },
    OpenFile { input: String, cursor: usize, selected: bool },
    NewFile { input: String, cursor: usize },
    PdfPath { input: String, bw: bool },
    Keybinds,
    RenameFile { path: PathBuf, input: String, cursor: usize },
    DeleteConfirm(PathBuf),
    SaveBeforeNew,
    Error(String),
}

pub struct App<'a> {
    pub textarea: TextArea<'a>,
    pub file_path: Option<PathBuf>,
    pub modified: bool,
    pub status_msg: String,
    pub should_quit: bool,
    pub row_offset: usize,
    pub col_offset: usize,
    pub file_tree: FileTree,
    pub config: Config,
    // In-file search state.
    pub search_active: bool,
    pub search_query: String,
    pub search_matches: Vec<(usize, usize)>, // (row, char_col) of each match start
    pub search_current: usize,
    // Marquee scroll state for long file-tree entries.
    pub marquee_offset: u64,
    marquee_last_tick: Instant,
    marquee_last_key: (usize, bool), // (selected_idx, fuzzy_active)
    input: InputState,
    recent: RecentFiles,
    /// Show the splash screen (shown on launch when no file argument is given).
    pub show_splash: bool,
    /// Active dialog overlay; consumes all key input while Some.
    pub modal: Option<Modal>,
    /// Last path used for PDF export (pre-fills the dialog next time).
    pub last_pdf_path: Option<String>,
    /// When true, a successful save_to_path() will set should_quit.
    quit_after_save: bool,
    /// Row indices of heading lines whose bodies are currently folded.
    pub folded_headings: HashSet<usize>,
    /// Receives the PDF path (or error string) when a background export finishes.
    pdf_rx: Option<std::sync::mpsc::Receiver<Result<PathBuf, String>>>,
    /// Whether the file tree panel is currently shown.
    pub file_tree_visible: bool,
    /// True when the current buffer is a .norg file (or unsaved scratch).
    pub is_norg: bool,
    /// Stack of (path, row, col) for `gb` go-back navigation after `gf`.
    file_history: Vec<(PathBuf, usize, usize)>,
    /// Tracks elapsed time for auto-save.
    last_auto_save: Instant,
    /// Completions generated for the current Tab cycle; None = not cycling.
    tab_completions: Option<Vec<String>>,
    tab_idx: usize,
    /// Rendered areas, updated each frame so mouse handlers can map coordinates.
    pub editor_area: Rect,
    pub tree_area: Rect,
}

impl<'a> App<'a> {
    pub fn new(file_path: Option<String>) -> Result<Self> {
        let config = Config::load();
        let mut recent = RecentFiles::load();

        let no_file = file_path.is_none();
        let (textarea, path) = match file_path {
            Some(p) => {
                let path = PathBuf::from(&p);
                let content = if path.exists() {
                    fs::read_to_string(&path)?
                } else {
                    String::new()
                };
                let lines: Vec<String> = content.lines().map(String::from).collect();
                recent.push(&path);
                (TextArea::new(lines), Some(path))
            }
            None => (TextArea::default(), None),
        };

        let workspace_root = workspace_root(path.as_deref(), config.workspace_root_path().as_deref());
        let mut file_tree = FileTree::new(workspace_root);
        file_tree.recent = recent.files.iter().cloned().collect();
        if let Some(p) = path.as_deref() {
            if let Ok(abs) = p.canonicalize() {
                file_tree.reveal(&abs);
            }
        }

        let is_norg = path.as_deref().map(is_norg_path).unwrap_or(true);

        Ok(Self {
            textarea,
            file_path: path,
            modified: false,
            status_msg: String::from("Ctrl+S save  Ctrl+Q quit  Ctrl+B files"),
            should_quit: false,
            row_offset: 0,
            col_offset: 0,
            file_tree,
            config,
            search_active: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current: 0,
            marquee_offset: 0,
            marquee_last_tick: Instant::now(),
            marquee_last_key: (0, false),
            input: InputState::default(),
            recent,
            show_splash: no_file,
            modal: None,
            last_pdf_path: None,
            quit_after_save: false,
            folded_headings: HashSet::new(),
            pdf_rx: None,
            file_tree_visible: true,
            is_norg,
            file_history: Vec::new(),
            last_auto_save: Instant::now(),
            tab_completions: None,
            tab_idx: 0,
            editor_area: Rect::default(),
            tree_area: Rect::default(),
        })
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            terminal.draw(|f| ui::draw(f, self))?;

            if event::poll(Duration::from_millis(100))? {
              match event::read()? {
                Event::Mouse(mouse) => { self.handle_mouse(mouse); }
                Event::Key(key) => {
                // Modal always has highest priority — swallows all input while open.
                if self.modal.is_some() {
                    self.handle_modal_key(key);
                    if self.should_quit { break; }
                    continue;
                }

                if self.show_splash {
                    match (key.modifiers, key.code) {
                        (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
                            self.should_quit = true;
                        }
                        (KeyModifiers::NONE, KeyCode::Char('n')) => {
                            self.show_splash = false;
                            self.modal = Some(Modal::NewFile { input: String::new(), cursor: 0 });
                        }
                        (KeyModifiers::NONE, KeyCode::Char('o')) => {
                            self.show_splash = false;
                            let (input, selected) = self.recent_open_default();
                            let cursor = input.chars().count();
                            self.modal = Some(Modal::OpenFile { input, cursor, selected });
                        }
                        (KeyModifiers::NONE, KeyCode::Char('c')) => {
                            self.show_splash = false;
                            if let Some(p) = dirs::config_dir() {
                                self.open_file(p.join("norgx").join("config.toml"));
                            }
                        }
                        _ => {}
                    }
                    if self.should_quit { break; }
                    continue;
                }


                if self.file_tree.fuzzy_active {
                    match (key.modifiers, key.code) {
                        (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
                            self.apply(AppAction::Quit);
                        }
                        (KeyModifiers::NONE, KeyCode::Esc) => {
                            self.file_tree.exit_fuzzy();
                        }
                        (KeyModifiers::NONE, KeyCode::Enter) => {
                            self.fuzzy_select();
                        }
                        (KeyModifiers::NONE, KeyCode::Up)
                        | (KeyModifiers::NONE, KeyCode::Char('k')) => {
                            self.file_tree.fuzzy_move_up();
                        }
                        (KeyModifiers::NONE, KeyCode::Down)
                        | (KeyModifiers::NONE, KeyCode::Char('j')) => {
                            self.file_tree.fuzzy_move_down();
                        }
                        (KeyModifiers::NONE, KeyCode::Backspace) => {
                            self.file_tree.fuzzy_pop();
                        }
                        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                            self.file_tree.fuzzy_push(c);
                        }
                        _ => {}
                    }
                    if self.should_quit { break; }
                    continue;
                }

                if self.search_active {
                    match (key.modifiers, key.code) {
                        (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
                            self.apply(AppAction::Quit);
                        }
                        (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
                            self.save();
                        }
                        // Ctrl+F while searching → next match
                        (KeyModifiers::CONTROL, KeyCode::Char('f')) |
                        (KeyModifiers::NONE, KeyCode::Down) => {
                            self.search_next();
                        }
                        (KeyModifiers::NONE, KeyCode::Up) => {
                            self.search_prev();
                        }
                        (KeyModifiers::NONE, KeyCode::Enter) |
                        (KeyModifiers::NONE, KeyCode::Esc) => {
                            self.search_active = false;
                        }
                        (KeyModifiers::NONE, KeyCode::Backspace) => {
                            self.search_query.pop();
                            self.recompute_search();
                            self.search_find_from_cursor();
                        }
                        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                            self.search_query.push(c);
                            self.recompute_search();
                            self.search_find_from_cursor();
                        }
                        _ => {}
                    }
                    if self.should_quit { break; }
                    continue;
                }

                let actions = self.input.handle(key);
                for action in actions {
                    self.apply(action);
                }
                } // Event::Key
                _ => {}
              } // match event::read()
            }

            self.tick_marquee();
            self.poll_pdf_export();
            self.tick_auto_save();

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    // ── Modal input handling ──────────────────────────────────────────────────

    fn handle_modal_key(&mut self, key: KeyEvent) {
        // Any key other than Tab resets the tab-completion cycle.
        if key.code != KeyCode::Tab {
            self.tab_completions = None;
            self.tab_idx = 0;
        }

        let Some(modal) = self.modal.take() else { return };

        match modal {
            Modal::SaveBeforeQuit => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.quit_after_save = true;
                    self.save();
                    // save() calls save_to_path() which sets should_quit when
                    // quit_after_save is true. If there was no path, save() opened
                    // a SaveAs modal instead — should_quit will be set after that.
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.should_quit = true;
                }
                _ => {
                    self.modal = Some(Modal::SaveBeforeQuit);
                }
            },

            Modal::SaveAs { mut input, mut cursor } => match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Left) => {
                    cursor = cursor.saturating_sub(1);
                    self.modal = Some(Modal::SaveAs { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Right) => {
                    cursor = (cursor + 1).min(input.chars().count());
                    self.modal = Some(Modal::SaveAs { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Home) => {
                    cursor = 0;
                    self.modal = Some(Modal::SaveAs { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::End) => {
                    cursor = input.chars().count();
                    self.modal = Some(Modal::SaveAs { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Tab) => {
                    input = self.tab_advance(&input);
                    cursor = input.chars().count();
                    self.modal = Some(Modal::SaveAs { input, cursor });
                }
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    insert_char_at(&mut input, &mut cursor, c);
                    self.modal = Some(Modal::SaveAs { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Backspace) => {
                    delete_char_before(&mut input, &mut cursor);
                    self.modal = Some(Modal::SaveAs { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Delete) => {
                    delete_char_after(&mut input, cursor);
                    self.modal = Some(Modal::SaveAs { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Enter) if !input.is_empty() => {
                    let path = crate::config::expand_tilde(&input);
                    self.save_to_path(path);
                }
                (KeyModifiers::NONE, KeyCode::Esc) => {
                    self.quit_after_save = false;
                }
                _ => {
                    self.modal = Some(Modal::SaveAs { input, cursor });
                }
            },

            Modal::OpenFile { mut input, mut cursor, mut selected } => match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Left) => {
                    if selected { cursor = 0; selected = false; }
                    else { cursor = cursor.saturating_sub(1); }
                    self.modal = Some(Modal::OpenFile { input, cursor, selected });
                }
                (KeyModifiers::NONE, KeyCode::Right) => {
                    if selected { cursor = input.chars().count(); selected = false; }
                    else { cursor = (cursor + 1).min(input.chars().count()); }
                    self.modal = Some(Modal::OpenFile { input, cursor, selected });
                }
                (KeyModifiers::NONE, KeyCode::Home) => {
                    cursor = 0; selected = false;
                    self.modal = Some(Modal::OpenFile { input, cursor, selected });
                }
                (KeyModifiers::NONE, KeyCode::End) => {
                    cursor = input.chars().count(); selected = false;
                    self.modal = Some(Modal::OpenFile { input, cursor, selected });
                }
                (KeyModifiers::NONE, KeyCode::Tab) => {
                    selected = false;
                    input = self.tab_advance(&input);
                    cursor = input.chars().count();
                    self.modal = Some(Modal::OpenFile { input, cursor, selected });
                }
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    if selected { input.clear(); cursor = 0; selected = false; }
                    insert_char_at(&mut input, &mut cursor, c);
                    self.modal = Some(Modal::OpenFile { input, cursor, selected });
                }
                (KeyModifiers::NONE, KeyCode::Backspace) => {
                    if selected { input.clear(); cursor = 0; selected = false; }
                    else { delete_char_before(&mut input, &mut cursor); }
                    self.modal = Some(Modal::OpenFile { input, cursor, selected });
                }
                (KeyModifiers::NONE, KeyCode::Delete) => {
                    if selected { input.clear(); cursor = 0; selected = false; }
                    else { delete_char_after(&mut input, cursor); }
                    self.modal = Some(Modal::OpenFile { input, cursor, selected });
                }
                (KeyModifiers::NONE, KeyCode::Enter) if !input.is_empty() => {
                    let path = crate::config::expand_tilde(&input);
                    self.open_file(path);
                }
                (KeyModifiers::NONE, KeyCode::Esc) => {}
                _ => {
                    self.modal = Some(Modal::OpenFile { input, cursor, selected });
                }
            },

            Modal::NewFile { mut input, mut cursor } => match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Left) => {
                    cursor = cursor.saturating_sub(1);
                    self.modal = Some(Modal::NewFile { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Right) => {
                    cursor = (cursor + 1).min(input.chars().count());
                    self.modal = Some(Modal::NewFile { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Home) => {
                    cursor = 0;
                    self.modal = Some(Modal::NewFile { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::End) => {
                    cursor = input.chars().count();
                    self.modal = Some(Modal::NewFile { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Tab) => {
                    input = self.tab_advance(&input);
                    cursor = input.chars().count();
                    self.modal = Some(Modal::NewFile { input, cursor });
                }
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    insert_char_at(&mut input, &mut cursor, c);
                    self.modal = Some(Modal::NewFile { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Backspace) => {
                    delete_char_before(&mut input, &mut cursor);
                    self.modal = Some(Modal::NewFile { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Delete) => {
                    delete_char_after(&mut input, cursor);
                    self.modal = Some(Modal::NewFile { input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Enter) if !input.is_empty() => {
                    let path = crate::config::expand_tilde(&input);
                    self.open_file(path);
                }
                (KeyModifiers::NONE, KeyCode::Esc) => {}
                _ => { self.modal = Some(Modal::NewFile { input, cursor }); }
            },

            Modal::Keybinds => {
                // Any key dismisses the keybinds overlay.
            }

            Modal::PdfPath { mut input, mut bw } => match (key.modifiers, key.code) {
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    input.push(c);
                    self.modal = Some(Modal::PdfPath { input, bw });
                }
                (KeyModifiers::NONE, KeyCode::Backspace) => {
                    input.pop();
                    self.modal = Some(Modal::PdfPath { input, bw });
                }
                (KeyModifiers::NONE, KeyCode::Tab) => {
                    bw = !bw;
                    self.modal = Some(Modal::PdfPath { input, bw });
                }
                (KeyModifiers::NONE, KeyCode::Enter) if !input.is_empty() => {
                    let path = crate::config::expand_tilde(&input);
                    self.last_pdf_path = Some(path.display().to_string());
                    self.start_pdf_export(path, bw);
                }
                (KeyModifiers::NONE, KeyCode::Esc) => {}
                _ => {
                    self.modal = Some(Modal::PdfPath { input, bw });
                }
            },

            Modal::RenameFile { path, mut input, mut cursor } => match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Left) => {
                    cursor = cursor.saturating_sub(1);
                    self.modal = Some(Modal::RenameFile { path, input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Right) => {
                    cursor = (cursor + 1).min(input.chars().count());
                    self.modal = Some(Modal::RenameFile { path, input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Home) => {
                    cursor = 0;
                    self.modal = Some(Modal::RenameFile { path, input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::End) => {
                    cursor = input.chars().count();
                    self.modal = Some(Modal::RenameFile { path, input, cursor });
                }
                (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                    insert_char_at(&mut input, &mut cursor, c);
                    self.modal = Some(Modal::RenameFile { path, input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Backspace) => {
                    delete_char_before(&mut input, &mut cursor);
                    self.modal = Some(Modal::RenameFile { path, input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Delete) => {
                    delete_char_after(&mut input, cursor);
                    self.modal = Some(Modal::RenameFile { path, input, cursor });
                }
                (KeyModifiers::NONE, KeyCode::Enter) if !input.is_empty() => {
                    self.confirm_rename(path, input);
                }
                (KeyModifiers::NONE, KeyCode::Esc) => {}
                _ => {
                    self.modal = Some(Modal::RenameFile { path, input, cursor });
                }
            },

            Modal::DeleteConfirm(path) => match (key.modifiers, key.code) {
                (KeyModifiers::NONE, KeyCode::Char('y')) | (KeyModifiers::NONE, KeyCode::Enter) => {
                    self.confirm_delete(path);
                }
                _ => {}
            },

            Modal::SaveBeforeNew => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.save();
                    self.open_scratch();
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.open_scratch();
                }
                _ => {}
            },

            Modal::Error(_) => {} // any key dismisses
        }
    }

    fn tick_marquee(&mut self) {
        const INTERVAL: Duration = Duration::from_millis(120);

        let key = (self.file_tree.selected, self.file_tree.fuzzy_active);
        if key != self.marquee_last_key {
            self.marquee_offset = 0;
            self.marquee_last_tick = Instant::now();
            self.marquee_last_key = key;
            return;
        }

        if self.marquee_last_tick.elapsed() >= INTERVAL {
            self.marquee_offset = self.marquee_offset.saturating_add(1);
            self.marquee_last_tick = Instant::now();
        }
    }

    pub fn focus(&self) -> &Focus {
        &self.input.focus
    }

    fn apply(&mut self, action: AppAction) {
        match action {
            AppAction::Quit => {
                if self.modified {
                    self.modal = Some(Modal::SaveBeforeQuit);
                } else {
                    self.should_quit = true;
                }
            }
            AppAction::Save => {
                self.save();
            }
            AppAction::CycleTodo => {
                self.do_cycle_todo();
            }
            AppAction::NextHeading => {
                let (row, _) = self.textarea.cursor();
                let owned: Vec<String> = self.textarea.lines().to_vec();
                let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
                if let Some(target) = next_heading(&refs, row) {
                    self.textarea.move_cursor(CursorMove::Jump(target as u16, 0));
                    self.ensure_cursor_visible();
                }
            }
            AppAction::PrevHeading => {
                let (row, _) = self.textarea.cursor();
                let owned: Vec<String> = self.textarea.lines().to_vec();
                let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
                if let Some(target) = prev_heading(&refs, row) {
                    self.textarea.move_cursor(CursorMove::Jump(target as u16, 0));
                    self.ensure_cursor_visible();
                }
            }
            AppAction::ToggleFocus => {
                if self.input.focus == Focus::FileTree {
                    self.file_tree.exit_fuzzy();
                }
                self.input.focus = match self.input.focus {
                    Focus::Editor => Focus::FileTree,
                    Focus::FileTree => Focus::Editor,
                };
            }
            AppAction::ToggleFileTree => {
                if self.file_tree_visible {
                    self.file_tree.exit_fuzzy();
                    self.file_tree_visible = false;
                    self.input.focus = Focus::Editor;
                } else {
                    self.file_tree_visible = true;
                    self.input.focus = Focus::FileTree;
                }
            }
            AppAction::TreeUp => {
                self.file_tree.move_up();
            }
            AppAction::TreeDown => {
                self.file_tree.move_down();
            }
            AppAction::TreeSelect => {
                self.tree_select();
            }
            AppAction::TreeRefresh => {
                self.file_tree.refresh();
            }
            AppAction::StartFuzzy => {
                self.file_tree.enter_fuzzy();
                self.input.focus = Focus::FileTree;
            }
            AppAction::StartSearch => {
                self.search_active = true;
                self.input.focus = Focus::Editor;
                if !self.search_query.is_empty() {
                    self.recompute_search();
                    self.search_find_from_cursor();
                }
            }
            AppAction::FollowLink => {
                self.do_follow_link();
            }
            AppAction::GoBack => {
                self.go_back();
            }
            AppAction::TreeRename => {
                self.do_rename();
            }
            AppAction::TreeDelete => {
                self.do_delete();
            }
            AppAction::OpenJournal => {
                self.open_journal();
            }
            AppAction::ToggleFold => {
                self.toggle_fold();
            }
            AppAction::ExportPdf => {
                if self.pdf_rx.is_some() {
                    self.status_msg = "PDF export already in progress".to_string();
                    return;
                }
                let stem = self.file_path.as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "norgx_export".to_string());
                let default = self.last_pdf_path.clone().unwrap_or_else(|| {
                    let home = dirs::home_dir()
                        .map(|h| h.display().to_string())
                        .unwrap_or_else(|| "~".to_string());
                    format!("{}/{}.pdf", home, stem)
                });
                self.modal = Some(Modal::PdfPath { input: default, bw: false });
            }
            AppAction::OpenFileDialog => {
                let (input, selected) = self.recent_open_default();
                let cursor = input.chars().count();
                self.modal = Some(Modal::OpenFile { input, cursor, selected });
            }
            AppAction::SaveAsDialog => {
                let current = self.file_path.as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let cursor = current.chars().count();
                self.modal = Some(Modal::SaveAs { input: current, cursor });
            }
            AppAction::NewFileDialog => {
                if self.modified {
                    self.modal = Some(Modal::SaveBeforeNew);
                } else {
                    self.open_scratch();
                }
            }
            AppAction::CloseFile => {
                if self.modified {
                    self.modal = Some(Modal::SaveBeforeNew);
                } else {
                    self.open_scratch();
                }
            }
            AppAction::ShowKeybinds => {
                self.modal = Some(Modal::Keybinds);
            }
            AppAction::Copy => {
                self.textarea.copy();
                let text = self.textarea.yank_text();
                if !text.is_empty() {
                    clipboard_set(&text);
                }
            }
            AppAction::Cut => {
                self.textarea.cut();
                let text = self.textarea.yank_text();
                if !text.is_empty() {
                    clipboard_set(&text);
                    self.modified = true;
                }
            }
            AppAction::Paste => {
                if let Some(text) = clipboard_get() {
                    if self.textarea.insert_str(text) {
                        self.modified = true;
                    }
                } else if self.textarea.paste() {
                    self.modified = true;
                }
            }
            AppAction::ShowHelp => {
                self.file_tree.exit_fuzzy();
                self.modal = Some(Modal::Keybinds);
            }
            AppAction::PassThrough(key) => {
                if self.config.editor.line_wrap {
                    match (key.modifiers, key.code) {
                        (KeyModifiers::NONE, KeyCode::Up) => {
                            self.wrap_navigate_up();
                            return;
                        }
                        (KeyModifiers::NONE, KeyCode::Down) => {
                            self.wrap_navigate_down();
                            return;
                        }
                        _ => {}
                    }
                }
                if self.textarea.input(key) {
                    self.modified = true;
                    self.status_msg = String::from("Ctrl+S save  Ctrl+Q quit  Ctrl+B files");
                    if self.search_active {
                        self.recompute_search();
                    }
                }
                self.ensure_cursor_visible();
            }
        }
    }

    // ── In-file search ────────────────────────────────────────────────────────

    fn recompute_search(&mut self) {
        self.search_matches.clear();
        if self.search_query.is_empty() {
            return;
        }
        let query_lower = self.search_query.to_lowercase();
        for (row, line) in self.textarea.lines().iter().enumerate() {
            let line_lower = line.to_lowercase();
            let mut byte_start = 0;
            while let Some(off) = line_lower[byte_start..].find(&query_lower) {
                let match_byte = byte_start + off;
                let char_col = line[..match_byte].chars().count();
                self.search_matches.push((row, char_col));
                byte_start = match_byte + query_lower.len().max(1);
            }
        }
        if self.search_current >= self.search_matches.len() {
            self.search_current = 0;
        }
    }

    fn search_find_from_cursor(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let (cur_row, cur_col) = self.textarea.cursor();
        let idx = self.search_matches
            .iter()
            .position(|&(r, c)| r > cur_row || (r == cur_row && c >= cur_col))
            .unwrap_or(0);
        self.search_current = idx;
        self.jump_to_search_match();
    }

    fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_current = (self.search_current + 1) % self.search_matches.len();
        self.jump_to_search_match();
    }

    fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_current = if self.search_current == 0 {
            self.search_matches.len() - 1
        } else {
            self.search_current - 1
        };
        self.jump_to_search_match();
    }

    fn jump_to_search_match(&mut self) {
        if let Some(&(row, col)) = self.search_matches.get(self.search_current) {
            self.textarea.move_cursor(CursorMove::Jump(row as u16, col as u16));
        }
    }

    // ── Link following ────────────────────────────────────────────────────────

    fn do_follow_link(&mut self) {
        let (row, col) = self.textarea.cursor();
        let line = self.textarea.lines()[row].clone();
        if let Some(link) = find_link_at_col(&line, col) {
            let path = resolve_link_path(
                &link,
                self.file_path.as_deref(),
                &self.file_tree.root,
            );
            // Push current position onto history so `gb` can return here.
            if let Some(current) = self.file_path.clone() {
                self.file_history.push((current, row, col));
            }
            self.open_file(path);
        } else {
            self.status_msg = String::from("No link under cursor");
        }
    }

    fn show_error(&mut self, msg: impl Into<String>) {
        self.modal = Some(Modal::Error(msg.into()));
    }

    // ── Mouse handling ────────────────────────────────────────────────────────

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.modal.is_some() { return; }

        let col = mouse.column;
        let row = mouse.row;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if !self.show_splash && self.in_area(col, row, self.editor_area) {
                    self.handle_editor_click(col, row);
                    self.input.focus = Focus::Editor;
                } else if self.file_tree_visible && self.in_area(col, row, self.tree_area) {
                    self.handle_tree_click(col, row);
                }
            }
            MouseEventKind::ScrollUp => {
                if self.in_area(col, row, self.editor_area) {
                    for _ in 0..3 {
                        if self.config.editor.line_wrap {
                            self.wrap_navigate_up();
                        } else {
                            self.textarea.move_cursor(CursorMove::Up);
                        }
                    }
                    self.ensure_cursor_visible();
                } else if self.file_tree_visible && self.in_area(col, row, self.tree_area) {
                    for _ in 0..3 { self.file_tree.move_up(); }
                }
            }
            MouseEventKind::ScrollDown => {
                if self.in_area(col, row, self.editor_area) {
                    for _ in 0..3 {
                        if self.config.editor.line_wrap {
                            self.wrap_navigate_down();
                        } else {
                            self.textarea.move_cursor(CursorMove::Down);
                        }
                    }
                    self.ensure_cursor_visible();
                } else if self.file_tree_visible && self.in_area(col, row, self.tree_area) {
                    for _ in 0..3 { self.file_tree.move_down(); }
                }
            }
            _ => {}
        }
    }

    fn in_area(&self, col: u16, row: u16, a: Rect) -> bool {
        a.width > 0 && col >= a.x && col < a.x + a.width && row >= a.y && row < a.y + a.height
    }

    fn handle_editor_click(&mut self, col: u16, row: u16) {
        let a = self.editor_area;
        let local_row = row.saturating_sub(a.y.saturating_add(1)) as usize;
        let local_col = col.saturating_sub(a.x.saturating_add(1)) as usize;

        let tab_w = self.config.editor.tab_width as usize;
        let lnum_width = if self.config.editor.line_numbers {
            self.textarea.lines().len().max(1).to_string().len() + 2
        } else {
            0
        };
        let content_col = local_col.saturating_sub(lnum_width);

        if !self.config.editor.line_wrap {
            let buf_row = self.row_offset + local_row;
            let buf_col = self.col_offset + content_col;
            self.textarea.move_cursor(CursorMove::Jump(buf_row as u16, buf_col as u16));
            self.ensure_cursor_visible();
            return;
        }

        let cw = self.editor_content_width();
        let lines = self.textarea.lines();
        let base = build_display_map(lines, &self.folded_headings);
        let display_map = expand_wrapped_lines(base, lines, cw, tab_w);

        match display_map.get(self.row_offset + local_row) {
            Some(DisplayRow::Actual(logical_row)) => {
                let r = *logical_row;
                let char_col = display_col_to_char_col(lines[r].as_str(), content_col, tab_w);
                self.textarea.move_cursor(CursorMove::Jump(r as u16, char_col as u16));
            }
            Some(DisplayRow::Continuation { source_row, display_start, indent_cols }) => {
                let r = *source_row;
                let target_dcol = display_start + content_col.saturating_sub(*indent_cols);
                let char_col = display_col_to_char_col(lines[r].as_str(), target_dcol, tab_w);
                self.textarea.move_cursor(CursorMove::Jump(r as u16, char_col as u16));
            }
            _ => return,
        }
        self.ensure_cursor_visible();
    }

    fn handle_tree_click(&mut self, _col: u16, row: u16) {
        let a = self.tree_area;
        let local_row = row.saturating_sub(a.y.saturating_add(1)) as usize;
        let scroll = self.compute_tree_scroll();
        let visual_row = local_row + scroll;

        let rlen = self.file_tree.recent.len();
        let entries_len = self.file_tree.entries().len();

        if rlen > 0 {
            if visual_row == 0 { return; } // "Recent" header
            if visual_row <= rlen {
                let idx = visual_row - 1;
                self.file_tree.selected = idx;
                if let Some(path) = self.file_tree.recent.get(idx).cloned() {
                    self.open_file(path);
                }
                return;
            }
            if visual_row == rlen + 1 { return; } // separator
            let tree_idx = visual_row - rlen - 2;
            if tree_idx < entries_len {
                self.file_tree.selected = rlen + tree_idx;
                if self.file_tree.entries()[tree_idx].is_dir {
                    self.input.focus = Focus::FileTree;
                    self.file_tree.toggle_dir();
                } else {
                    let path = self.file_tree.entries()[tree_idx].path.clone();
                    self.open_file(path);
                }
            }
        } else if visual_row < entries_len {
            self.file_tree.selected = visual_row;
            if self.file_tree.entries()[visual_row].is_dir {
                self.input.focus = Focus::FileTree;
                self.file_tree.toggle_dir();
            } else {
                let path = self.file_tree.entries()[visual_row].path.clone();
                self.open_file(path);
            }
        }
    }

    fn compute_tree_scroll(&self) -> usize {
        let rlen = self.file_tree.recent.len();
        let entries_len = self.file_tree.entries().len();
        let total_visual = if rlen > 0 { 1 + rlen + 1 + entries_len } else { entries_len };
        let selected = self.file_tree.selected;
        let visual_selected = if rlen > 0 {
            if selected < rlen { 1 + selected } else { 1 + rlen + 1 + (selected - rlen) }
        } else {
            selected
        };
        let height = self.tree_area.height.saturating_sub(2) as usize;
        if total_visual <= height || height == 0 { return 0; }
        let half = height / 2;
        let max = total_visual.saturating_sub(height);
        visual_selected.saturating_sub(half).min(max)
    }

    /// Returns the pre-fill (input, selected) for the Open File dialog.
    /// If there is a recent file, returns its path pre-selected; otherwise empty.
    fn recent_open_default(&self) -> (String, bool) {
        if let Some(path) = self.recent.files.front() {
            (path.display().to_string(), true)
        } else {
            (String::new(), false)
        }
    }

    /// Advance the tab-completion cycle for `current` and return the next completion.
    /// If no completions exist the input is returned unchanged.
    fn tab_advance(&mut self, current: &str) -> String {
        if self.tab_completions.is_none() {
            self.tab_completions = Some(path_completions(current));
            self.tab_idx = 0;
        }
        let completions = self.tab_completions.as_ref().unwrap();
        if completions.is_empty() {
            return current.to_string();
        }
        let result = completions[self.tab_idx % completions.len()].clone();
        self.tab_idx += 1;
        result
    }

    fn go_back(&mut self) {
        if let Some((path, row, col)) = self.file_history.pop() {
            self.open_file(path);
            self.textarea.move_cursor(CursorMove::Jump(row as u16, col as u16));
            self.ensure_cursor_visible();
        } else {
            self.status_msg = String::from("No history");
        }
    }

    fn do_rename(&mut self) {
        let Some(entry) = self.file_tree.selected_entry() else { return };
        if entry.is_dir { return; }
        let path = entry.path.clone();
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let cursor = name.chars().count();
        self.modal = Some(Modal::RenameFile { path, input: name, cursor });
    }

    fn do_delete(&mut self) {
        let Some(entry) = self.file_tree.selected_entry() else { return };
        if entry.is_dir { return; }
        self.modal = Some(Modal::DeleteConfirm(entry.path.clone()));
    }

    fn confirm_rename(&mut self, path: PathBuf, new_name: String) {
        let Some(parent) = path.parent() else { return };
        let new_path = parent.join(&new_name);
        match fs::rename(&path, &new_path) {
            Ok(_) => {
                if self.file_path.as_deref() == Some(&path) {
                    self.file_path = Some(new_path.clone());
                }
                self.file_tree.refresh();
                self.status_msg = format!("Renamed → {new_name}");
            }
            Err(e) => {
                self.show_error(format!("Rename failed: {e}"));
            }
        }
    }

    fn confirm_delete(&mut self, path: PathBuf) {
        match fs::remove_file(&path) {
            Ok(_) => {
                if self.file_path.as_deref() == Some(&path) {
                    self.textarea = TextArea::default();
                    self.file_path = None;
                    self.modified = false;
                    self.file_history.clear();
                }
                self.file_tree.refresh();
                self.status_msg = format!("Deleted {}", path.display());
            }
            Err(e) => {
                self.show_error(format!("Delete failed: {e}"));
            }
        }
    }

    fn tick_auto_save(&mut self) {
        let Some(secs) = self.config.editor.auto_save_secs else { return };
        if !self.modified || self.file_path.is_none() { return; }
        if self.last_auto_save.elapsed() >= Duration::from_secs(secs) {
            self.save();
            self.last_auto_save = Instant::now();
        }
    }

    fn fuzzy_select(&mut self) {
        let path = self.file_tree.fuzzy_selected_entry().map(|e| e.path.clone());
        self.file_tree.exit_fuzzy();
        if let Some(path) = path {
            self.open_file(path);
        }
    }

    fn tree_select(&mut self) {
        if let Some(path) = self.file_tree.recent_section_selected().cloned() {
            self.open_file(path);
            return;
        }
        let Some(entry) = self.file_tree.selected_entry() else { return };
        if entry.is_dir {
            self.file_tree.toggle_dir();
        } else {
            let path = entry.path.clone();
            self.open_file(path);
        }
    }

    fn open_scratch(&mut self) {
        self.textarea = TextArea::default();
        self.file_path = None;
        self.modified = false;
        self.row_offset = 0;
        self.col_offset = 0;
        self.show_splash = false;
        self.folded_headings.clear();
        self.file_history.clear();
        self.is_norg = true;
        self.status_msg = String::from("[scratch]  —  Ctrl+S to name and save");
        self.input.focus = Focus::Editor;
    }

    fn open_file(&mut self, path: PathBuf) {
        let content = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    self.show_error(format!("Error opening file: {e}"));
                    return;
                }
            }
        } else {
            String::new()
        };
        let lines: Vec<String> = content.lines().map(String::from).collect();
        self.textarea = TextArea::new(lines);
        self.file_path = Some(path.clone());
        self.is_norg = is_norg_path(&path);
        self.modified = false;
        self.row_offset = 0;
        self.col_offset = 0;
        self.show_splash = false;
        self.folded_headings.clear();
        self.status_msg = String::from("Ctrl+S save  Ctrl+Q quit  Ctrl+B files");
        self.input.focus = Focus::Editor;

        self.recent.push(&path);
        self.file_tree.recent = self.recent.files.iter().cloned().collect();
    }

    fn start_pdf_export(&mut self, output_path: PathBuf, bw: bool) {
        let stem = output_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "norgx_export".to_string());

        let lines: Vec<String> = self.textarea.lines().to_vec();
        let pdf_cfg = self.config.pdf.clone();
        let ui_cfg  = self.config.ui.clone();
        let viewer  = pdf_cfg.viewer.clone();

        let (tx, rx) = std::sync::mpsc::channel();
        self.pdf_rx = Some(rx);
        self.status_msg = "Exporting to PDF…".to_string();

        std::thread::spawn(move || {
            let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
            match export_to_pdf(&refs, &stem, &pdf_cfg, &ui_cfg, bw) {
                Ok(tmp_path) => {
                    // Copy to user-specified destination if different from temp output.
                    let final_path = if output_path != tmp_path {
                        if let Some(parent) = output_path.parent() {
                            if !parent.as_os_str().is_empty() {
                                let _ = fs::create_dir_all(parent);
                            }
                        }
                        match fs::copy(&tmp_path, &output_path) {
                            Ok(_) => output_path,
                            Err(e) => {
                                let _ = tx.send(Err(format!("Could not copy PDF: {e}")));
                                return;
                            }
                        }
                    } else {
                        tmp_path
                    };
                    let _ = std::process::Command::new(&viewer)
                        .arg(&final_path)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                    let _ = tx.send(Ok(final_path));
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                }
            }
        });
    }

    fn poll_pdf_export(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let result = match &self.pdf_rx {
            None => return,
            Some(rx) => match rx.try_recv() {
                Ok(r) => r,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err("export thread disconnected".to_string()),
            },
        };
        self.pdf_rx = None;
        match result {
            Ok(path) => self.status_msg = format!("PDF: {}", path.display()),
            Err(e)   => self.show_error(format!("PDF export failed: {e}")),
        }
    }

    fn toggle_fold(&mut self) {
        let (row, _) = self.textarea.cursor();
        let lines: Vec<String> = self.textarea.lines().to_vec();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        if heading_level_of(refs.get(row).copied().unwrap_or("")) == 0 {
            return;
        }
        if self.folded_headings.contains(&row) {
            self.folded_headings.remove(&row);
        } else {
            let range = fold_body_range(&refs, row);
            if !range.is_empty() {
                self.folded_headings.insert(row);
            }
        }
    }

    fn editor_content_width(&self) -> usize {
        let inner_w = self.editor_area.width.saturating_sub(2) as usize;
        if !self.config.editor.line_numbers {
            return inner_w;
        }
        let lnum_len = self.textarea.lines().len().max(1).to_string().len();
        inner_w.saturating_sub(lnum_len + 2)
    }

    fn wrap_navigate_down(&mut self) {
        let tab_w = self.config.editor.tab_width as usize;
        let cw = self.editor_content_width();
        if cw == 0 { return; }

        let (cur_row, cur_char_col) = self.textarea.cursor();
        let lines = self.textarea.lines();
        let line = lines[cur_row].as_str();
        let cur_dcol = char_to_display_col(line, cur_char_col, tab_w);
        let indent = leading_indent_width(line, tab_w);
        let seg_w = cw.saturating_sub(indent).max(1);
        let line_total = char_to_display_col(line, line.chars().count(), tab_w);

        let (seg_start, vis_col) = if cur_dcol < cw {
            (0usize, cur_dcol)
        } else {
            let seg_idx = (cur_dcol - cw) / seg_w;
            let ss = cw + seg_idx * seg_w;
            (ss, indent + (cur_dcol - ss))
        };

        let next_seg = if seg_start == 0 { cw } else { seg_start + seg_w };

        if next_seg < line_total {
            let target_dcol = (next_seg + vis_col.saturating_sub(indent)).min(line_total);
            let target_char = display_col_to_char_col(line, target_dcol, tab_w);
            self.textarea.move_cursor(CursorMove::Jump(cur_row as u16, target_char as u16));
        } else if cur_row + 1 < lines.len() {
            let next_row = cur_row + 1;
            let next_line = lines[next_row].as_str();
            let next_total = char_to_display_col(next_line, next_line.chars().count(), tab_w);
            let target_dcol = vis_col.min(next_total);
            let target_char = display_col_to_char_col(next_line, target_dcol, tab_w);
            self.textarea.move_cursor(CursorMove::Jump(next_row as u16, target_char as u16));
            self.ensure_cursor_visible();
        }
    }

    fn wrap_navigate_up(&mut self) {
        let tab_w = self.config.editor.tab_width as usize;
        let cw = self.editor_content_width();
        if cw == 0 { return; }

        let (cur_row, cur_char_col) = self.textarea.cursor();
        let lines = self.textarea.lines();
        let line = lines[cur_row].as_str();
        let cur_dcol = char_to_display_col(line, cur_char_col, tab_w);
        let indent = leading_indent_width(line, tab_w);
        let seg_w = cw.saturating_sub(indent).max(1);
        let line_total = char_to_display_col(line, line.chars().count(), tab_w);

        let (seg_start, vis_col) = if cur_dcol < cw {
            (0usize, cur_dcol)
        } else {
            let seg_idx = (cur_dcol - cw) / seg_w;
            let ss = cw + seg_idx * seg_w;
            (ss, indent + (cur_dcol - ss))
        };

        if seg_start > 0 {
            let prev_seg = if seg_start == cw { 0 } else { seg_start - seg_w };
            let target_dcol = if prev_seg == 0 {
                vis_col.min(line_total)
            } else {
                (prev_seg + vis_col.saturating_sub(indent)).min(line_total)
            };
            let target_char = display_col_to_char_col(line, target_dcol, tab_w);
            self.textarea.move_cursor(CursorMove::Jump(cur_row as u16, target_char as u16));
        } else if cur_row > 0 {
            let prev_row = cur_row - 1;
            let prev_line = lines[prev_row].as_str();
            let prev_total = char_to_display_col(prev_line, prev_line.chars().count(), tab_w);
            let prev_indent = leading_indent_width(prev_line, tab_w);
            let prev_seg_w = cw.saturating_sub(prev_indent).max(1);

            let target_dcol = if prev_total <= cw {
                vis_col.min(prev_total)
            } else {
                let segs_after = prev_total.saturating_sub(cw).saturating_sub(1) / prev_seg_w;
                let last_seg = cw + segs_after * prev_seg_w;
                let last_end = (last_seg + prev_seg_w).min(prev_total);
                (last_seg + vis_col.saturating_sub(prev_indent)).min(last_end)
            };
            let target_char = display_col_to_char_col(prev_line, target_dcol, tab_w);
            self.textarea.move_cursor(CursorMove::Jump(prev_row as u16, target_char as u16));
            self.ensure_cursor_visible();
        }
    }

    fn ensure_cursor_visible(&mut self) {
        let (cursor_row, _) = self.textarea.cursor();
        if self.folded_headings.is_empty() {
            return;
        }
        let lines: Vec<String> = self.textarea.lines().to_vec();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let to_unfold: Vec<usize> = self
            .folded_headings
            .iter()
            .copied()
            .filter(|&h| fold_body_range(&refs, h).contains(&cursor_row))
            .collect();
        for h in to_unfold {
            self.folded_headings.remove(&h);
        }
    }

    fn open_journal(&mut self) {
        let dir = self.config.journal.dir_path(&self.file_tree.root);
        let date_str = Local::now()
            .format(&self.config.journal.date_format)
            .to_string();
        let path = dir.join(format!("{date_str}.norg"));

        if let Err(e) = fs::create_dir_all(&dir) {
            self.show_error(format!("Could not create journal dir: {e}"));
            return;
        }

        let is_new = !path.exists();
        self.open_file(path);

        if is_new {
            let template = self.config.journal.template
                .as_deref()
                .unwrap_or("* {date}\n\n")
                .replace("{date}", &date_str);
            self.textarea.insert_str(&template);
            self.modified = true;
        }
    }

    fn do_cycle_todo(&mut self) {
        let (row, _) = self.textarea.cursor();
        let line = self.textarea.lines()[row].clone();
        if let Some(new_line) = cycle_todo(&line) {
            self.textarea.move_cursor(CursorMove::Head);
            self.textarea.delete_str(line.chars().count());
            self.textarea.insert_str(&new_line);
            self.modified = true;
        }
    }

    fn save(&mut self) {
        if let Some(path) = self.file_path.clone() {
            self.save_to_path(path);
        } else {
            self.modal = Some(Modal::SaveAs { input: String::new(), cursor: 0 });
        }
    }

    fn save_to_path(&mut self, path: PathBuf) {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = fs::create_dir_all(parent) {
                    self.show_error(format!("Cannot create directory: {e}"));
                    self.quit_after_save = false;
                    return;
                }
            }
        }
        let content = self.textarea.lines().join("\n");
        match fs::write(&path, content) {
            Ok(_) => {
                self.file_path = Some(path.clone());
                self.modified = false;
                self.status_msg = format!("Saved — {}", path.display());
                self.recent.push(&path);
                self.file_tree.recent = self.recent.files.iter().cloned().collect();
                if self.quit_after_save {
                    self.should_quit = true;
                    self.quit_after_save = false;
                }
            }
            Err(e) => {
                self.show_error(format!("Save failed: {e}"));
                self.quit_after_save = false;
            }
        }
    }
}

/// Determine the workspace root.
/// Priority: config workspace_root → ~/notes → cwd.
/// The opened file's directory is never used as the root; instead the tree is
/// expanded to reveal the file after init.
fn is_norg_path(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("norg")
}

/// Generate sorted filesystem completions for a partial path input.
/// Trailing `/` lists the directory contents; otherwise the parent directory is
/// searched for entries whose name starts with the final component.
/// Directories are returned with a trailing `/`. Tilde prefix is preserved.
fn path_completions(input: &str) -> Vec<String> {
    let expanded = crate::config::expand_tilde(input);
    let exp_str = expanded.display().to_string();

    let (search_dir, prefix) = if exp_str.ends_with('/') {
        (PathBuf::from(&exp_str), String::new())
    } else {
        let p = PathBuf::from(&exp_str);
        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
        let pfx = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        (dir, pfx)
    };

    let Ok(read_dir) = fs::read_dir(&search_dir) else { return vec![] };

    let home_str = dirs::home_dir().map(|h| h.display().to_string());
    let use_tilde = input.starts_with('~');

    let mut completions: Vec<String> = read_dir
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            !s.starts_with('.') && s.starts_with(&prefix)
        })
        .map(|e| {
            let full = e.path();
            let raw = if full.is_dir() {
                format!("{}/", full.display())
            } else {
                full.display().to_string()
            };
            if use_tilde {
                if let Some(ref home) = home_str {
                    if raw.starts_with(home.as_str()) {
                        return format!("~{}", &raw[home.len()..]);
                    }
                }
            }
            raw
        })
        .collect();

    completions.sort();
    completions
}

fn workspace_root(_file: Option<&std::path::Path>, config_root: Option<&std::path::Path>) -> PathBuf {
    if let Some(root) = config_root {
        if root.exists() {
            return root.to_path_buf();
        }
    }

    if let Some(home) = dirs::home_dir() {
        let notes = home.join("notes");
        if notes.exists() {
            return notes;
        }
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

// ── Modal input field helpers ─────────────────────────────────────────────────

fn insert_char_at(s: &mut String, cursor: &mut usize, c: char) {
    let byte = s.char_indices().nth(*cursor).map(|(i, _)| i).unwrap_or(s.len());
    s.insert(byte, c);
    *cursor += 1;
}

fn delete_char_before(s: &mut String, cursor: &mut usize) {
    if *cursor == 0 { return; }
    *cursor -= 1;
    let byte = s.char_indices().nth(*cursor).map(|(i, _)| i).unwrap_or(s.len());
    s.remove(byte);
}

fn delete_char_after(s: &mut String, cursor: usize) {
    if let Some((byte, _)) = s.char_indices().nth(cursor) {
        s.remove(byte);
    }
}

// ── System clipboard helpers ──────────────────────────────────────────────────

/// Write `text` to the system clipboard.
/// Tries wl-copy (Wayland) first, then xclip (X11).
fn clipboard_set(text: &str) {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    for (cmd, args) in [
        ("wl-copy", vec![] as Vec<&str>),
        ("xclip",   vec!["-selection", "clipboard"]),
    ] {
        if let Ok(mut child) = Command::new(cmd)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            // Don't wait — wl-copy/xclip must stay alive as clipboard owner.
            return;
        }
    }
}

/// Read text from the system clipboard.
/// Tries wl-paste (Wayland) first, then xclip (X11).
fn clipboard_get() -> Option<String> {
    use std::process::Command;

    for (cmd, args) in [
        ("wl-paste", vec!["--no-newline"]),
        ("xclip",    vec!["-o", "-selection", "clipboard"]),
    ] {
        if let Ok(out) = Command::new(cmd).args(&args).output() {
            if out.status.success() {
                if let Ok(s) = String::from_utf8(out.stdout) {
                    return Some(s);
                }
            }
        }
    }
    None
}
