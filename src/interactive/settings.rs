use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::cli::{SessionMode, ZenohConnection};

pub const PROFILE: &str = "zenohui-profile.json";

pub struct Settings {
    pub active: ZenohConnection,
    pub mode: SessionMode,
    pub peers: String,
    pub listeners: String,
    pub multicast: bool,
    pub selected: usize,
    pub editing: bool,
    pub message: String,
}

impl Settings {
    pub fn new(connection: &ZenohConnection) -> Self {
        Self {
            active: connection.clone(),
            mode: connection.mode,
            peers: connection.peer.join(", "),
            listeners: connection.listen.join(", "),
            multicast: !connection.no_multicast,
            selected: 0,
            editing: false,
            message: "Edit your connection, then press F5 to apply.".into(),
        }
    }

    pub fn connection(&self) -> ZenohConnection {
        let split = |text: &str| {
            text.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        };
        ZenohConnection {
            mode: self.mode,
            peer: split(&self.peers),
            listen: split(&self.listeners),
            no_multicast: !self.multicast,
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let value = serde_json::json!({"mode": self.mode.as_str(), "peers": self.peers, "listeners": self.listeners, "multicast": self.multicast});
        std::fs::write(PROFILE, serde_json::to_string_pretty(&value)?)?;
        Ok(())
    }

    pub fn load(&mut self) -> anyhow::Result<()> {
        self.load_json(&std::fs::read_to_string(PROFILE)?)
    }

    fn load_json(&mut self, json: &str) -> anyhow::Result<()> {
        let value: serde_json::Value = serde_json::from_str(json)?;
        let mode = match value["mode"].as_str() {
            Some("client") => SessionMode::Client,
            Some("peer") => SessionMode::Peer,
            Some("router") => SessionMode::Router,
            _ => anyhow::bail!("Invalid profile mode"),
        };
        let peers = value["peers"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing peers"))?
            .to_owned();
        let listeners = value["listeners"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing listeners"))?
            .to_owned();
        let multicast = value["multicast"]
            .as_bool()
            .ok_or_else(|| anyhow::anyhow!("Missing multicast setting"))?;
        self.mode = mode;
        self.peers = peers;
        self.listeners = listeners;
        self.multicast = multicast;
        Ok(())
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if self.editing {
            let field = if self.selected == 1 {
                &mut self.peers
            } else {
                &mut self.listeners
            };
            match key.code {
                KeyCode::Enter | KeyCode::Esc => self.editing = false,
                KeyCode::Backspace => {
                    field.pop();
                }
                KeyCode::Char(c) => field.push(c),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Down | KeyCode::Tab => self.selected = (self.selected + 1) % 4,
            KeyCode::Up | KeyCode::BackTab => self.selected = (self.selected + 3) % 4,
            KeyCode::Enter | KeyCode::Char(' ') => match self.selected {
                0 => {
                    self.mode = match self.mode {
                        SessionMode::Peer => SessionMode::Client,
                        SessionMode::Client => SessionMode::Router,
                        SessionMode::Router => SessionMode::Peer,
                    }
                }
                1 | 2 => self.editing = true,
                _ => self.multicast = !self.multicast,
            },
            KeyCode::F(2) => {
                self.message = match self.save() {
                    Ok(()) => format!("Saved {PROFILE}"),
                    Err(e) => format!("Save failed: {e}"),
                }
            }
            KeyCode::F(3) => {
                self.message = match self.load() {
                    Ok(()) => "Profile loaded. Press F5 to connect.".into(),
                    Err(e) => format!("Load failed: {e}"),
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn draw(&self, frame: &mut Frame, area: Rect, busy: bool) {
        let panels = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(if area.width >= 100 {
                vec![Constraint::Percentage(65), Constraint::Percentage(35)]
            } else {
                vec![Constraint::Percentage(100), Constraint::Length(0)]
            })
            .split(area);
        let changed = self.connection() != self.active;
        let fields = [
            format!("Mode                 {}", self.mode.as_str()),
            format!("Connect endpoints    {}", self.peers),
            format!("Listen endpoints     {}", self.listeners),
            format!(
                "Multicast discovery  {}",
                if self.multicast { "On" } else { "Off" }
            ),
        ];
        let mut lines = vec![
            Line::from(""),
            Line::styled(
                "  CONNECTION SETTINGS",
                Style::default().fg(Color::LightCyan),
            ),
            Line::from(""),
        ];
        for (index, field) in fields.iter().enumerate() {
            let selected = index == self.selected;
            lines.push(Line::styled(
                format!(
                    "  {} {field}{}",
                    if selected { "›" } else { " " },
                    if selected && self.editing { " ▏" } else { "" }
                ),
                if selected {
                    Style::default().fg(Color::Black).bg(Color::LightGreen)
                } else {
                    Style::default()
                },
            ));
            lines.push(Line::from(""));
        }
        lines.extend([
            Line::from("  Enter: edit / toggle    F5: apply & reconnect"),
            Line::from("  F2: save profile       F3: load profile"),
            Line::from(""),
            Line::from("  Endpoints separated by commas; empty = Zenoh defaults."),
            Line::from("  Example: tcp/router.example.org:7447"),
            Line::from("  Profiles are stored in zenohui-profile.json in the working directory."),
            Line::from("  Applying a connection starts a fresh topic history."),
            Line::from(""),
            Line::styled(
                format!(
                    "  {}",
                    if busy {
                        "Connecting… your current session remains active."
                    } else {
                        &self.message
                    }
                ),
                Style::default().fg(Color::LightCyan),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Settings · connection profiles"),
                )
                .wrap(Wrap { trim: false }),
            panels[0],
        );
        if panels[1].width > 0 {
            let summary = vec![
                Line::from(""),
                Line::styled(
                    if busy {
                        "  ● CONNECTING"
                    } else if changed {
                        "  ● PENDING CONNECTION CHANGES"
                    } else {
                        "  ● ACTIVE CONNECTION"
                    },
                    Style::default().fg(if changed {
                        Color::Yellow
                    } else {
                        Color::LightGreen
                    }),
                ),
                Line::from(""),
                Line::from(format!("  Mode: {}", self.active.mode.as_str())),
                Line::from(format!(
                    "  Targets: {}",
                    if self.active.peer.is_empty() {
                        "Automatic / default".into()
                    } else {
                        self.active.peer.join(", ")
                    }
                )),
                Line::from(format!(
                    "  Discovery: {}",
                    if self.active.no_multicast {
                        "Multicast off"
                    } else {
                        "Multicast on"
                    }
                )),
                Line::from(""),
                Line::from("  LOCAL + REMOTE"),
                Line::from(
                    "  Peer mode keeps your local network visible while connecting to a remote router.",
                ),
                Line::from(""),
                Line::from("  Router status appears in tab 2 as soon as the connection succeeds."),
            ];
            frame.render_widget(
                Paragraph::new(summary)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Session · live"),
                    )
                    .wrap(Wrap { trim: false }),
                panels[1],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;

    use super::*;

    fn settings() -> Settings {
        Settings::new(&ZenohConnection {
            mode: SessionMode::Peer,
            peer: vec![],
            listen: vec![],
            no_multicast: false,
        })
    }

    #[test]
    fn endpoint_editing_accepts_tab_shortcuts_and_unicode() {
        let mut form = settings();
        form.selected = 1;
        for code in [
            KeyCode::Enter,
            KeyCode::Char('4'),
            KeyCode::Char('q'),
            KeyCode::Char('ü'),
            KeyCode::Backspace,
            KeyCode::Esc,
        ] {
            form.on_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        assert_eq!(form.peers, "4q");
        assert!(!form.editing);
    }

    #[test]
    fn endpoints_are_trimmed_and_empty_items_ignored() {
        let mut form = settings();
        form.peers = " tcp/localhost:7447, ,tcp/localhost:7448 ".into();
        assert_eq!(
            form.connection().peer,
            ["tcp/localhost:7447", "tcp/localhost:7448"]
        );
    }

    #[test]
    fn invalid_profile_leaves_current_form_unchanged() {
        let mut form = settings();
        form.peers = "tcp/original:7447".into();
        let before = form.connection();
        assert!(
            form.load_json(r#"{"mode":"client","peers":"tcp/new:7447","listeners":""}"#)
                .is_err()
        );
        assert_eq!(form.connection(), before);
        form.load_json(
            r#"{"mode":"client","peers":"tcp/new:7447","listeners":"","multicast":false}"#,
        )
        .unwrap();
        assert_eq!(form.mode, SessionMode::Client);
        assert_eq!(form.connection().peer, ["tcp/new:7447"]);
        assert!(form.connection().no_multicast);
    }

    #[test]
    fn settings_render_in_small_and_large_terminals() {
        for (width, height) in [(30, 8), (100, 30)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| settings().draw(frame, frame.size(), false))
                .unwrap();
        }
    }
}
