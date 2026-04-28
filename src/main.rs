use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{env, io};

mod app;
mod config;
mod export;
mod input;
mod norg;
mod recent;
mod setup;
mod ui;
mod workspace;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.get(1).map(|s| s.as_str()) == Some("--setup") {
        return setup::run();
    }

    let file_path = args.get(1).cloned();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Enable keyboard enhancement on supporting terminals (e.g. Kitty).
    // This lets us distinguish Ctrl+Shift+S from Ctrl+S, Ctrl+/ etc.
    let kb_enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    if kb_enhanced {
        execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app::App::new(file_path).and_then(|mut app| app.run(&mut terminal));

    // Restore terminal regardless of result
    if kb_enhanced {
        execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags)?;
    }
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = &result {
        eprintln!("norgx error: {e}");
    }

    result
}
