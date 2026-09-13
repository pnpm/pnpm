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
fn control_characters_are_escaped_and_round_trip() {
    // A crafted intent id / directory with control characters (from an odd
    // .changeset filename) must not produce unparsable YAML.
    let mut ledger = Ledger::new();
    ledger.insert(
        "pkg@1.0.0".to_string(),
        LedgerEntry::Attributed {
            dir: "tab\ttab".to_string(),
            intents: vec![
                "carriage\rreturn".to_string(),
                "nul\0byte".to_string(),
                "esc\u{1b}here".to_string(),
            ],
        },
    );
    let rendered = render_ledger(&ledger);
    assert!(!rendered.contains('\r'), "carriage return must be escaped, not literal");
    assert!(!rendered.contains('\0'), "NUL must be escaped, not literal");
    let parsed: Ledger = serde_saphyr::from_str(&rendered).expect("render output parses back");
    assert_eq!(parsed, ledger);
}

#[test]
fn empty_intent_lists_render_as_flow_sequences_and_round_trip() {
    let mut ledger = Ledger::new();
    ledger.insert(
        "pacquet@12.0.0-alpha.13".to_string(),
        LedgerEntry::Attributed { dir: "pnpm/npm/pnpm".to_string(), intents: Vec::new() },
    );
    ledger.insert("pkg@1.0.0".to_string(), LedgerEntry::Ids(Vec::new()));
    let rendered = render_ledger(&ledger);
    assert_eq!(
        rendered,
        "pacquet@12.0.0-alpha.13:\n  dir: pnpm/npm/pnpm\n  intents: []\npkg@1.0.0: []\n",
    );
    let parsed: Ledger = serde_saphyr::from_str(&rendered).expect("render output parses back");
    assert_eq!(parsed, ledger);
}

#[test]
fn null_and_missing_intent_lists_parse_as_empty() {
    let parsed: Ledger = serde_saphyr::from_str(concat!(
        "pacquet@12.0.0-alpha.13:\n  dir: pnpm/npm/pnpm\n  intents:\n",
        "pkg@1.0.0:\n",
        "other@2.0.0:\n  dir: packages/other\n",
    ))
    .expect("bare and intents-less keys parse");
    assert_eq!(
        parsed.get("pacquet@12.0.0-alpha.13"),
        Some(&LedgerEntry::Attributed { dir: "pnpm/npm/pnpm".to_string(), intents: Vec::new() }),
    );
    assert_eq!(parsed.get("pkg@1.0.0"), Some(&LedgerEntry::Ids(Vec::new())));
    assert_eq!(
        parsed.get("other@2.0.0"),
        Some(&LedgerEntry::Attributed { dir: "packages/other".to_string(), intents: Vec::new() }),
    );
}

#[test]
fn yaml_scalar_quotes_only_when_needed() {
    assert_eq!(yaml_scalar("packages/util"), "packages/util");
    assert_eq!(yaml_scalar("pkg@1.0.0"), "pkg@1.0.0");
    assert_eq!(yaml_scalar("@scope/pkg"), r#""@scope/pkg""#);
    assert_eq!(yaml_scalar("a: b"), r#""a: b""#);
    assert_eq!(yaml_scalar("has #hash"), r#""has #hash""#);
}
