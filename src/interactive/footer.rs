use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::interactive::{App, ElementInFocus};
use crate::zenoh_client::SessionInfo;

const VERSION_TEXT: &str = concat!(" zenohui ", env!("CARGO_PKG_VERSION"), " ");
const VERSION_STYLE: Style = Style::new().fg(Color::Black).bg(Color::Gray);
const KEY_STYLE: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Gray)
    .add_modifier(Modifier::BOLD);

pub struct Footer {
    session: Box<str>,
    full_info: Box<str>,
}

impl Footer {
    pub fn new(session_info: &SessionInfo) -> Self {
        Self {
            session: format!(" {} ", session_info.description).into(),
            full_info: format!("{VERSION_TEXT}@ {} ", session_info.description).into(),
        }
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, app: &App) {
        let mut keys = Vec::new();

        macro_rules! add {
            ($key:literal, $text:literal) => {
                keys.push(Span {
                    content: std::borrow::Cow::Borrowed(concat![" ", $key, " "]),
                    style: KEY_STYLE,
                });
                keys.push(Span {
                    content: std::borrow::Cow::Borrowed(concat![" ", $text, " "]),
                    style: Style::new(),
                });
            };
        }

        match app.focus {
            ElementInFocus::TopicOverview => {
                add!("q", "Quit");
                add!("/", "Search");
                add!("o", "Open all");
                if !app.topic_overview.state.opened().is_empty() {
                    add!("O", "Close all");
                }
                if app.topic_overview.get_selected().is_some() {
                    add!("Del", "Delete keys");
                }
                if app.can_switch_to_payload() {
                    add!("Tab", "Switch to Payload");
                } else if app.can_switch_to_history_table() {
                    add!("Tab", "Switch to History");
                } else {
                    // Changing somewhere is pointless currently
                }
            }
            ElementInFocus::TopicSearch => {
                add!("↑", "Before");
                add!("↓", "Next");
                add!("Enter", "Open All");
                add!("Esc", "Clear");
                keys.push(Span::styled(
                    " Search: ",
                    Style::new()
                        .fg(Color::Black)
                        .bg(Color::LightGreen)
                        .add_modifier(Modifier::BOLD),
                ));
                keys.push(Span::raw(" "));
                keys.push(Span::raw(&app.topic_overview.search));
            }
            ElementInFocus::Payload => {
                add!("q", "Quit");
                #[allow(clippy::branches_sharing_code)]
                if app.can_switch_to_history_table() {
                    add!("Tab", "Switch to History");
                } else {
                    add!("Tab", "Switch to Topics");
                }
            }
            ElementInFocus::HistoryTable => {
                add!("q", "Quit");
                add!("Tab", "Switch to Topics");
            }
            ElementInFocus::CleanPopup(_) => {
                add!("Enter", "Delete key tree");
                add!("Any", "Abort");
            }
        }
        let keys = Line::from(keys);

        #[allow(clippy::cast_possible_truncation)]
        if matches!(app.focus, ElementInFocus::TopicSearch) {
            let x = area.left().saturating_add(keys.width() as u16);
            frame.set_cursor(x, area.y);
        }

        // Show version / session info when enough space
        {
            let remaining = (area.width as usize).saturating_sub(keys.width());
            let text = if remaining > self.full_info.len() {
                Some(&*self.full_info)
            } else if remaining > self.session.len() {
                Some(&*self.session)
            } else if remaining > VERSION_TEXT.len() {
                Some(VERSION_TEXT)
            } else {
                None // Not enough space -> show nothing
            };
            if let Some(text) = text {
                #[allow(clippy::cast_possible_truncation)]
                let area = Rect {
                    x: area.width.saturating_sub(text.len() as u16),
                    y: area.y,
                    width: text.len() as u16,
                    height: 1,
                };
                frame.render_widget(Paragraph::new(text).style(VERSION_STYLE), area);
            }
        }
    }
}
