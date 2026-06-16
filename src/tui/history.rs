use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table};

use crate::history::HistoryDb;
use crate::types::HistoryOverlayState;

pub enum HistoryAction {
    None,
    Close,
    Refresh,
}

pub fn render(f: &mut Frame, state: &HistoryOverlayState) {
    let area = f.area();
    let popup_width = (area.width * 4 / 5).min(area.width.saturating_sub(4));
    let popup_height = (area.height * 4 / 5).min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(area.x + x, area.y + y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    if let Some(ref confirm) = state.confirm_action {
        render_confirm(f, confirm, popup_area);
        return;
    }

    if let Some(ref detail) = state.detail_view {
        render_detail(f, detail, popup_area);
        return;
    }

    render_list(f, state, popup_area);
}

fn render_list(f: &mut Frame, state: &HistoryOverlayState, popup_area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" History (Ctrl+H to close) ");
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    if state.sessions.is_empty() {
        let msg = Paragraph::new("No history found.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        let centered = Rect::new(inner.x, inner.y + inner.height / 2, inner.width, 1);
        f.render_widget(msg, centered);

        let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
        f.render_widget(
            Paragraph::new("Esc: Close")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            hint_area,
        );
        return;
    }

    // Reserve 1 row for hint bar
    let list_height = inner.height.saturating_sub(2) as usize; // -1 header, -1 hint

    // Build header
    let header = Row::new(vec![
        Cell::from("ID").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Date").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Title").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Season").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Disc").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Files").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Size").style(Style::default().add_modifier(Modifier::BOLD)),
    ])
    .style(Style::default().fg(Color::White));

    // Compute scroll offset
    let scroll_offset = if list_height == 0 {
        0
    } else if state.selected >= list_height {
        state.selected - list_height + 1
    } else {
        0
    };

    let title_width = inner.width.saturating_sub(50) as usize;

    let rows: Vec<Row> = state
        .sessions
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(list_height)
        .map(|(i, s)| {
            let date = s.started_at.split('T').next().unwrap_or("");
            let season_str = s
                .season
                .map(|n| format!("S{:02}", n))
                .unwrap_or_else(|| "-".to_string());
            let disc_str = s
                .disc_number
                .map(|n| format!("D{}", n))
                .unwrap_or_else(|| "-".to_string());
            let files_str = format!("{}/{}", s.files_completed, s.files_total);
            let size_str = format_size(s.total_size);
            let title = truncate(&s.title, title_width);

            let status = s.display_status();
            let status_style = match status {
                "completed" => Style::default().fg(Color::Green),
                "failed" => Style::default().fg(Color::Red),
                "partial" => Style::default().fg(Color::Yellow),
                "in_progress" => Style::default().fg(Color::Cyan),
                "cancelled" => Style::default().fg(Color::DarkGray),
                _ => Style::default(),
            };

            let row = Row::new(vec![
                Cell::from(format!("{}", s.id)),
                Cell::from(date.to_string()),
                Cell::from(title),
                Cell::from(season_str),
                Cell::from(disc_str),
                Cell::from(Span::styled(status.to_string(), status_style)),
                Cell::from(files_str),
                Cell::from(size_str),
            ]);

            if i == state.selected {
                row.style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Length(4),
        Constraint::Length(11),
        Constraint::Min(10),
        Constraint::Length(6),
        Constraint::Length(4),
        Constraint::Length(11),
        Constraint::Length(5),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths).header(header);
    let table_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    f.render_widget(table, table_area);

    // Hint bar
    let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
    f.render_widget(
        Paragraph::new("Enter: Details  d: Delete  D: Clear all  Esc: Close")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center),
        hint_area,
    );
}

