use ratatui::prelude::*;
use ratatui::widgets::Tabs;

use crate::types::{TabState, TabSummary};

/// Render the tab bar at the top of the screen.
/// Returns the remaining area below the tab bar for content.
pub fn render(f: &mut Frame, tabs: &[TabSummary], active_index: usize, area: Rect) -> Rect {
    if tabs.len() <= 1 {
        return area;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let titles: Vec<Line> = tabs
        .iter()
        .map(|tab| {
            let label = format_tab_label(tab);
            Line::from(label)
        })
        .collect();

    let tab_widget = Tabs::new(titles)
        .select(active_index)
        .highlight_style(Style::default().bold().fg(Color::Cyan))
        .divider(" │ ");

    f.render_widget(tab_widget, chunks[0]);

    chunks[1]
}

fn format_tab_label(tab: &TabSummary) -> String {
    match tab.state {
        TabState::Idle => format!("{}: Waiting for disc", tab.device_name),
        TabState::Scanning => format!("{}: Scanning", tab.device_name),
        TabState::Wizard => format!("{}: Setup", tab.device_name),
        TabState::Ripping => {
            if let Some((current, total, pct)) = tab.rip_progress {
                format!(
                    "{}: Ripping {}/{} {}%",
                    tab.device_name, current, total, pct
                )
            } else {
                format!("{}: Ripping", tab.device_name)
            }
        }
        TabState::Done => format!("{}: Done", tab.device_name),
        TabState::Error => {
            let err = tab.error.as_deref().unwrap_or("error");
            format!("{}: {}", tab.device_name, err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionId;

    #[test]
    fn test_format_tab_label_idle() {
        let tab = TabSummary {
            session_id: SessionId(1),
            device_name: "sr0".into(),
            state: TabState::Idle,
            rip_progress: None,
            error: None,
        };
        assert_eq!(format_tab_label(&tab), "sr0: Waiting for disc");
    }

    #[test]
    fn test_format_tab_label_ripping() {
        let tab = TabSummary {
            session_id: SessionId(1),
            device_name: "sr0".into(),
            state: TabState::Ripping,
            rip_progress: Some((3, 8, 42)),
            error: None,
        };
        assert_eq!(format_tab_label(&tab), "sr0: Ripping 3/8 42%");
    }

    #[test]
    fn test_format_tab_label_error() {
        let tab = TabSummary {
            session_id: SessionId(1),
            device_name: "sr1".into(),
            state: TabState::Error,
            rip_progress: None,
            error: Some("drive disconnected".into()),
        };
        assert_eq!(format_tab_label(&tab), "sr1: drive disconnected");
    }

    #[test]
    fn test_format_tab_label_scanning() {
        let tab = TabSummary {
            session_id: SessionId(1),
            device_name: "sr0".into(),
            state: TabState::Scanning,
            rip_progress: None,
            error: None,
        };
        assert_eq!(format_tab_label(&tab), "sr0: Scanning");
    }

    #[test]
    fn test_format_tab_label_done() {
        let tab = TabSummary {
            session_id: SessionId(1),
            device_name: "sr0".into(),
            state: TabState::Done,
            rip_progress: None,
            error: None,
        };
        assert_eq!(format_tab_label(&tab), "sr0: Done");
    }

    #[test]
    fn test_format_tab_label_ripping_no_progress() {
        let tab = TabSummary {
            session_id: SessionId(1),
            device_name: "sr0".into(),
            state: TabState::Ripping,
            rip_progress: None,
            error: None,
        };
        assert_eq!(format_tab_label(&tab), "sr0: Ripping");
    }
}
