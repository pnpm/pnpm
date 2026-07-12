use pretty_assertions::assert_eq;

use super::{Ledger, LedgerEntry, render_ledger, yaml_scalar};

#[test]
fn real_content_renders_as_plain_scalars() {
    let mut ledger = Ledger::new();
    ledger.insert(
        "@scope/util@1.2.3".to_string(),
        LedgerEntry::Attributed {
            dir: "packages/util".to_string(),
            intents: vec!["wild-wombats-do".to_string()],
        },
    );
    let rendered = render_ledger(&ledger);
    assert_eq!(
        rendered,
        "\"@scope/util@1.2.3\":\n  dir: packages/util\n  intents:\n    - wild-wombats-do\n",
    );
}

#[test]
fn special_characters_round_trip_through_the_serializer() {
    let mut ledger = Ledger::new();
    ledger.insert(
        "pkg@1.0.0".to_string(),
        LedgerEntry::Attributed {
            dir: "weird: dir #1".to_string(),
            intents: vec!["id: with colon".to_string(), "plain-id".to_string()],
        },
    );
    let rendered = render_ledger(&ledger);
    let parsed: Ledger = serde_saphyr::from_str(&rendered).expect("render output parses back");
    assert_eq!(parsed, ledger);
}

#[test]
fn yaml_scalar_quotes_only_when_needed() {
    assert_eq!(yaml_scalar("packages/util"), "packages/util");
    assert_eq!(yaml_scalar("pkg@1.0.0"), "pkg@1.0.0");
    assert_eq!(yaml_scalar("@scope/pkg"), r#""@scope/pkg""#);
    assert_eq!(yaml_scalar("a: b"), r#""a: b""#);
    assert_eq!(yaml_scalar("has #hash"), r#""has #hash""#);
}
