use super::parse_update_param;

#[test]
fn parses_multibyte_utf8_and_negated_selectors() {
    let direct = parse_update_param("ä@^1.2.0");
    assert_eq!(direct.pattern, "ä");
    assert_eq!(direct.version.as_deref(), Some("^1.2.0"));

    let negated = parse_update_param("!ä@^1.2.0");
    assert_eq!(negated.pattern, "!ä");
    assert_eq!(negated.version.as_deref(), Some("^1.2.0"));

    let negated_scoped = parse_update_param("!@ä/ü@2.0.0");
    assert_eq!(negated_scoped.pattern, "!@ä/ü");
    assert_eq!(negated_scoped.version.as_deref(), Some("2.0.0"));
}
