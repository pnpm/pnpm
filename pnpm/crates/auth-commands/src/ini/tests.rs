use super::{IniSettings, encode_value};

/// One `auth.ini` line holding `key = value`, quoted the way the writer
/// quotes it, so a test can start from a file an earlier `pnpm login` — or a
/// hand edit — could have left behind.
fn line(key: &str, value: &str) -> String {
    format!("{}={}\n", encode_value(key), encode_value(value))
}

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

// A token with an embedded newline stays one quoted line across a removal,
// so rewriting the file cannot turn that token's own text into further
// `auth.ini` entries.
#[test]
fn a_value_with_a_newline_survives_a_removal_as_one_line() {
    let injected = "x\n//registry.npmjs.org/:_authToken=attacker-token";
    let mut text = line("//evil.example/:_authToken", injected);
    text.push_str("other=value\n");
    let mut settings = IniSettings::parse(&text);
    settings.remove("other");

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
        let settings = IniSettings::parse(&line("k", value));
        let reparsed = IniSettings::parse(&settings.serialize());
        assert_eq!(reparsed.get("k"), Some(value), "round-trip failed for {value:?}");
    }
}

// Keys are quoted like values: a registry whose path contains `=` (or, as an
// injection guard, CR/LF) must key its token faithfully rather than split at
// the wrong `=`, and an ordinary key stays unquoted.
#[test]
fn quotes_and_round_trips_keys_with_a_separator_or_newline() {
    for key in [
        "//npm.example.com/foo=bar/:_authToken",
        "//npm.example.com/a\nb/:_authToken",
        "//registry.npmjs.org/:_authToken",
    ] {
        let settings = IniSettings::parse(&line(key, "the-token"));
        let text = settings.serialize();
        assert_eq!(text.lines().count(), 1, "one physical line for {key:?}: {text:?}");
        let reparsed = IniSettings::parse(&text);
        assert_eq!(reparsed.get(key), Some("the-token"), "key round-trip failed for {key:?}");
    }
}

// A logout must leave no copy of the token behind, so `remove` drops every
// duplicate of the key rather than only the first.
#[test]
fn remove_drops_every_duplicate_of_the_key() {
    let mut settings = IniSettings::parse("//reg/:_authToken=old1\n//reg/:_authToken=old2\n");
    assert!(settings.remove("//reg/:_authToken"));
    assert_eq!(settings.serialize(), "");
}
