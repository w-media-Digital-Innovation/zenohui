use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Tabs;
use ratatui::{Frame, Terminal};
use zenoh::Session;

use self::ui::ElementInFocus;
use crate::payload::Payload;
use crate::zenoh_client::SessionInfo;

mod clean;
mod connection_error_widget;
mod details;
mod footer;
mod scouting;
mod settings;
mod topic_overview;
mod ui;
mod zenoh_history;
mod zenoh_thread;

enum Refresh {
    /// Update the TUI
    Update,
    /// Skip the update of the TUI
    Skip,
    /// Quit the TUI and return to the shell
    Quit,
}

#[derive(Clone, Copy)]
enum SearchSelection {
    Before,
    Stay,
    After,
}

#[derive(Clone, Copy)]
enum ScrollDirection {
    Up,
    Down,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveView {
    Topics,
    Routers,
    Peers,
    Settings,
}

impl ActiveView {
    const fn index(self) -> usize {
        match self {
            Self::Topics => 0,
            Self::Routers => 1,
            Self::Peers => 2,
            Self::Settings => 3,
        }
    }

    const fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Routers,
            2 => Self::Peers,
            3 => Self::Settings,
            _ => Self::Topics,
        }
    }

    const fn next(self) -> Self {
        Self::from_index((self.index() + 1) % 4)
    }

    const fn prev(self) -> Self {
        Self::from_index((self.index() + 3) % 4)
    }
}

fn reset_terminal() -> anyhow::Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show
    )?;
    Ok(())
}

pub fn show(
    session: Arc<Session>,
    session_info: &SessionInfo,
    subscribe_keyexpr: Vec<String>,
    payload_size_limit: usize,
) -> anyhow::Result<()> {
    let zenoh_thread = zenoh_thread::ZenohThread::new(
        session,
        &subscribe_keyexpr,
        payload_size_limit,
        session_info.config.clone(),
    )?;
    let mut app = App::new(session_info, zenoh_thread);
    app.subscribe_keyexpr = subscribe_keyexpr;
    app.payload_size_limit = payload_size_limit;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        reset_terminal().unwrap();
        original_hook(panic);
    }));

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
        crossterm::cursor::Hide
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    terminal.clear()?;

    let main_loop_result = main_loop(app, terminal);

    reset_terminal()?;

    main_loop_result
}

fn main_loop<B>(mut app: App, mut terminal: Terminal<B>) -> anyhow::Result<()>
where
    B: Backend,
{
    const INTERVAL: Duration = Duration::from_millis(500);
    const DEBOUNCE: Duration = Duration::from_millis(20); // 50 FPS

    terminal.draw(|frame| app.draw(frame))?;

    let mut last_render = Instant::now();
    let mut debounce: Option<Instant> = None;

    loop {
        let timeout = debounce.map_or(INTERVAL, |start| DEBOUNCE.saturating_sub(start.elapsed()));
        if crossterm::event::poll(timeout)? {
            let refresh = match crossterm::event::read()? {
                Event::Key(key) if !matches!(key.kind, KeyEventKind::Press) => Refresh::Skip,
                Event::Key(key) => app.on_key(key)?,
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        app.on_click(mouse.column, mouse.row)
                    }
                    MouseEventKind::ScrollDown => {
                        app.on_scroll(ScrollDirection::Down, mouse.column, mouse.row)
                    }
                    MouseEventKind::ScrollUp => {
                        app.on_scroll(ScrollDirection::Up, mouse.column, mouse.row)
                    }
                    _ => Refresh::Skip,
                },
                Event::Resize(_, _) => Refresh::Update,
                Event::FocusGained | Event::FocusLost | Event::Paste(_) => Refresh::Skip,
            };
            match refresh {
                Refresh::Quit => return Ok(()),
                Refresh::Skip => {}
                Refresh::Update => {
                    debounce.get_or_insert_with(Instant::now);
                }
            }
        }
        if debounce.map_or_else(
            || last_render.elapsed() > INTERVAL,
            |debounce| debounce.elapsed() > DEBOUNCE,
        ) {
            terminal.draw(|frame| app.draw(frame))?;
            last_render = Instant::now();
            debounce = None;
        }
    }
}