fn render_detail(f: &mut Frame, detail: &crate::history::SessionDetail, popup_area: Rect) {
    let s = &detail.summary;
    let season_str = s.season.map(|n| format!("S{:02}", n)).unwrap_or_default();
    let disc_str = s.disc_number.map(|n| format!("D{}", n)).unwrap_or_default();

    let mut title_parts = vec![s.title.as_str()];
    if !season_str.is_empty() {
        title_parts.push(&season_str);
    }
    if !disc_str.is_empty() {
        title_parts.push(&disc_str);
    }

    let block_title = format!(" Session #{} -- {} ", s.id, title_parts.join(" "));
    let block = Block::default().borders(Borders::ALL).title(block_title);
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("  Disc:     ", Style::default().fg(Color::DarkGray)),
        Span::raw(&s.volume_label),
    ]));

    if let Some(ref dev) = detail.device {
        lines.push(Line::from(vec![
            Span::styled("  Device:   ", Style::default().fg(Color::DarkGray)),
            Span::raw(dev.as_str()),
        ]));
    }

    let started_date = s.started_at.split('T').next().unwrap_or("");
    let started_time = s
        .started_at
        .split('T')
        .nth(1)
        .and_then(|t| t.split('.').next())
        .unwrap_or("");
    let finished_time = detail
        .finished_at
        .as_ref()
        .and_then(|f| f.split('T').nth(1))
        .and_then(|t| t.split('.').next())
        .unwrap_or("");

    let date_str = if !finished_time.is_empty() {
        format!("{} {} -> {}", started_date, started_time, finished_time)
    } else {
        format!("{} {}", started_date, started_time)
    };
    lines.push(Line::from(vec![
        Span::styled("  Date:     ", Style::default().fg(Color::DarkGray)),
        Span::raw(date_str),
    ]));

    if let (Some(tmdb_id), Some(ref tmdb_type)) = (detail.tmdb_id, &detail.tmdb_type) {
        lines.push(Line::from(vec![
            Span::styled("  TMDb:     ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}/{}", tmdb_type, tmdb_id)),
        ]));
    }

    let status_display = match s.display_status() {
        "partial" => format!(
            "partial ({}/{} playlists ripped)",
            s.files_completed, s.files_total
        ),
        other => other.to_string(),
    };
    let status_color = match s.display_status() {
        "completed" => Color::Green,
        "failed" => Color::Red,
        "partial" => Color::Yellow,
        "in_progress" => Color::Cyan,
        _ => Color::White,
    };
    lines.push(Line::from(vec![
        Span::styled("  Status:   ", Style::default().fg(Color::DarkGray)),
        Span::styled(status_display, Style::default().fg(status_color)),
    ]));

    let size_str = format_size(s.total_size);
    lines.push(Line::from(vec![
        Span::styled("  Size:     ", Style::default().fg(Color::DarkGray)),
        Span::raw(size_str),
    ]));

    lines.push(Line::raw(""));

    // Files section
    if !detail.files.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Files:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));

        for file in &detail.files {
            let symbol = match file.status {
                crate::history::FileStatus::Completed => "+",
                crate::history::FileStatus::Failed => "x",
                crate::history::FileStatus::Skipped => ".",
                crate::history::FileStatus::InProgress => "~",
            };

            let symbol_color = match file.status {
                crate::history::FileStatus::Completed => Color::Green,
                crate::history::FileStatus::Failed => Color::Red,
                crate::history::FileStatus::Skipped => Color::DarkGray,
                crate::history::FileStatus::InProgress => Color::Cyan,
            };

            let file_size = file
                .file_size
                .map(format_size)
                .unwrap_or_else(|| "-".to_string());

            let path_display = std::path::Path::new(&file.output_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| file.output_path.clone());

            let mut spans = vec![
                Span::raw("    "),
                Span::styled(symbol, Style::default().fg(symbol_color)),
                Span::raw(format!(
                    " {:<30} {:>8}",
                    truncate(&path_display, 30),
                    file_size
                )),
            ];

            if file.status == crate::history::FileStatus::Failed {
                if let Some(ref err) = file.error {
                    spans.push(Span::styled(
                        format!("  {}", truncate(err, 30)),
                        Style::default().fg(Color::Red),
                    ));
                }
            }

            if let Some(ref verified) = file.verified {
                let v_color = if verified == "passed" {
                    Color::Green
                } else {
                    Color::Yellow
                };
                spans.push(Span::styled(
                    format!("  {}", verified),
                    Style::default().fg(v_color),
                ));
            }

            lines.push(Line::from(spans));
        }
    }

    // Render all lines as a paragraph (scrollable via max height)
    let content = Paragraph::new(lines);
    let content_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    f.render_widget(content, content_area);

    // Hint bar
    let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
    f.render_widget(
        Paragraph::new("Esc: Back to list")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center),
        hint_area,
    );
}

fn render_confirm(f: &mut Frame, action: &crate::types::HistoryConfirmAction, popup_area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" History ");
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let msg = match action {
        crate::types::HistoryConfirmAction::Delete(id) => {
            format!("Delete session #{}?", id)
        }
        crate::types::HistoryConfirmAction::ClearAll => "Clear ALL history?".to_string(),
    };

    let prompt_area = Rect::new(inner.x, inner.y + inner.height / 2, inner.width, 1);
    f.render_widget(
        Paragraph::new(msg)
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center),
        prompt_area,
    );

    let hint_area = Rect::new(inner.x, inner.y + inner.height / 2 + 2, inner.width, 1);
    f.render_widget(
        Paragraph::new("[y] Yes  [n] No")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center),
        hint_area,
    );
}

