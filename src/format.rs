use zenoh::sample::SampleKind;

pub const fn kind(kind: SampleKind) -> &'static str {
    match kind {
        SampleKind::Put => "Put",
        SampleKind::Delete => "Delete",
    }
}

#[test]
fn formats_kind() {
    assert_eq!("Put", kind(SampleKind::Put));
    assert_eq!("Delete", kind(SampleKind::Delete));
}
