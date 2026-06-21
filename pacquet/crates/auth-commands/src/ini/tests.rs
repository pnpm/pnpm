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