pub fn handle_input(
    state: &mut HistoryOverlayState,
    key: KeyEvent,
    db: &HistoryDb,
) -> HistoryAction {
    // Confirm dialog takes priority
    if state.confirm_action.is_some() {
        return handle_confirm_input(state, key, db);
    }

    // Detail view
    if state.detail_view.is_some() {
        return handle_detail_input(state, key);
    }

    // List navigation
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if state.selected > 0 {
                state.selected -= 1;
            }
            HistoryAction::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if !state.sessions.is_empty() && state.selected < state.sessions.len() - 1 {
                state.selected += 1;
            }
            HistoryAction::None
        }
        KeyCode::Enter => {
            if let Some(session) = state.sessions.get(state.selected) {
                let detail = db.get_session(session.id).ok().flatten();
                if let Some(d) = detail {
                    state.detail_view = Some(d);
                }
            }
            HistoryAction::None
        }
        KeyCode::Char('d') => {
            if let Some(session) = state.sessions.get(state.selected) {
                state.confirm_action = Some(crate::types::HistoryConfirmAction::Delete(session.id));
            }
            HistoryAction::None
        }
        KeyCode::Char('D') => {
            if !state.sessions.is_empty() {
                state.confirm_action = Some(crate::types::HistoryConfirmAction::ClearAll);
            }
            HistoryAction::None
        }
        KeyCode::Esc => HistoryAction::Close,
        _ => HistoryAction::None,
    }
}

fn handle_detail_input(state: &mut HistoryOverlayState, key: KeyEvent) -> HistoryAction {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            state.detail_view = None;
            HistoryAction::None
        }
        _ => HistoryAction::None,
    }
}

fn handle_confirm_input(
    state: &mut HistoryOverlayState,
    key: KeyEvent,
    db: &HistoryDb,
) -> HistoryAction {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let action = state.confirm_action.take();
            match action {
                Some(crate::types::HistoryConfirmAction::Delete(id)) => {
                    let _ = db.delete_session(id);
                }
                Some(crate::types::HistoryConfirmAction::ClearAll) => {
                    let _ = db.clear_all();
                }
                None => {}
            }
            HistoryAction::Refresh
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            state.confirm_action = None;
            HistoryAction::None
        }
        _ => HistoryAction::None,
    }
}