pub struct App {
    settings: settings::Settings,
    reconnect:
        Option<std::sync::mpsc::Receiver<anyhow::Result<(SessionInfo, zenoh_thread::ZenohThread)>>>,
    subscribe_keyexpr: Vec<String>,
    payload_size_limit: usize,
    details: details::Details,
    focus: ElementInFocus,
    footer: footer::Footer,
    active_view: ActiveView,
    scout_tables: scouting::ScoutTables,
    zenoh_thread: zenoh_thread::ZenohThread,
    topic_overview: topic_overview::TopicOverview,
}

impl App {
    fn new(session_info: &SessionInfo, zenoh_thread: zenoh_thread::ZenohThread) -> Self {
        Self {
            settings: settings::Settings::new(&session_info.connection),
            reconnect: None,
            subscribe_keyexpr: vec!["**".into()],
            payload_size_limit: 8000,
            details: details::Details::default(),
            focus: ElementInFocus::TopicOverview,
            footer: footer::Footer::new(session_info),
            active_view: ActiveView::Topics,
            scout_tables: scouting::ScoutTables::default(),
            zenoh_thread,
            topic_overview: topic_overview::TopicOverview::default(),
        }
    }

    fn can_switch_to_history_table(&self) -> bool {
        let Some(topic) = self.topic_overview.get_selected() else {
            return false;
        };
        self.zenoh_thread.get_history().get(&topic).is_some()
    }

    fn can_switch_to_payload(&self) -> bool {
        let Some(topic) = self.topic_overview.get_selected() else {
            return false;
        };
        self.zenoh_thread
            .get_history()
            .get(&topic)
            .and_then(|entries| {
                let index = self.details.selected_history_index(entries.len());
                entries.get(index)
            })
            .is_some_and(|entry| {
                matches!(
                    entry.payload,
                    Payload::Binary(_) | Payload::Json(_) | Payload::MessagePack(_)
                )
            })
    }

    fn set_active_view(&mut self, view: ActiveView) -> bool {
        if self.active_view == view {
            return false;
        }
        self.active_view = view;
        self.focus = match view {
            ActiveView::Topics => ElementInFocus::TopicOverview,
            ActiveView::Routers => ElementInFocus::RoutersTable,
            ActiveView::Peers => ElementInFocus::PeersTable,
            ActiveView::Settings => ElementInFocus::Settings,
        };
        true
    }

    fn table_select_next(state: &mut ratatui::widgets::TableState, row_count: usize) -> bool {
        if row_count == 0 {
            state.select(None);
            return false;
        }
        let next = state.selected().map_or(0, |selected| {
            (selected + 1).min(row_count.saturating_sub(1))
        });
        let changed = state.selected() != Some(next);
        state.select(Some(next));
        changed
    }

    fn table_select_prev(state: &mut ratatui::widgets::TableState, row_count: usize) -> bool {
        if row_count == 0 {
            state.select(None);
            return false;
        }
        let prev = state
            .selected()
            .map_or(0, |selected| selected.saturating_sub(1));
        let changed = state.selected() != Some(prev);
        state.select(Some(prev));
        changed
    }

    fn table_select_first(state: &mut ratatui::widgets::TableState, row_count: usize) -> bool {
        if row_count == 0 {
            state.select(None);
            return false;
        }
        let changed = state.selected() != Some(0);
        state.select(Some(0));
        changed
    }

    fn table_select_last(state: &mut ratatui::widgets::TableState, row_count: usize) -> bool {
        if row_count == 0 {
            state.select(None);
            return false;
        }
        let last = row_count.saturating_sub(1);
        let changed = state.selected() != Some(last);
        state.select(Some(last));
        changed
    }

    fn table_scroll_by(
        state: &mut ratatui::widgets::TableState,
        row_count: usize,
        offset: isize,
    ) -> bool {
        if row_count == 0 {
            state.select(None);
            return false;
        }
        let next = state
            .selected()
            .unwrap_or(0)
            .saturating_add_signed(offset)
            .min(row_count.saturating_sub(1));
        let changed = state.selected() != Some(next);
        state.select(Some(next));
        changed
    }

