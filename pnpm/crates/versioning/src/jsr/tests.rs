use super::top_level_version_span;
use pretty_assertions::assert_eq;

fn replace_version(text: &str) -> Option<String> {
    let span = top_level_version_span(text)?;
    let mut updated = text.to_string();
    updated.replace_range(span, r#""2.0.0""#);
    Some(updated)
}

#[test]
fn replaces_only_the_root_version_literal() {
    let text = "\u{feff}{\n  // \"version\": \"0.0.0\"\n  \"name\": \"@scope/pkg\",\n  \"exports\": { \"version\": \"./version.ts\" },\n  /* \"version\": */ \"version\" : \"1.0.0\",\n}\n";
    assert_eq!(
        replace_version(text).as_deref(),
        Some(
            "\u{feff}{\n  // \"version\": \"0.0.0\"\n  \"name\": \"@scope/pkg\",\n  \"exports\": { \"version\": \"./version.ts\" },\n  /* \"version\": */ \"version\" : \"2.0.0\",\n}\n"
        ),
    );
}

#[test]
fn string_values_named_version_are_not_keys() {
    let text = r#"{"name":"version","description":"a \"version\": \"x\"","version":"1.0.0"}"#;
    assert_eq!(
        replace_version(text).as_deref(),
        Some(r#"{"name":"version","description":"a \"version\": \"x\"","version":"2.0.0"}"#),
    );
}

#[test]
fn nested_version_keys_are_skipped() {
    assert_eq!(replace_version(r#"{"exports":{"version":"1"},"tags":["version"]}"#), None);
}
