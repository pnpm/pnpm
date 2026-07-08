use super::IniSettings;

#[test]
fn parses_flat_key_value_lines() {
    let settings =
        IniSettings::parse("//registry.npmjs.org/:_authToken=my-token-123\nother-setting=value\n");
    assert_eq!(settings.get("//registry.npmjs.org/:_authToken"), Some("my-token-123"));
    assert_eq!(settings.get("other-setting"), Some("value"));
}

#[test]
fn skips_blanks_comments_and_sections() {
    let settings = IniSettings::parse("; a comment\n# another\n[section]\n\nkey=value\n");
    assert_eq!(settings.get("key"), Some("value"));
    assert_eq!(settings.get("section"), None);
}

#[test]
fn remove_reports_presence_and_drops_the_entry() {
    let mut settings = IniSettings::parse("a=1\nb=2\n");
    assert!(settings.remove("a"));
    assert!(!settings.remove("a"));
    assert_eq!(settings.get("a"), None);
    assert_eq!(settings.get("b"), Some("2"));
}

#[test]
fn serialize_round_trips_remaining_entries_in_order() {
    let mut settings = IniSettings::parse("//registry.npmjs.org/:_authToken=tok\nother=value\n");
    settings.remove("//registry.npmjs.org/:_authToken");
    assert_eq!(settings.serialize(), "other=value\n");
}

// A registry-controlled token with an embedded newline must not be able to
// plant extra `auth.ini` entries. It is quoted (JSON) so it stays on one
// physical line and round-trips to the exact value.
#[test]
fn quotes_values_with_newlines_to_prevent_auth_ini_injection() {
    let injected = "x\n//registry.npmjs.org/:_authToken=attacker-token";
    let mut settings = IniSettings::default();
    settings.set("//evil.example/:_authToken", injected);

    let text = settings.serialize();
    assert_eq!(text.lines().count(), 1, "the value must stay on one line: {text:?}");

    let reparsed = IniSettings::parse(&text);
    assert_eq!(reparsed.get("//evil.example/:_authToken"), Some(injected));
    assert_eq!(
        reparsed.get("//registry.npmjs.org/:_authToken"),
        None,
        "no auth entry was injected",
    );
}

// `encode_value` and `decode_value` must be inverses for every value shape
// the `ini` package quotes — `=`, an already-`"`-wrapped value (else the
// quotes are stripped on read), leading/trailing whitespace (else trimmed),
// and a leading `[` — not just newlines.
#[test]
fn quotes_every_ambiguous_value_shape_for_a_faithful_round_trip() {
    for value in [
        "a=b=c",
        r#""already-quoted""#,
        r#""""#,
        " leading-space",
        "trailing-space ",
        "[bracketed",
        "plain-token",
    ] {
        let mut settings = IniSettings::default();
        settings.set("k", value);
        let reparsed = IniSettings::parse(&settings.serialize());
        assert_eq!(reparsed.get("k"), Some(value), "round-trip failed for {value:?}");
    }
}

// `set` collapses pre-existing duplicate keys to a single value, matching
// `remove`'s all-duplicates handling and the `ini` object model.
#[test]
fn set_collapses_pre_existing_duplicate_keys() {
    let mut settings = IniSettings::parse("//reg/:_authToken=old1\n//reg/:_authToken=old2\n");
    settings.set("//reg/:_authToken", "new");
    assert_eq!(settings.serialize(), "//reg/:_authToken=new\n");
}
