use crossterm::event::KeyEvent;
use ratatui::prelude::*;

use super::App;

pub fn render_scanning(f: &mut Frame, app: &App) {
    let text = ratatui::widgets::Paragraph::new(app.status_message.as_str());
    f.render_widget(text, f.area());
}

pub fn render_tmdb_search(_f: &mut Frame, _app: &App) { todo!() }
pub fn render_show_select(_f: &mut Frame, _app: &App) { todo!() }
pub fn render_season_episode(_f: &mut Frame, _app: &App) { todo!() }
pub fn render_playlist_select(_f: &mut Frame, _app: &App) { todo!() }
pub fn render_confirm(_f: &mut Frame, _app: &App) { todo!() }

pub fn handle_tmdb_search_input(_app: &mut App, _key: KeyEvent) { todo!() }
pub fn handle_show_select_input(_app: &mut App, _key: KeyEvent) { todo!() }
pub fn handle_season_episode_input(_app: &mut App, _key: KeyEvent) { todo!() }
pub fn handle_playlist_select_input(_app: &mut App, _key: KeyEvent) { todo!() }
pub fn handle_confirm_input(_app: &mut App, _key: KeyEvent) { todo!() }
