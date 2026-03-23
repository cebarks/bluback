use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::types::{SettingItem, SettingsState};

pub fn render(f: &mut Frame, state: &SettingsState) {
    let area = f.area();
    let popup_width = 60.min(area.width.saturating_sub(4));
    let content_height = state.items.len() as u16 + 3; // +2 borders +1 hint
    let popup_height = content_height.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(area.x + x, area.y + y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let title = if state.dirty { " Settings (modified) " } else { " Settings " };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Reserve 1 row for hint bar
    let list_height = inner.height.saturating_sub(1) as usize;
    let scroll_offset = if state.cursor >= state.scroll_offset + list_height {
        state.cursor - list_height + 1
    } else if state.cursor < state.scroll_offset {
        state.cursor
    } else {
        state.scroll_offset
    };

    let label_width = 22;

    for (i, item) in state.items.iter().enumerate().skip(scroll_offset).take(list_height) {
        let row_y = inner.y + (i - scroll_offset) as u16;
        let row_area = Rect::new(inner.x, row_y, inner.width, 1);
        let is_selected = i == state.cursor;
        let max_val_width = inner.width as usize - label_width - 4;

        match item {
            SettingItem::Separator { label } => {
                if let Some(lbl) = label {
                    let span = Span::styled(
                        format!("  {}", lbl),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    );
                    f.render_widget(Paragraph::new(Line::from(span)), row_area);
                }
            }
            SettingItem::Toggle { label, value, .. } => {
                let val_str = if *value { "[ON]" } else { "[OFF]" };
                let val_color = if *value { Color::Green } else { Color::Red };
                let line = Line::from(vec![
                    Span::raw(format!("  {:width$}", label, width = label_width)),
                    Span::styled(val_str, Style::default().fg(val_color)),
                ]);
                let style = if is_selected { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
                f.render_widget(Paragraph::new(line).style(style), row_area);
            }
            SettingItem::Choice { label, options, selected, .. } => {
                let val_str = format!("[{}]", options[*selected]);
                let line = Line::from(vec![
                    Span::raw(format!("  {:width$}", label, width = label_width)),
                    Span::styled(val_str, Style::default().fg(Color::Cyan)),
                ]);
                let style = if is_selected { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
                f.render_widget(Paragraph::new(line).style(style), row_area);
            }
            SettingItem::Text { label, key, value, .. } => {
                let is_editing = state.editing == Some(i);
                let is_dimmed = (key == "tv_format" || key == "movie_format") && is_preset_active(state);
                let display_val = if is_editing {
                    render_edit_buffer(&state.input_buffer, state.cursor_pos, max_val_width)
                } else if key == "tmdb_api_key" && !value.is_empty() {
                    mask_api_key(value)
                } else {
                    truncate(value, max_val_width)
                };
                let val_style = if is_editing {
                    Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
                } else if is_dimmed {
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
                } else {
                    Style::default()
                };
                let line = Line::from(vec![
                    Span::raw(format!("  {:width$}", label, width = label_width)),
                    Span::styled(display_val, val_style),
                ]);
                let style = if is_selected && !is_editing { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
                f.render_widget(Paragraph::new(line).style(style), row_area);
            }
            SettingItem::Number { label, value, .. } => {
                let is_editing = state.editing == Some(i);
                let display_val = if is_editing {
                    render_edit_buffer(&state.input_buffer, state.cursor_pos, max_val_width)
                } else {
                    value.to_string()
                };
                let val_style = if is_editing {
                    Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)
                } else {
                    Style::default()
                };
                let line = Line::from(vec![
                    Span::raw(format!("  {:width$}", label, width = label_width)),
                    Span::styled(display_val, val_style),
                ]);
                let style = if is_selected && !is_editing { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
                f.render_widget(Paragraph::new(line).style(style), row_area);
            }
            SettingItem::Action { label, .. } => {
                let mut spans = vec![Span::styled(
                    format!("  {}", label),
                    Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
                )];
                if let Some(ref msg) = state.save_message {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(msg.clone(), Style::default().fg(Color::Green)));
                }
                let style = if is_selected { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
                f.render_widget(Paragraph::new(Line::from(spans)).style(style), row_area);
            }
        }
    }

    // Hint bar
    let hint_y = inner.y + inner.height - 1;
    let hint_area = Rect::new(inner.x, hint_y, inner.width, 1);
    let hint = if state.editing.is_some() {
        "Enter: Confirm  Esc: Cancel"
    } else if state.confirm_close.is_some() {
        "Save before closing? [y] Yes  [n] No  [Esc] Cancel"
    } else {
        "Ctrl+S: Save  Esc: Close  Enter/Space: Edit"
    };
    f.render_widget(
        Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)).alignment(Alignment::Center),
        hint_area,
    );
}

fn is_preset_active(state: &SettingsState) -> bool {
    state.items.iter().any(|item| {
        matches!(item, SettingItem::Choice { key, selected, .. } if key == "preset" && *selected != 0)
    })
}

fn truncate(s: &str, max_len: usize) -> String {
    if max_len == 0 { return String::new(); }
    if s.len() <= max_len { s.to_string() }
    else if max_len > 3 { format!("{}...", &s[..max_len - 3]) }
    else { s[..max_len].to_string() }
}

fn mask_api_key(key: &str) -> String {
    if key.len() <= 4 { "*".repeat(key.len()) }
    else { format!("{}...{}", "*".repeat(key.len() - 4), &key[key.len() - 4..]) }
}

fn render_edit_buffer(buf: &str, cursor_pos: usize, max_len: usize) -> String {
    if max_len == 0 { return String::new(); }
    if buf.len() <= max_len { buf.to_string() }
    else if cursor_pos >= max_len {
        let start = cursor_pos.saturating_sub(max_len) + 1;
        buf[start..].to_string()
    } else {
        buf[..max_len].to_string()
    }
}

pub enum SettingsAction {
    None,
    Close,
    Save,
    SaveAndClose,
}

pub fn handle_input(state: &mut SettingsState, key: KeyEvent) -> SettingsAction {
    // Handle confirm_close prompt (--settings standalone mode)
    if state.confirm_close.is_some() {
        return match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => { state.confirm_close = None; SettingsAction::SaveAndClose }
            KeyCode::Char('n') | KeyCode::Char('N') => { state.confirm_close = None; SettingsAction::Close }
            KeyCode::Esc => { state.confirm_close = None; SettingsAction::None }
            _ => SettingsAction::None,
        };
    }

    // Ctrl+S always saves (even during editing)
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if let Some(idx) = state.editing {
            confirm_edit(state, idx);
        }
        return SettingsAction::Save;
    }

    // Handle editing mode
    if let Some(idx) = state.editing {
        return handle_edit_input(state, key, idx);
    }

    // Navigation and actions
    match key.code {
        KeyCode::Up => { state.move_cursor_up(); SettingsAction::None }
        KeyCode::Down => { state.move_cursor_down(); SettingsAction::None }
        KeyCode::Enter | KeyCode::Char(' ') => handle_activate(state),
        KeyCode::Left => { handle_cycle(state, false); SettingsAction::None }
        KeyCode::Right => { handle_cycle(state, true); SettingsAction::None }
        KeyCode::Esc => {
            if state.standalone && state.dirty {
                state.confirm_close = Some(true);
                SettingsAction::None
            } else {
                SettingsAction::Close
            }
        }
        _ => SettingsAction::None,
    }
}

