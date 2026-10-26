use std::collections::HashMap;

use ego_tree::{NodeId, NodeRef, Tree};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use tui_tree_widget::TreeItem;

use crate::interactive::ui::STYLE_BOLD;
use crate::zenoh_client::HistoryEntry;

pub const MAX_TOPICS: usize = 1000;
pub const MAX_ENTRIES: usize = 10_000;
pub const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;

pub const STYLE_DARKGRAY: Style = Style::new().fg(Color::DarkGray);

struct Topic {
    /// Key expression `foo/bar` would have the leaf `bar`
    leaf: Box<str>,
    history: Vec<HistoryEntry>,
}

impl Topic {
    const fn new(leaf: Box<str>) -> Self {
        Self {
            leaf,
            history: Vec::new(),
        }
    }
}

struct RecursiveTreeItemGenerator {
    messages_below: usize,
    messages: usize,
    topics_below: usize,
    tree_item: TreeItem<'static, String>,
}

pub struct ZenohHistory {
    tree: Tree<Topic>,
    ids: HashMap<String, NodeId>,
    per_topic_limit: usize,
    max_bytes: usize,
    max_entries: usize,
    max_topics: usize,
    entries: usize,
    bytes: usize,
    pub discarded: usize,
}

impl ZenohHistory {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_limit(1000)
    }

    pub fn with_limit(per_topic_limit: usize) -> Self {
        Self {
            tree: Tree::new(Topic::new("".into())),
            ids: HashMap::new(),
            per_topic_limit: per_topic_limit.clamp(1, MAX_ENTRIES),
            max_bytes: MAX_HISTORY_BYTES,
            max_entries: MAX_ENTRIES,
            max_topics: MAX_TOPICS,
            entries: 0,
            bytes: 0,
            discarded: 0,
        }
    }

    fn entry(&mut self, keyexpr: String) -> NodeId {
        if let Some(id) = self.ids.get(&keyexpr) {
            *id
        } else {
            let mut parent = self.tree.root().id();
            for part in keyexpr.split('/') {
                let noderef = self.tree.get(parent).unwrap();
                let equal_or_after = noderef.children().find(|node| &*node.value().leaf >= part);
                if let Some(eoa) = equal_or_after {
                    if eoa.value().leaf.as_ref() == part {
                        parent = eoa.id();
                        continue;
                    }
                    let eoa_id = eoa.id();
                    let mut eoamut = self.tree.get_mut(eoa_id).unwrap();
                    parent = eoamut.insert_before(Topic::new(part.into())).id();
                } else {
                    let mut nodemut = self.tree.get_mut(parent).unwrap();
                    parent = nodemut.append(Topic::new(part.into())).id();
                }
            }
            self.ids.insert(keyexpr, parent);
            parent
        }
    }

    fn entry_bytes(entry: &HistoryEntry) -> usize {
        std::mem::size_of::<HistoryEntry>() + entry.payload.estimated_size() + 128
    }

    fn remove(&mut self, id: NodeId, index: usize) -> HistoryEntry {
        let mut node = self.tree.get_mut(id).unwrap();
        let history = &mut node.value().history;
        let entry = history.remove(index);
        // Release unused vector capacity as well as the removed payload.
        if history.capacity() > history.len().saturating_mul(2).max(16) {
            history.shrink_to_fit();
        }
        self.entries -= 1;
        self.bytes -= Self::entry_bytes(&entry);
        entry
    }

    pub fn add(&mut self, keyexpr: String, history_entry: HistoryEntry) {
        let bytes = Self::entry_bytes(&history_entry);
        if keyexpr.len() > 4096
            || keyexpr.split('/').count() > 64
            || bytes > self.max_bytes
            || (!self.ids.contains_key(&keyexpr) && self.ids.len() >= self.max_topics)
        {
            self.discarded = self.discarded.saturating_add(1);
            return;
        }
        let id = self.entry(keyexpr);
        if self.tree.get(id).unwrap().value().history.len() >= self.per_topic_limit {
            self.remove(id, 0);
            self.discarded = self.discarded.saturating_add(1);
        }
        while self.entries >= self.max_entries || self.bytes + bytes > self.max_bytes {
            // Trim the largest retained history first under global pressure.
            let victim = self
                .ids
                .values()
                .copied()
                .max_by_key(|id| self.tree.get(*id).unwrap().value().history.len())
                .unwrap();
            self.remove(victim, 0);
            self.discarded = self.discarded.saturating_add(1);
        }
        self.tree
            .get_mut(id)
            .unwrap()
            .value()
            .history
            .push(history_entry);
        self.entries += 1;
        self.bytes += bytes;
    }

    pub fn get(&self, keyexpr: &str) -> Option<&Vec<HistoryEntry>> {
        let id = self.ids.get(keyexpr)?;
        self.tree
            .get(*id)
            .map(|node| &node.value().history)
            .filter(|history| !history.is_empty())
    }

    pub fn uncache_topic_entry(&mut self, keyexpr: &str, index: usize) -> Option<HistoryEntry> {
        let id = self.ids.get(keyexpr)?;
        let topic = self.tree.get(*id)?.value();
        // Prevent removing the newest entry (or out of bounds)
        if index >= topic.history.len().saturating_sub(1) {
            return None;
        }
        let id = *id;
        Some(self.remove(id, index))
    }

    pub fn get_all_topics(&self) -> Vec<&String> {
        let mut topics = self.ids.keys().collect::<Vec<_>>();
        topics.sort();
        topics
    }

    pub fn get_topics_below(&self, base: &str) -> Vec<String> {
        self.ids
            .keys()
            .filter(|key| is_topic_below(base, key))
            .cloned()
            .collect()
    }

    /// Returns (`topic_amount`, `message_amount`, `TreeItem`s)
    pub fn to_tree_items(&self) -> (usize, usize, Vec<TreeItem<'static, String>>) {
        fn build_recursive(prefix: &[&str], node: NodeRef<Topic>) -> RecursiveTreeItemGenerator {
            let Topic { leaf, history } = node.value();
            let mut topic = prefix.to_vec();
            topic.push(leaf);

            let entries_below = node.children().map(|node| build_recursive(&topic, node));
            let mut messages_below: usize = 0;
            let mut topics_below: usize = 0;
            let mut children = Vec::new();
            for below in entries_below {
                messages_below = messages_below
                    .saturating_add(below.messages)
                    .saturating_add(below.messages_below);
                topics_below = topics_below
                    .saturating_add(usize::from(below.messages > 0))
                    .saturating_add(below.topics_below);
                children.push(below.tree_item);
            }

            let meta = history.last().map(|entry| &entry.payload).map_or_else(
                || format!("({topics_below} topics, {messages_below} messages)"),
                |payload| format!("= {payload}"),
            );
            let text = Line::from(vec![
                Span::styled(leaf.to_string(), STYLE_BOLD),
                Span::raw(" "),
                Span::styled(meta, STYLE_DARKGRAY),
            ]);

            RecursiveTreeItemGenerator {
                messages_below,
                messages: history.len(),
                topics_below,
                tree_item: TreeItem::new(leaf.to_string(), text, children).unwrap(),
            }
        }

        let children = self
            .tree
            .root()
            .children()
            .map(|node| build_recursive(&[], node));
        let mut topics: usize = 0;
        let mut messages: usize = 0;
        let mut items = Vec::new();
        for child in children {
            topics = topics
                .saturating_add(usize::from(child.messages > 0))
                .saturating_add(child.topics_below);
            messages = messages
                .saturating_add(child.messages)
                .saturating_add(child.messages_below);
            items.push(child.tree_item);
        }
        (topics, messages, items)
    }

    #[cfg(test)]
    pub fn example() -> Self {
        use zenoh::sample::SampleKind;

        fn entry(payload: &str) -> HistoryEntry {
            HistoryEntry {
                kind: SampleKind::Put,
                time: crate::zenoh_client::Time::new_now(),
                payload_size: payload.len(),
                payload: crate::payload::Payload::unlimited(payload.into()),
            }
        }

        let mut history = Self::new();
        history.add("test".to_owned(), entry("A"));
        history.add("foo/test".to_owned(), entry("B"));
        history.add("test".to_owned(), entry("C"));
        history.add("foo/bar".to_owned(), entry("D"));
        history.add("testing/stuff".to_owned(), entry("E"));
        history
    }
}

