use super::SnapshotEntry;
use crate::serialize_yaml;
use text_block_macros::text_block;

#[test]
fn optional_true_round_trips() {
    let yaml = text_block! {
        "dependencies:"
        "  foo: 1.2.3"
        "optional: true"
    };
    let entry: SnapshotEntry = serde_saphyr::from_str(yaml).expect("parse");
    assert!(entry.optional, "deserialize must capture optional: true");

    let out = serialize_yaml::to_string(&entry).expect("serialize");
    assert!(out.contains("optional: true"), "serialize must round-trip optional: true:\n{out}");
}

#[test]
fn optional_defaults_false_and_omits_when_false() {
    let yaml = text_block! {
        "dependencies:"
        "  bar: 1.0.0"
    };
    let entry: SnapshotEntry = serde_saphyr::from_str(yaml).expect("parse");
    assert!(!entry.optional, "default must be false when absent");

    let out = serialize_yaml::to_string(&entry).expect("serialize");
    // Match the exact key spelling (`optional:` followed by a space
    // or a newline) so a future fixture containing
    // `optionalDependencies:` doesn't fool this assertion.
    assert!(
        !out.contains("optional: ") && !out.contains("optional:\n"),
        "the `optional` key must not be serialized when false:\n{out}",
    );
}