fn handle_activate(state: &mut SettingsState) -> SettingsAction {
    let idx = state.cursor;
    match &state.items[idx] {
        SettingItem::Toggle { .. } => {
            if let SettingItem::Toggle { value, .. } = &mut state.items[idx] {
                *value = !*value;
                state.dirty = true;
            }
            SettingsAction::None
        }
        SettingItem::Choice { .. } => { handle_cycle(state, true); SettingsAction::None }
        SettingItem::Text { key, value, .. } => {
            if (key == "tv_format" || key == "movie_format") && is_preset_active(state) {
                return SettingsAction::None;
            }
            state.input_buffer = value.clone();
            state.cursor_pos = state.input_buffer.len();
            state.editing = Some(idx);
            SettingsAction::None
        }
        SettingItem::Number { value, .. } => {
            state.input_buffer = value.to_string();
            state.cursor_pos = state.input_buffer.len();
            state.editing = Some(idx);
            SettingsAction::None
        }
        SettingItem::Action { .. } => SettingsAction::Save,
        SettingItem::Separator { .. } => SettingsAction::None,
    }
}

fn handle_cycle(state: &mut SettingsState, forward: bool) {
    let idx = state.cursor;
    if let SettingItem::Choice { options, selected, .. } = &mut state.items[idx] {
        let len = options.len();
        *selected = if forward { (*selected + 1) % len } else { (*selected + len - 1) % len };
        state.dirty = true;
    }
}

fn handle_edit_input(state: &mut SettingsState, key: KeyEvent, idx: usize) -> SettingsAction {
    let is_number = matches!(state.items[idx], SettingItem::Number { .. });
    match key.code {
        KeyCode::Enter => { confirm_edit(state, idx); SettingsAction::None }
        KeyCode::Esc => { state.editing = None; state.input_buffer.clear(); state.cursor_pos = 0; SettingsAction::None }
        KeyCode::Backspace => {
            if state.cursor_pos > 0 {
                state.input_buffer.remove(state.cursor_pos - 1);
                state.cursor_pos -= 1;
            }
            SettingsAction::None
        }
        KeyCode::Delete => {
            if state.cursor_pos < state.input_buffer.len() {
                state.input_buffer.remove(state.cursor_pos);
            }
            SettingsAction::None
        }
        KeyCode::Left => { state.cursor_pos = state.cursor_pos.saturating_sub(1); SettingsAction::None }
        KeyCode::Right => { if state.cursor_pos < state.input_buffer.len() { state.cursor_pos += 1; } SettingsAction::None }
        KeyCode::Home => { state.cursor_pos = 0; SettingsAction::None }
        KeyCode::End => { state.cursor_pos = state.input_buffer.len(); SettingsAction::None }
        KeyCode::Char(c) => {
            if is_number && !c.is_ascii_digit() { return SettingsAction::None; }
            state.input_buffer.insert(state.cursor_pos, c);
            state.cursor_pos += 1;
            SettingsAction::None
        }
        _ => SettingsAction::None,
    }
}

fn confirm_edit(state: &mut SettingsState, idx: usize) {
    let new_value = state.input_buffer.clone();
    match &mut state.items[idx] {
        SettingItem::Text { value, .. } => { *value = new_value; state.dirty = true; }
        SettingItem::Number { value, .. } => {
            if let Ok(n) = new_value.parse::<u32>() {
                if n > 0 { *value = n; state.dirty = true; }
            }
        }
        _ => {}
    }
    state.editing = None;
    state.input_buffer.clear();
    state.cursor_pos = 0;
}
