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

#[test]
fn artifact_pins_round_trip_and_preserve_other_inputs() {
    let yaml = text_block! {
        "artifactPins:"
        "  dependency-side-effects:v1:deps=old:"
        "    organization:acme:"
        "      linux-node22: abc123"
    };
    let mut entry: SnapshotEntry = serde_saphyr::from_str(yaml).expect("parse");
    assert!(!entry.record_artifact_pin(
        "dependency-side-effects:v1:deps=old".to_string(),
        "organization:acme".to_string(),
        "linux-node22".to_string(),
        "abc123".to_string(),
    ));
    assert!(entry.record_artifact_pin(
        "dependency-side-effects:v1:deps=new".to_string(),
        "organization:acme".to_string(),
        "linux-node22".to_string(),
        "def456".to_string(),
    ));
    let out = serialize_yaml::to_string(&entry).expect("serialize");
    assert!(out.contains("dependency-side-effects:v1:deps=old:"));
    assert!(out.contains("dependency-side-effects:v1:deps=new:"));
    assert!(out.contains("linux-node22: def456"));
    assert!(entry.clear_artifact_pins());
    assert!(!entry.clear_artifact_pins());
}
