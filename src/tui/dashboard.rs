use crossterm::event::KeyEvent;
use ratatui::prelude::*;

use super::App;

pub fn render(_f: &mut Frame, _app: &App) { todo!() }
pub fn render_done(_f: &mut Frame, _app: &App) { todo!() }
pub fn handle_input(_app: &mut App, _key: KeyEvent) { todo!() }
pub fn tick(_app: &mut App) -> anyhow::Result<()> { todo!() }
