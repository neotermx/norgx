use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};

use crate::app::App;

pub(crate) mod editor;
mod filetree;
mod modal;
mod splash;
mod statusbar;

pub fn draw(f: &mut Frame, app: &mut App) {
    let tree_width = if app.file_tree_visible { app.config.ui.file_tree_width } else { 0 };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(tree_width)])
        .split(rows[0]);

    app.editor_area = cols[0];
    app.tree_area = if app.file_tree_visible { cols[1] } else { Rect::default() };

    editor::render(f, app, cols[0]);
    if app.file_tree_visible {
        filetree::render(f, app, cols[1]);
    }
    statusbar::render(f, app, rows[1]);
    modal::render(f, app);
}
