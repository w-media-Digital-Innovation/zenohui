use zenoh::sample::SampleKind;

pub struct HistoryEntry {
    pub kind: SampleKind,
    pub time: crate::zenoh_client::Time,
    pub payload_size: usize,
    pub payload: crate::payload::Payload,
}
