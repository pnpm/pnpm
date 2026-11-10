use super::json_lockfile_version;

#[test]
fn the_root_version_is_read_past_a_nested_field_of_the_same_name() {
    let document = r#"{"settings": {"lockfileVersion": "13.0"}, "lockfileVersion": "9.0"}"#;
    assert_eq!(json_lockfile_version(document), Some("9.0"));
    let document =
        r#"{"x": ["lockfileVersion", {"lockfileVersion": "13.0"}], "lockfileVersion": "9.0"}"#;
    assert_eq!(json_lockfile_version(document), Some("9.0"));
    let document = r#"{"name": "say \"lockfileVersion\"", "lockfileVersion": "13.0"}"#;
    assert_eq!(json_lockfile_version(document), Some("13.0"));
    assert_eq!(json_lockfile_version(r#"{"packages": {}}"#), None);
    assert_eq!(json_lockfile_version(r#"{"lockfileVersion": 9}"#), None);
}
