use std::collections::{BTreeMap, BTreeSet};

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap};
use zenoh::config::WhatAmI;
use zenoh::scouting::Hello;

use crate::interactive::ui;

#[derive(Clone)]
pub struct ScoutEntry {
    pub zid: String,
    pub connected: bool,
    pub whatami: WhatAmI,
    pub locators: Vec<String>,
}

#[derive(Default)]
pub struct ScoutCache {
    routers: BTreeMap<String, ScoutEntry>,
    peers: BTreeMap<String, ScoutEntry>,
    error: Option<String>,
}

impl ScoutCache {
    pub fn update(&mut self, hello: &Hello) {
        let zid = hello.zid().to_string();
        let mut entry = ScoutEntry {
            zid: zid.clone(),
            connected: false,
            whatami: hello.whatami(),
            locators: hello.locators().iter().map(ToString::to_string).collect(),
        };
        if let Some(existing) = self
            .routers
            .remove(&zid)
            .or_else(|| self.peers.remove(&zid))
        {
            merge_locators(&existing.locators, &mut entry.locators);
        }
        match entry.whatami {
            WhatAmI::Router => {
                self.routers.insert(zid, entry);
            }
            WhatAmI::Peer => {
                self.peers.insert(zid, entry);
            }
            WhatAmI::Client => {}
        }
        self.error = None;
    }