fn is_topic_below(base: &str, child: &str) -> bool {
    if base == child {
        return true;
    }
    if !child.starts_with(base) {
        return false;
    }
    child
        .get(base.len()..)
        .is_some_and(|after| after.starts_with('/'))
}

#[test]
fn topics_below_works() {
    let mut actual = ZenohHistory::example().get_topics_below("foo");
    actual.sort_unstable();
    assert_eq!(actual, ["foo/bar", "foo/test"]);
}

#[test]
fn topics_below_finds_itself_works() {
    let actual = ZenohHistory::example().get_topics_below("test");
    assert_eq!(actual, ["test"]);
}

#[test]
fn tree_items_works() {
    let example = ZenohHistory::example();
    let (topics, messages, items) = example.to_tree_items();
    assert_eq!(topics, 4);
    assert_eq!(messages, 5);
    dbg!(&items);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].children().len(), 2);
    assert_eq!(items[1].children().len(), 0);
    assert_eq!(items[2].children().len(), 1);
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    use crate::payload::Payload;
    use crate::zenoh_client::Time;
    use zenoh::sample::SampleKind;

    fn entry(value: &str) -> HistoryEntry {
        HistoryEntry {
            kind: SampleKind::Put,
            time: Time::new_now(),
            payload_size: value.len(),
            payload: Payload::String(value.into()),
        }
    }

    #[test]
    fn oldest_entries_are_removed_but_latest_value_is_kept() {
        let mut history = ZenohHistory::with_limit(3);
        for i in 0..20 {
            history.add("temperature".into(), entry(&i.to_string()));
        }
        let entries = history.get("temperature").unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].payload.to_string(), "17");
        assert_eq!(entries[2].payload.to_string(), "19");
        assert_eq!(history.discarded, 17);
        assert!(history.uncache_topic_entry("temperature", 2).is_none());
        assert!(history.uncache_topic_entry("temperature", 0).is_some());
        assert_eq!(history.entries, 2);
    }

    #[test]
    fn sustained_load_stays_within_global_entry_and_byte_budgets() {
        let mut history = ZenohHistory::with_limit(1000);
        history.max_bytes = 8192;
        history.max_entries = 16;
        for i in 0..100_000 {
            history.add(format!("sensor/{}", i % 20), entry(&i.to_string()));
            assert!(history.bytes <= history.max_bytes);
            assert!(history.entries <= history.max_entries);
        }
        let actual_bytes: usize = history
            .ids
            .values()
            .map(|id| {
                history
                    .tree
                    .get(*id)
                    .unwrap()
                    .value()
                    .history
                    .iter()
                    .map(ZenohHistory::entry_bytes)
                    .sum::<usize>()
            })
            .sum();
        assert_eq!(history.bytes, actual_bytes);
        assert!(history.discarded > 99_000);
        history.to_tree_items();
    }

    #[test]
    fn topic_churn_and_oversized_entries_are_bounded() {
        let mut history = ZenohHistory::with_limit(3);
        history.max_topics = 8;
        history.max_bytes = 4096;
        for i in 0..10_000 {
            history.add(format!("sensor/{i}"), entry("ok"));
        }
        assert_eq!(history.ids.len(), 8);
        history.add("sensor/0".into(), entry(&"x".repeat(8192)));
        assert_eq!(
            history.get("sensor/0").unwrap()[0].payload.to_string(),
            "ok"
        );
        assert!(history.bytes <= 4096);
    }

    #[test]
    fn json_heap_expansion_counts_against_the_budget() {
        let mut history = ZenohHistory::with_limit(1000);
        history.max_bytes = 4096;
        let json = serde_json::json!({"readings": [1, 2, 3, 4, 5], "unit": "°C"});
        for _ in 0..100 {
            history.add(
                "sensor".into(),
                HistoryEntry {
                    kind: SampleKind::Put,
                    time: Time::new_now(),
                    payload_size: 1,
                    payload: Payload::Json(json.clone()),
                },
            );
        }
        assert!(history.get("sensor").unwrap().len() < 100);
        assert!(history.bytes <= 4096);
        assert!(history.bytes > 100);
    }
}

#[test]
fn default_history_budget_plateaus_under_100000_json_samples() {
    use crate::payload::Payload;
    use crate::zenoh_client::Time;
    use zenoh::sample::SampleKind;
    let payload = serde_json::json!({"node_id":"test-node", "readings":[{"channel":"TMP117", "unit":"°C", "value":24.08}], "frame_age_ms":18, "fix":"NoFix"});
    let mut history = ZenohHistory::with_limit(1000);
    let mut plateau = 0;
    for i in 0..100_000 {
        history.add(
            format!("sensor/{}", i % 40),
            HistoryEntry {
                kind: SampleKind::Put,
                time: Time::new_now(),
                payload_size: 150,
                payload: Payload::Json(payload.clone()),
            },
        );
        if i == 20_000 {
            plateau = history.bytes;
        }
    }
    assert_eq!(history.entries, MAX_ENTRIES);
    assert_eq!(history.bytes, plateau);
    assert!(history.bytes <= MAX_HISTORY_BYTES);
    assert_eq!(history.discarded, 90_000);
}