    /// On current topic with the current history table index
    fn get_selected_payload(&self) -> Option<Payload> {
        let topic = self.topic_overview.get_selected()?;
        self.zenoh_thread
            .get_history()
            .get(&topic)
            .and_then(|entries| {
                let index = self.details.selected_history_index(entries.len());
                entries.get(index)
            })
            .map(|entry| entry.payload.clone())
    }

    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn on_key(&mut self, key: KeyEvent) -> anyhow::Result<Refresh> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(Refresh::Quit);
        }

        if self.active_view == ActiveView::Settings && self.settings.editing {
            self.settings.on_key(key);
            return Ok(Refresh::Update);
        }
        if !matches!(
            self.focus,
            ElementInFocus::TopicSearch | ElementInFocus::CleanPopup(_)
        ) {
            let view = match key.code {
                KeyCode::Char('1') if key.modifiers.is_empty() => Some(ActiveView::Topics),
                KeyCode::Char('2') if key.modifiers.is_empty() => Some(ActiveView::Routers),
                KeyCode::Char('3') if key.modifiers.is_empty() => Some(ActiveView::Peers),
                KeyCode::Char('4') if key.modifiers.is_empty() => Some(ActiveView::Settings),
                KeyCode::Char('[') => Some(self.active_view.prev()),
                KeyCode::Char(']') => Some(self.active_view.next()),
                _ => None,
            };
            if let Some(view) = view
                && self.set_active_view(view)
            {
                return Ok(Refresh::Update);
            }
        }

        let update = match &self.focus {
            ElementInFocus::TopicOverview => match key.code {
                KeyCode::Char('q') => return Ok(Refresh::Quit),
                KeyCode::Tab if self.can_switch_to_payload() => {
                    self.focus = ElementInFocus::Payload;
                    true
                }
                KeyCode::Tab | KeyCode::BackTab if self.can_switch_to_history_table() => {
                    self.focus = ElementInFocus::HistoryTable;
                    true
                }
                KeyCode::Char('/') => {
                    self.focus = ElementInFocus::TopicSearch;
                    true
                }
                KeyCode::Esc => self.topic_overview.state.select(vec![]),
                KeyCode::Enter | KeyCode::Char(' ') => self.topic_overview.state.toggle_selected(),
                KeyCode::Down | KeyCode::Char('j') => self.topic_overview.state.key_down(),
                KeyCode::Up | KeyCode::Char('k') => self.topic_overview.state.key_up(),
                KeyCode::Left | KeyCode::Char('h') => self.topic_overview.state.key_left(),
                KeyCode::Right | KeyCode::Char('l') => self.topic_overview.state.key_right(),
                KeyCode::Home => self.topic_overview.state.select_first(),
                KeyCode::End => self.topic_overview.state.select_last(),
                KeyCode::PageUp => {
                    let page_jump = (self.topic_overview.last_area.height / 3) as usize;
                    self.topic_overview.state.scroll_up(page_jump)
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let page_jump = (self.topic_overview.last_area.height / 3) as usize;
                    self.topic_overview.state.scroll_up(page_jump)
                }
                KeyCode::PageDown => {
                    let page_jump = (self.topic_overview.last_area.height / 3) as usize;
                    self.topic_overview.state.scroll_down(page_jump)
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    let page_jump = (self.topic_overview.last_area.height / 3) as usize;
                    self.topic_overview.state.scroll_down(page_jump)
                }
                KeyCode::Backspace | KeyCode::Delete => {
                    if let Some(topic) = self.topic_overview.get_selected() {
                        self.focus = ElementInFocus::CleanPopup(topic);
                        true
                    } else {
                        false
                    }
                }
                KeyCode::Char('o') => {
                    self.topic_overview.search.clear();
                    self.open_all_search_matches()
                }
                KeyCode::Char('O') => self.topic_overview.state.close_all(),
                _ => false,
            },
            ElementInFocus::TopicSearch => match key.code {
                KeyCode::Char(char) => {
                    self.topic_overview.search += &char.to_lowercase().to_string();
                    self.search_select(SearchSelection::Stay);
                    true // Render new because of search string change
                }
                KeyCode::Backspace => {
                    self.topic_overview.search.pop();
                    self.search_select(SearchSelection::Stay);
                    true // Render new because of search string change
                }
                KeyCode::Up => self.search_select(SearchSelection::Before),
                KeyCode::Down => self.search_select(SearchSelection::After),
                KeyCode::Enter => {
                    let selection_changed = self.search_select(SearchSelection::After);
                    self.topic_overview.state.close_all(); // Exclusively open search matches
                    let open_changed = self.open_all_search_matches();
                    selection_changed || open_changed
                }
                KeyCode::Esc => {
                    self.topic_overview.search = String::new();
                    self.focus = ElementInFocus::TopicOverview;
                    true
                }
                KeyCode::PageUp => {
                    let page_jump = (self.topic_overview.last_area.height / 3) as usize;
                    self.topic_overview.state.scroll_up(page_jump)
                }
                KeyCode::PageDown => {
                    let page_jump = (self.topic_overview.last_area.height / 3) as usize;
                    self.topic_overview.state.scroll_down(page_jump)
                }
                KeyCode::Tab => {
                    self.focus = ElementInFocus::TopicOverview;
                    true
                }
                _ => false,
            },
            ElementInFocus::Payload => {
                if key.code == KeyCode::Char('q') {
                    return Ok(Refresh::Quit);
                }
                if matches!(key.code, KeyCode::Tab) && self.can_switch_to_history_table() {
                    self.focus = ElementInFocus::HistoryTable;
                    return Ok(Refresh::Update);
                }
                if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
                    self.focus = ElementInFocus::TopicOverview;
                    return Ok(Refresh::Update);
                }
                match self.get_selected_payload() {
                    Some(Payload::Binary(_)) => match key.code {
                        KeyCode::Esc => self.details.payload.binary_state.select_address(None),
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.details.payload.binary_state.key_down()
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.details.payload.binary_state.key_up()
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            self.details.payload.binary_state.key_left()
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            self.details.payload.binary_state.key_right()
                        }
                        KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.details.payload.binary_state.select_address(Some(0))
                        }
                        KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => self
                            .details
                            .payload
                            .binary_state
                            .select_address(Some(usize::MAX)),
                        KeyCode::Home => self.details.payload.binary_state.select_first_in_row(),
                        KeyCode::End => self.details.payload.binary_state.select_last_in_row(),
                        KeyCode::PageUp => self.details.payload.binary_state.scroll_up(3),
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.details.payload.binary_state.scroll_up(3)
                        }
                        KeyCode::PageDown => self.details.payload.binary_state.scroll_down(3),
                        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.details.payload.binary_state.scroll_down(3)
                        }
                        _ => false,
                    },
                    Some(Payload::Json(_) | Payload::MessagePack(_)) => match key.code {
                        KeyCode::Esc => self.details.payload.json_state.select(vec![]),
                        KeyCode::Enter | KeyCode::Char(' ') => {
                            self.details.payload.json_state.toggle_selected()
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.details.payload.json_state.key_down()
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.details.payload.json_state.key_up()
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            self.details.payload.json_state.key_left()
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            self.details.payload.json_state.key_right()
                        }
                        KeyCode::Home => self.details.payload.json_state.select_first(),
                        KeyCode::End => self.details.payload.json_state.select_last(),
                        KeyCode::PageUp => self.details.payload.json_state.scroll_up(3),
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.details.payload.json_state.scroll_up(3)
                        }
                        KeyCode::PageDown => self.details.payload.json_state.scroll_down(3),
                        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.details.payload.json_state.scroll_down(3)
                        }
                        _ => false,
                    },
                    Some(Payload::String(_)) | None => false,
                }
            }
            ElementInFocus::HistoryTable => match key.code {
                KeyCode::Char('q') => return Ok(Refresh::Quit),
                KeyCode::BackTab if self.can_switch_to_payload() => {
                    self.focus = ElementInFocus::Payload;
                    true
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    self.focus = ElementInFocus::TopicOverview;
                    true
                }
                KeyCode::Esc => self.details.table_state.select(None),
                KeyCode::Down | KeyCode::Char('j') => self.details.table_state.select_next(),
                KeyCode::Up | KeyCode::Char('k') => self.details.table_state.select_previous(),
                KeyCode::Home => self.details.table_state.select_first(),
                KeyCode::End => self.details.table_state.select_last(),
                KeyCode::PageUp => self.details.table_state.scroll_up_by(3),
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.details.table_state.scroll_up_by(3)
                }
                KeyCode::PageDown => self.details.table_state.scroll_down_by(3),
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.details.table_state.scroll_down_by(3)
                }
                KeyCode::Delete | KeyCode::Backspace => {
                    // This is a more hidden feature as changing the cached history might be weird to understand.
                    // Delete keys != Remove from local cache.
                    // Therefore, its not visible in the footer.
                    if let Some(selection) = self.details.table_state.selected() {
                        let topic = self
                            .topic_overview
                            .get_selected()
                            .expect("Should have a selected topic when on history view");
                        self.zenoh_thread
                            .uncache_topic_entry(&topic, selection)
                            .is_some()
                    } else {
                        false
                    }
                }
                _ => false,
            },
            ElementInFocus::RoutersTable => {
                if key.code == KeyCode::Char('q') {
                    return Ok(Refresh::Quit);
                }
                let row_count = self.zenoh_thread.get_scouting().routers_len();
                let page_jump = isize::try_from(
                    (self.scout_tables.routers_area.height.saturating_sub(2) / 2).max(1),
                )
                .unwrap_or(isize::MAX);
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        Self::table_select_next(&mut self.scout_tables.routers_state, row_count)
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        Self::table_select_prev(&mut self.scout_tables.routers_state, row_count)
                    }
                    KeyCode::Home => {
                        Self::table_select_first(&mut self.scout_tables.routers_state, row_count)
                    }
                    KeyCode::End => {
                        Self::table_select_last(&mut self.scout_tables.routers_state, row_count)
                    }
                    KeyCode::PageUp => Self::table_scroll_by(
                        &mut self.scout_tables.routers_state,
                        row_count,
                        -(page_jump.max(1)),
                    ),
                    KeyCode::PageDown => Self::table_scroll_by(
                        &mut self.scout_tables.routers_state,
                        row_count,
                        page_jump.max(1),
                    ),
                    _ => false,
                }
            }
            ElementInFocus::PeersTable => {
                if key.code == KeyCode::Char('q') {
                    return Ok(Refresh::Quit);
                }
                let row_count = self.zenoh_thread.get_scouting().peers_len();
                let page_jump = isize::try_from(
                    (self.scout_tables.peers_area.height.saturating_sub(2) / 2).max(1),
                )
                .unwrap_or(isize::MAX);
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        Self::table_select_next(&mut self.scout_tables.peers_state, row_count)
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        Self::table_select_prev(&mut self.scout_tables.peers_state, row_count)
                    }
                    KeyCode::Home => {
                        Self::table_select_first(&mut self.scout_tables.peers_state, row_count)
                    }
                    KeyCode::End => {
                        Self::table_select_last(&mut self.scout_tables.peers_state, row_count)
                    }
                    KeyCode::PageUp => Self::table_scroll_by(
                        &mut self.scout_tables.peers_state,
                        row_count,
                        -(page_jump.max(1)),
                    ),
                    KeyCode::PageDown => Self::table_scroll_by(
                        &mut self.scout_tables.peers_state,
                        row_count,
                        page_jump.max(1),
                    ),
                    _ => false,
                }
            }
            ElementInFocus::Settings => {
                if key.code == KeyCode::Char('q') {
                    return Ok(Refresh::Quit);
                }
                if key.code == KeyCode::F(5) && self.reconnect.is_none() {
                    let connection = self.settings.connection();
                    let keys = self.subscribe_keyexpr.clone();
                    let limit = self.payload_size_limit;
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let result =
                            crate::zenoh_client::connect(connection).and_then(|(info, session)| {
                                let worker = zenoh_thread::ZenohThread::new(
                                    Arc::new(session),
                                    &keys,
                                    limit,
                                    info.config.clone(),
                                )?;
                                Ok((info, worker))
                            });
                        let _ = tx.send(result);
                    });
                    self.reconnect = Some(rx);
                } else {
                    self.settings.on_key(key);
                }
                true
            }
            ElementInFocus::CleanPopup(topic) => {
                if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
                    self.zenoh_thread.clean_below(topic)?;
                }
                self.focus = ElementInFocus::TopicOverview;
                true
            }
        };
        Ok(if update {
            Refresh::Update
        } else {
            Refresh::Skip
        })
    }

    fn on_scroll(&mut self, direction: ScrollDirection, column: u16, row: u16) -> Refresh {
        let position = Position { x: column, y: row };

        let changed = match self.active_view {
            ActiveView::Settings => false,
            ActiveView::Topics => {
                if self.topic_overview.last_area.contains(position) {
                    match direction {
                        ScrollDirection::Up => self.topic_overview.state.scroll_up(1),
                        ScrollDirection::Down => self.topic_overview.state.scroll_down(1),
                    }
                } else if self.details.payload.last_area.contains(position) {
                    match self.get_selected_payload() {
                        Some(Payload::Binary(_)) => {
                            let state = &mut self.details.payload.binary_state;
                            match direction {
                                ScrollDirection::Up => state.scroll_up(1),
                                ScrollDirection::Down => state.scroll_down(1),
                            }
                        }
                        Some(Payload::Json(_) | Payload::MessagePack(_)) => {
                            let state = &mut self.details.payload.json_state;
                            match direction {
                                ScrollDirection::Up => state.scroll_up(1),
                                ScrollDirection::Down => state.scroll_down(1),
                            }
                        }
                        Some(Payload::String(_)) | None => return Refresh::Skip,
                    }
                } else if self.details.last_table_area.contains(position) {
                    match direction {
                        ScrollDirection::Down => self.details.table_state.scroll_down_by(1),
                        ScrollDirection::Up => self.details.table_state.scroll_up_by(1),
                    }
                } else {
                    false
                }
            }
            ActiveView::Routers => {
                if self.scout_tables.routers_area.contains(position) {
                    let row_count = self.zenoh_thread.get_scouting().routers_len();
                    let offset = match direction {
                        ScrollDirection::Up => -1,
                        ScrollDirection::Down => 1,
                    };
                    Self::table_scroll_by(&mut self.scout_tables.routers_state, row_count, offset)
                } else {
                    false
                }
            }
            ActiveView::Peers => {
                if self.scout_tables.peers_area.contains(position) {
                    let row_count = self.zenoh_thread.get_scouting().peers_len();
                    let offset = match direction {
                        ScrollDirection::Up => -1,
                        ScrollDirection::Down => 1,
                    };
                    Self::table_scroll_by(&mut self.scout_tables.peers_state, row_count, offset)
                } else {
                    false
                }
            }
        };
        if changed {
            Refresh::Update
        } else {
            Refresh::Skip
        }
    }

    fn on_click(&mut self, column: u16, row: u16) -> Refresh {
        let position = Position::new(column, row);
        if row == 0 && !self.settings.editing {
            let view = match column {
                0..=8 => ActiveView::Topics,
                11..=22 => ActiveView::Routers,
                23..=32 => ActiveView::Peers,
                33..=42 => ActiveView::Settings,
                _ => return Refresh::Skip,
            };
            self.set_active_view(view);
            return Refresh::Update;
        }
        if self.active_view == ActiveView::Settings {
            return Refresh::Skip;
        }

        if matches!(self.active_view, ActiveView::Routers) {
            let row_count = self.zenoh_thread.get_scouting().routers_len();
            if let Some(index) =
                Self::table_row_at(position, self.scout_tables.routers_area, row_count)
            {
                self.scout_tables.routers_state.select(Some(index));
                self.focus = ElementInFocus::RoutersTable;
                return Refresh::Update;
            }
            return Refresh::Skip;
        }

        if matches!(self.active_view, ActiveView::Peers) {
            let row_count = self.zenoh_thread.get_scouting().peers_len();
            if let Some(index) =
                Self::table_row_at(position, self.scout_tables.peers_area, row_count)
            {
                self.scout_tables.peers_state.select(Some(index));
                self.focus = ElementInFocus::PeersTable;
                return Refresh::Update;
            }
            return Refresh::Skip;
        }

        if let Some(identifier) = self.topic_overview.state.rendered_at(position) {
            let is_already_selected = identifier == self.topic_overview.state.selected();
            if is_already_selected {
                // change focus or toggle, don't do both
                if matches!(self.focus, ElementInFocus::TopicOverview) {
                    self.topic_overview.state.toggle_selected();
                } else {
                    self.focus = ElementInFocus::TopicOverview;
                }
            } else {
                self.focus = ElementInFocus::TopicOverview;
                self.topic_overview.state.select(identifier.to_vec());
            }

            return Refresh::Update;
        }
        if self.topic_overview.state.click_at(position) {
            self.focus = ElementInFocus::TopicOverview;
            return Refresh::Update;
        }

        if self.details.payload.last_area.contains(position) {
            match self.get_selected_payload() {
                None => return Refresh::Update, // No payload but click into payload area -> redraw
                Some(Payload::Binary(_)) => {
                    self.details.payload.binary_state.select_at(column, row);
                    self.focus = ElementInFocus::Payload;
                    return Refresh::Update;
                }
                Some(Payload::Json(_) | Payload::MessagePack(_)) => {
                    self.details.payload.json_state.click_at(position);
                    self.focus = ElementInFocus::Payload;
                    return Refresh::Update;
                }
                Some(Payload::String(_)) => return Refresh::Skip,
            }
        }

        if self.details.table_state.select_at(position) {
            self.focus = ElementInFocus::HistoryTable;
            return Refresh::Update;
        }

        Refresh::Skip
    }

    const fn table_row_at(position: Position, area: Rect, row_count: usize) -> Option<usize> {
        if row_count == 0 || !area.contains(position) || area.height < 3 {
            return None;
        }
        let data_start = area.y.saturating_add(2);
        let data_end = area.y.saturating_add(area.height).saturating_sub(1);
        if position.y < data_start || position.y >= data_end {
            return None;
        }
        let index = (position.y - data_start) as usize;
        if index < row_count { Some(index) } else { None }
    }

    // Returns `true` when selection changed
    fn search_select(&mut self, advance: SearchSelection) -> bool {
        let selection = self.topic_overview.get_selected();
        let history = self.zenoh_thread.get_history();
        let mut topics = history
            .get_all_topics()
            .into_iter()
            .enumerate()
            .collect::<Vec<_>>();

        let begin_index = selection
            .and_then(|selection| {
                topics
                    .iter()
                    .find(|(_, topic)| *topic == &selection)
                    .map(|(index, _)| *index)
            })
            .unwrap_or(0);

        // Filter out topics not matching the search
        topics.retain(|(_, topic)| topic.to_lowercase().contains(&self.topic_overview.search));

        let select = match advance {
            SearchSelection::Before => topics
                .iter()
                .rev()
                .find(|(index, _)| *index < begin_index)
                .or_else(|| topics.last()),
            SearchSelection::Stay => topics
                .iter()
                .find(|(index, _)| *index >= begin_index)
                .or_else(|| topics.first()),
            SearchSelection::After => topics
                .iter()
                .find(|(index, _)| *index > begin_index)
                .or_else(|| topics.first()),
        };
        let select = select.map_or(Vec::new(), |(_, topic)| {
            topic.split('/').map(ToOwned::to_owned).collect()
        });
        drop(history);

        for i in 0..select.len() {
            self.topic_overview.state.open(select[0..i].to_vec());
        }

        self.topic_overview.state.select(select)
    }

    // Returns `true` when the opened topics changed
    fn open_all_search_matches(&mut self) -> bool {
        let topics = self
            .zenoh_thread
            .get_history()
            .get_all_topics()
            .into_iter()
            .filter(|topic| topic.to_lowercase().contains(&self.topic_overview.search))
            .map(|topic| topic.split('/').map(ToOwned::to_owned).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let mut change = false;
        for parts in topics {
            for i in 0..parts.len() {
                if self.topic_overview.state.open(parts[0..i].to_vec()) {
                    change = true;
                }
            }
        }
        change
    }

    #[allow(clippy::too_many_lines)]
    fn draw(&mut self, frame: &mut Frame) {
        const HEADER_HEIGHT: u16 = 1;
        const FOOTER_HEIGHT: u16 = 1;
        if let Some(rx) = &self.reconnect {
            match rx.try_recv() {
                Ok(result) => {
                    self.reconnect = None;
                    match result {
                        Ok((info, worker)) => {
                            self.zenoh_thread = worker;
                            self.footer = footer::Footer::new(&info);
                            self.topic_overview = topic_overview::TopicOverview::default();
                            self.details = details::Details::default();
                            self.scout_tables = scouting::ScoutTables::default();
                            self.settings.active = info.connection;
                            self.settings.message = format!("Connected · {}", info.description);
                        }
                        Err(err) => {
                            self.settings.message =
                                format!("Connection failed: {err}. Previous session retained.");
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.reconnect = None;
                    self.settings.message =
                        "Connection worker stopped. Previous session retained.".into();
                }
            }
        }

        let connection_error = self.zenoh_thread.has_connection_err();

        let area = frame.size();
        let Rect { width, height, .. } = area;
        debug_assert_eq!(area.x, 0, "area should fill the whole space");
        debug_assert_eq!(area.y, 0, "area should fill the whole space");

        let header_area = Rect {
            height: HEADER_HEIGHT,
            y: 0,
            ..area
        };
        let footer_area = Rect {
            height: FOOTER_HEIGHT,
            y: height - 1,
            ..area
        };
        let error_height = if connection_error.is_some() { 4 } else { 0 };
        let error_area = Rect {
            height: error_height,
            y: height
                .saturating_sub(FOOTER_HEIGHT)
                .saturating_sub(error_height),
            ..area
        };
        let main_area = Rect {
            height: height
                .saturating_sub(HEADER_HEIGHT + FOOTER_HEIGHT)
                .saturating_sub(error_height),
            y: HEADER_HEIGHT,
            ..area
        };

        let tab_titles = vec![
            Line::from("Topics"),
            Line::from("Routers"),
            Line::from("Peers"),
            Line::from("Settings"),
        ];
        let tabs = Tabs::new(tab_titles)
            .select(self.active_view.index())
            .divider(Span::raw(" | "))
            .style(Style::new().fg(Color::Gray))
            .highlight_style(
                Style::new()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::LightGreen),
            );
        frame.render_widget(tabs, header_area);

        self.footer.draw(frame, footer_area, self);
        if let Some(connection_error) = connection_error {
            connection_error_widget::draw(
                frame,
                error_area,
                "Zenoh Connection Error",
                &connection_error,
            );
        }

        match self.active_view {
            ActiveView::Settings => self
                .settings
                .draw(frame, main_area, self.reconnect.is_some()),
            ActiveView::Topics => {
                let history = self.zenoh_thread.get_history();

                let overview_area = self
                    .topic_overview
                    .get_selected()
                    .as_ref()
                    .and_then(|selected_topic| history.get(selected_topic))
                    .map_or(main_area, |topic_history| {
                        let x = width / 3;
                        let details_area = Rect {
                            width: width - x,
                            x,
                            ..main_area
                        };

                        self.details
                            .draw(frame, details_area, topic_history, &self.focus);

                        Rect {
                            width: x,
                            x: 0,
                            ..main_area
                        }
                    });

                self.topic_overview.draw(
                    frame,
                    overview_area,
                    &history,
                    matches!(self.focus, ElementInFocus::TopicOverview),
                );
                drop(history);

                if let ElementInFocus::CleanPopup(topic) = &self.focus {
                    clean::draw_popup(frame, topic);
                }
            }
            ActiveView::Routers => {
                let layout = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                    .split(main_area);
                self.scout_tables.routers_area = layout[0];
                let scouting = self.zenoh_thread.get_scouting();
                let entries = scouting.routers_list();
                let error = scouting.error().map(ToString::to_string);
                drop(scouting);
                scouting::draw_table(
                    frame,
                    layout[0],
                    "Routers",
                    &entries,
                    &mut self.scout_tables.routers_state,
                    matches!(self.focus, ElementInFocus::RoutersTable),
                    error.as_deref(),
                );
                let selected = self
                    .scout_tables
                    .routers_state
                    .selected()
                    .and_then(|index| entries.get(index));
                scouting::draw_details(
                    frame,
                    layout[1],
                    "Router Details",
                    selected,
                    "No routers discovered yet",
                );
            }
            ActiveView::Peers => {
                let layout = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                    .split(main_area);
                self.scout_tables.peers_area = layout[0];
                let scouting = self.zenoh_thread.get_scouting();
                let entries = scouting.peers_list();
                let error = scouting.error().map(ToString::to_string);
                drop(scouting);
                scouting::draw_table(
                    frame,
                    layout[0],
                    "Peers",
                    &entries,
                    &mut self.scout_tables.peers_state,
                    matches!(self.focus, ElementInFocus::PeersTable),
                    error.as_deref(),
                );
                let selected = self
                    .scout_tables
                    .peers_state
                    .selected()
                    .and_then(|index| entries.get(index));
                scouting::draw_details(
                    frame,
                    layout[1],
                    "Peer Details",
                    selected,
                    "No peers discovered yet",
                );
            }
        }
    }
}