    pub fn sync_connected(&mut self, routers: Vec<String>, peers: Vec<String>) {
        for (entries, ids, whatami) in [
            (&mut self.routers, routers, WhatAmI::Router),
            (&mut self.peers, peers, WhatAmI::Peer),
        ] {
            entries.retain(|_, entry| !entry.locators.is_empty());
            for entry in entries.values_mut() {
                entry.connected = false;
            }
            for zid in ids {
                entries
                    .entry(zid.clone())
                    .or_insert_with(|| ScoutEntry {
                        zid,
                        whatami,
                        locators: vec![],
                        connected: false,
                    })
                    .connected = true;
            }
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn routers_list(&self) -> Vec<ScoutEntry> {
        self.routers.values().cloned().collect()
    }

    pub fn peers_list(&self) -> Vec<ScoutEntry> {
        self.peers.values().cloned().collect()
    }

    pub fn routers_len(&self) -> usize {
        self.routers.len()
    }

    pub fn peers_len(&self) -> usize {
        self.peers.len()
    }
}

#[derive(Default)]
pub struct ScoutTables {
    pub routers_state: TableState,
    pub peers_state: TableState,
    pub routers_area: Rect,
    pub peers_area: Rect,
}

const ADDRESS_SUMMARY_LIMIT: usize = 2;

pub fn draw_table(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    entries: &[ScoutEntry],
    table_state: &mut TableState,
    has_focus: bool,
    error: Option<&str>,
) {
    let block = Block::default().borders(Borders::ALL).title(title);
    if let Some(error) = error.filter(|_| entries.is_empty()) {
        let paragraph = Paragraph::new(error)
            .style(Style::new().fg(Color::Red))
            .block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    let rows = if entries.is_empty() {
        vec![Row::new(vec![
            Cell::from("No nodes found — connect via Settings (4)"),
            Cell::from(""),
            Cell::from(""),
            Cell::from(""),
        ])]
    } else {
        entries
            .iter()
            .map(|entry| {
                let (protocols, addresses) = locator_parts(entry);
                Row::new(vec![
                    Cell::from(entry.zid.as_str()),
                    Cell::from(if entry.connected {
                        "Connected"
                    } else {
                        "Discovered"
                    }),
                    Cell::from(protocols),
                    Cell::from(addresses),
                ])
            })
            .collect()
    };

    if entries.is_empty() {
        table_state.select(None);
    } else if let Some(selected) = table_state.selected() {
        let clamped = selected.min(entries.len().saturating_sub(1));
        if clamped != selected {
            table_state.select(Some(clamped));
        }
    } else {
        table_state.select(Some(0));
    }

    let header = Row::new(vec![
        Cell::from("Peer-ID"),
        Cell::from("Status"),
        Cell::from("Protocols"),
        Cell::from("Addresses"),
    ])
    .style(Style::new().add_modifier(Modifier::BOLD));

    let widths = [
        Constraint::Length(18),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Min(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .highlight_style(
            Style::new()
                .bg(ui::focus_color(has_focus))
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(table, area, table_state);
}

pub fn draw_details(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    entry: Option<&ScoutEntry>,
    empty_message: &str,
) {
    let block = Block::default().borders(Borders::ALL).title(title);
    let text = entry.map_or_else(
        || Text::from(empty_message),
        |entry| {
            let (protocols, _, locators) = locator_sets(entry);
            let mut lines = Vec::new();
            lines.push(Line::from(format!("Peer-ID: {}", entry.zid)));
            lines.push(Line::from(if entry.connected {
                "Status: Connected"
            } else {
                "Status: Discovered (not connected)"
            }));
            lines.push(Line::from(format!("Type: {:?}", entry.whatami)));
            lines.push(Line::from(format!("Protocols: {}", join_set(&protocols))));
            lines.push(Line::from("Locators:"));
            if locators.is_empty() {
                lines.push(Line::from("-"));
            } else {
                for locator in locators {
                    lines.push(Line::from(format!("  - {locator}")));
                }
            }
            Text::from(lines)
        },
    );

    let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn locator_parts(entry: &ScoutEntry) -> (String, String) {
    let (protocols, addresses, _) = locator_sets(entry);
    let protocols = join_set(&protocols);
    let addresses = summarize_set(&addresses, ADDRESS_SUMMARY_LIMIT);
    (protocols, addresses)
}

fn locator_sets(entry: &ScoutEntry) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
    let mut protocols = BTreeSet::new();
    let mut addresses = BTreeSet::new();
    let mut locators = BTreeSet::new();
    for locator in &entry.locators {
        let (protocol, address) = split_locator(locator);
        if !protocol.is_empty() {
            protocols.insert(protocol.to_string());
        }
        if !address.is_empty() {
            addresses.insert(address.to_string());
        }
        if !address.is_empty() {
            if protocol.is_empty() {
                locators.insert(address.to_string());
            } else {
                locators.insert(format!("{protocol}/{address}"));
            }
        }
    }
    (protocols, addresses, locators)
}

fn join_set(values: &BTreeSet<String>) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.iter().cloned().collect::<Vec<_>>().join(", ")
    }
}

fn summarize_set(values: &BTreeSet<String>, max: usize) -> String {
    if values.is_empty() {
        return "-".to_string();
    }
    let mut shown = Vec::new();
    for value in values.iter().take(max) {
        shown.push(value.as_str());
    }
    let remaining = values.len().saturating_sub(shown.len());
    if remaining == 0 {
        shown.join(", ")
    } else {
        format!("{} (+{} more)", shown.join(", "), remaining)
    }
}

fn merge_locators(existing: &[String], incoming: &mut Vec<String>) {
    let mut set = BTreeSet::new();
    for locator in existing {
        set.insert(locator.clone());
    }
    for locator in incoming.drain(..) {
        set.insert(locator);
    }
    *incoming = set.into_iter().collect();
}

fn split_locator(locator: &str) -> (&str, &str) {
    let (protocol, rest) = locator.split_once('/').unwrap_or(("", locator));
    let address = rest.split_once('?').map_or(rest, |(addr, _)| addr);
    (protocol, address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_router_appears_without_multicast_and_disappears_on_disconnect() {
        let mut cache = ScoutCache::default();
        cache.sync_connected(vec!["remote".into()], vec![]);
        assert!(cache.routers_list()[0].connected);
        assert_eq!(cache.routers_list()[0].locators, Vec::<String>::new());
        cache.sync_connected(vec![], vec![]);
        assert_eq!(cache.routers_len(), 0);
    }

    #[test]
    fn discovered_node_keeps_locators_after_disconnect() {
        let mut cache = ScoutCache::default();
        cache.routers.insert(
            "router".into(),
            ScoutEntry {
                zid: "router".into(),
                connected: false,
                whatami: WhatAmI::Router,
                locators: vec!["tcp/localhost:7447".into()],
            },
        );
        cache.sync_connected(vec!["router".into()], vec![]);
        assert_eq!(cache.routers_len(), 1);
        assert!(cache.routers_list()[0].connected);
        cache.sync_connected(vec![], vec![]);
        assert!(!cache.routers_list()[0].connected);
        assert_eq!(cache.routers_list()[0].locators.len(), 1);
    }
}