use crate::util::{format_size_human as format_size, truncate};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{HistoryDb, SessionFilter, SessionInfo, SessionStatus};
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn make_test_state(db: &HistoryDb) -> HistoryOverlayState {
        let filter = SessionFilter {
            limit: Some(50),
            ..Default::default()
        };
        let sessions = db.list_sessions(&filter).unwrap_or_default();
        HistoryOverlayState {
            sessions,
            selected: 0,
            filter_text: String::new(),
            status_filter: None,
            detail_view: None,
            confirm_action: None,
        }
    }

    fn make_session_info(label: &str, title: &str) -> SessionInfo {
        SessionInfo {
            volume_label: label.to_string(),
            device: None,
            tmdb_id: None,
            tmdb_type: None,
            title: title.to_string(),
            season: None,
            disc_number: None,
            batch_id: None,
            config_snapshot: None,
        }
    }

    #[test]
    fn test_empty_state_esc_closes() {
        let db = HistoryDb::open_memory().unwrap();
        let mut state = make_test_state(&db);
        let action = handle_input(&mut state, key(KeyCode::Esc), &db);
        assert!(matches!(action, HistoryAction::Close));
    }

    #[test]
    fn test_navigate_down_and_up() {
        let db = HistoryDb::open_memory().unwrap();
        db.start_session(&make_session_info("D1", "Show A"))
            .unwrap();
        db.start_session(&make_session_info("D2", "Show B"))
            .unwrap();
        let mut state = make_test_state(&db);
        assert_eq!(state.sessions.len(), 2);
        assert_eq!(state.selected, 0);

        handle_input(&mut state, key(KeyCode::Down), &db);
        assert_eq!(state.selected, 1);

        // Can't go past end
        handle_input(&mut state, key(KeyCode::Down), &db);
        assert_eq!(state.selected, 1);

        handle_input(&mut state, key(KeyCode::Up), &db);
        assert_eq!(state.selected, 0);

        // Can't go past start
        handle_input(&mut state, key(KeyCode::Up), &db);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_vim_navigation() {
        let db = HistoryDb::open_memory().unwrap();
        db.start_session(&make_session_info("D1", "Show A"))
            .unwrap();
        db.start_session(&make_session_info("D2", "Show B"))
            .unwrap();
        let mut state = make_test_state(&db);

        handle_input(&mut state, key(KeyCode::Char('j')), &db);
        assert_eq!(state.selected, 1);

        handle_input(&mut state, key(KeyCode::Char('k')), &db);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_enter_opens_detail_view() {
        let db = HistoryDb::open_memory().unwrap();
        let sid = db
            .start_session(&make_session_info("D1", "Show A"))
            .unwrap();
        db.finish_session(sid, SessionStatus::Completed).unwrap();
        let mut state = make_test_state(&db);

        handle_input(&mut state, key(KeyCode::Enter), &db);
        assert!(state.detail_view.is_some());
    }

    #[test]
    fn test_esc_from_detail_returns_to_list() {
        let db = HistoryDb::open_memory().unwrap();
        let sid = db
            .start_session(&make_session_info("D1", "Show A"))
            .unwrap();
        db.finish_session(sid, SessionStatus::Completed).unwrap();
        let mut state = make_test_state(&db);

        handle_input(&mut state, key(KeyCode::Enter), &db);
        assert!(state.detail_view.is_some());

        handle_input(&mut state, key(KeyCode::Esc), &db);
        assert!(state.detail_view.is_none());
    }

    #[test]
    fn test_delete_shows_confirm() {
        let db = HistoryDb::open_memory().unwrap();
        db.start_session(&make_session_info("D1", "Show A"))
            .unwrap();
        let mut state = make_test_state(&db);

        handle_input(&mut state, key(KeyCode::Char('d')), &db);
        assert!(matches!(
            state.confirm_action,
            Some(crate::types::HistoryConfirmAction::Delete(_))
        ));
    }

    #[test]
    fn test_confirm_yes_deletes_and_refreshes() {
        let db = HistoryDb::open_memory().unwrap();
        let sid = db
            .start_session(&make_session_info("D1", "Show A"))
            .unwrap();
        let mut state = make_test_state(&db);

        state.confirm_action = Some(crate::types::HistoryConfirmAction::Delete(sid));

        let action = handle_input(&mut state, key(KeyCode::Char('y')), &db);
        assert!(matches!(action, HistoryAction::Refresh));
        assert!(state.confirm_action.is_none());

        // Verify deletion
        let sessions = db.list_sessions(&SessionFilter::default()).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_confirm_no_cancels() {
        let db = HistoryDb::open_memory().unwrap();
        let sid = db
            .start_session(&make_session_info("D1", "Show A"))
            .unwrap();
        let mut state = make_test_state(&db);

        state.confirm_action = Some(crate::types::HistoryConfirmAction::Delete(sid));

        let action = handle_input(&mut state, key(KeyCode::Char('n')), &db);
        assert!(matches!(action, HistoryAction::None));
        assert!(state.confirm_action.is_none());

        // Session should still exist
        let sessions = db.list_sessions(&SessionFilter::default()).unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn test_clear_all_confirm_flow() {
        let db = HistoryDb::open_memory().unwrap();
        db.start_session(&make_session_info("D1", "A")).unwrap();
        db.start_session(&make_session_info("D2", "B")).unwrap();
        let mut state = make_test_state(&db);

        handle_input(&mut state, key(KeyCode::Char('D')), &db);
        assert!(matches!(
            state.confirm_action,
            Some(crate::types::HistoryConfirmAction::ClearAll)
        ));

        let action = handle_input(&mut state, key(KeyCode::Char('y')), &db);
        assert!(matches!(action, HistoryAction::Refresh));

        let sessions = db.list_sessions(&SessionFilter::default()).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_delete_on_empty_is_noop() {
        let db = HistoryDb::open_memory().unwrap();
        let mut state = make_test_state(&db);
        assert!(state.sessions.is_empty());

        handle_input(&mut state, key(KeyCode::Char('d')), &db);
        assert!(state.confirm_action.is_none());
    }

    #[test]
    fn test_clear_on_empty_is_noop() {
        let db = HistoryDb::open_memory().unwrap();
        let mut state = make_test_state(&db);
        assert!(state.sessions.is_empty());

        handle_input(&mut state, key(KeyCode::Char('D')), &db);
        assert!(state.confirm_action.is_none());
    }

    #[test]
    fn test_format_size_zero() {
        assert_eq!(format_size(0), "\u{2014}");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(5_368_709_120), "5.0 GB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(10_485_760), "10.0 MB");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024), "1 KB");
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }
}
