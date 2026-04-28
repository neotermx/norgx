use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Focus {
    #[default]
    Editor,
    FileTree,
}

#[derive(Debug)]
pub enum AppAction {
    Quit,
    Save,
    CycleTodo,
    NextHeading,
    PrevHeading,
    ToggleFocus,
    ToggleFileTree,
    TreeUp,
    TreeDown,
    TreeSelect,
    TreeRefresh,
    StartFuzzy,
    StartSearch,
    FollowLink,
    GoBack,
    TreeRename,
    TreeDelete,
    ShowHelp,
    Copy,
    Cut,
    Paste,
    OpenJournal,
    ToggleFold,
    ExportPdf,
    OpenFileDialog,
    SaveAsDialog,
    NewFileDialog,
    CloseFile,
    ShowKeybinds,
    PassThrough(KeyEvent),
}

/// Tracks pending state for multi-key sequences and which pane is focused.
#[derive(Default)]
pub struct InputState {
    pending: Option<KeyCode>,
    pub focus: Focus,
}

impl InputState {
    pub fn handle(&mut self, key: KeyEvent) -> Vec<AppAction> {
        // Global bindings active from any pane.
        if key.modifiers == KeyModifiers::CONTROL {
            match key.code {
                KeyCode::Char('b')  => return vec![AppAction::ToggleFocus],
                KeyCode::Char('\\') => return vec![AppAction::ToggleFileTree],
                KeyCode::Char('q')  => return vec![AppAction::Quit],
                KeyCode::Char('s')  => return vec![AppAction::Save],
                KeyCode::Char('o')  => return vec![AppAction::OpenFileDialog],
                KeyCode::Char('n')  => return vec![AppAction::NewFileDialog],
                KeyCode::Char('j')  => return vec![AppAction::OpenJournal],
                KeyCode::Char('p')  => return vec![AppAction::ExportPdf],
                KeyCode::Char('/')  => return vec![AppAction::ShowKeybinds],
                KeyCode::Char('w')  => return vec![AppAction::CloseFile],
                _ => {}
            }
        }
        // Ctrl+Shift+S (keyboard-enhanced terminals).
        if key.modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT {
            if let KeyCode::Char('s') = key.code {
                return vec![AppAction::SaveAsDialog];
            }
        }

        match self.focus {
            Focus::Editor => self.handle_editor(key),
            Focus::FileTree => self.handle_tree(key),
        }
    }

    fn handle_editor(&mut self, key: KeyEvent) -> Vec<AppAction> {
        if let Some(pending) = self.pending.take() {
            return self.resolve_sequence(pending, key);
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('q')) => vec![AppAction::Quit],
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => vec![AppAction::Save],
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => vec![AppAction::CycleTodo],
            (KeyModifiers::CONTROL, KeyCode::Char('f')) => vec![AppAction::StartSearch],
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => vec![AppAction::Copy],
            (KeyModifiers::CONTROL, KeyCode::Char('x')) => vec![AppAction::Cut],
            (KeyModifiers::CONTROL, KeyCode::Char('v')) => vec![AppAction::Paste],
            (KeyModifiers::CONTROL, KeyCode::Char('j')) => vec![AppAction::OpenJournal],
            (KeyModifiers::CONTROL, KeyCode::Char('p')) => vec![AppAction::ExportPdf],
            (KeyModifiers::CONTROL, KeyCode::Char('o')) => vec![AppAction::OpenFileDialog],
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => vec![AppAction::NewFileDialog],
            (KeyModifiers::CONTROL, KeyCode::Char('/')) => vec![AppAction::ShowKeybinds],
            // Ctrl+Shift+S — requires keyboard enhancement (DISAMBIGUATE_ESCAPE_CODES).
            (m, KeyCode::Char('s')) if m == KeyModifiers::CONTROL | KeyModifiers::SHIFT => {
                vec![AppAction::SaveAsDialog]
            }
            (KeyModifiers::NONE, KeyCode::Esc) => vec![AppAction::ShowHelp],

            (KeyModifiers::NONE, KeyCode::Char(']')) => {
                self.pending = Some(KeyCode::Char(']'));
                vec![]
            }
            (KeyModifiers::NONE, KeyCode::Char('[')) => {
                self.pending = Some(KeyCode::Char('['));
                vec![]
            }
            (KeyModifiers::NONE, KeyCode::Char('g')) => {
                self.pending = Some(KeyCode::Char('g'));
                vec![]
            }
            (KeyModifiers::NONE, KeyCode::Char('z')) => {
                self.pending = Some(KeyCode::Char('z'));
                vec![]
            }

            _ => vec![AppAction::PassThrough(key)],
        }
    }

    fn handle_tree(&mut self, key: KeyEvent) -> Vec<AppAction> {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('q')) => vec![AppAction::Quit],

            (KeyModifiers::NONE, KeyCode::Char('j'))
            | (KeyModifiers::NONE, KeyCode::Down) => vec![AppAction::TreeDown],

            (KeyModifiers::NONE, KeyCode::Char('k'))
            | (KeyModifiers::NONE, KeyCode::Up) => vec![AppAction::TreeUp],

            (KeyModifiers::NONE, KeyCode::Enter) => vec![AppAction::TreeSelect],
            (KeyModifiers::NONE, KeyCode::Char('r')) => vec![AppAction::TreeRename],
            (KeyModifiers::NONE, KeyCode::Char('d')) => vec![AppAction::TreeDelete],
            (KeyModifiers::NONE, KeyCode::Char('R')) => vec![AppAction::TreeRefresh],
            (KeyModifiers::NONE, KeyCode::Char('/')) => vec![AppAction::StartFuzzy],
            (KeyModifiers::NONE, KeyCode::Esc) => vec![AppAction::ShowHelp],

            _ => vec![],
        }
    }

    fn resolve_sequence(&mut self, pending: KeyCode, key: KeyEvent) -> Vec<AppAction> {
        match (pending, key.code) {
            (KeyCode::Char(']'), KeyCode::Char('h')) => vec![AppAction::NextHeading],
            (KeyCode::Char('['), KeyCode::Char('h')) => vec![AppAction::PrevHeading],
            (KeyCode::Char('g'), KeyCode::Char('f')) => vec![AppAction::FollowLink],
            (KeyCode::Char('g'), KeyCode::Char('b')) => vec![AppAction::GoBack],
            (KeyCode::Char('z'), KeyCode::Char('a')) => vec![AppAction::ToggleFold],
            _ => {
                let flush = AppAction::PassThrough(KeyEvent::new(pending, KeyModifiers::NONE));
                let mut actions = vec![flush];
                actions.extend(self.handle_editor(key));
                actions
            }
        }
    }
}
