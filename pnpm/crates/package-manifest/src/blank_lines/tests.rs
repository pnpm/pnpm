use super::BlankLines;
use pretty_assertions::assert_eq;
use serde_json::Value;

fn round_trip(source: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut value: Value = serde_json::from_str(source).unwrap();
    edit(&mut value);
    let serialized = serde_json::to_string_pretty(&value).unwrap();
    let restored = BlankLines::detect(source).restore(&serialized);
    eprintln!("RESTORED:\n{restored}");
    assert_eq!(serde_json::from_str::<Value>(&restored).unwrap(), value);
    restored
}

#[test]
fn keeps_blank_lines_between_top_level_and_nested_members() {
    let source = "{\n  \"name\": \"demo\",\n\n  \"scripts\": {\n    \"test\": \"a\",\n\n\n    \"foo\": \"bar\"\n  }\n}";
    let restored = round_trip(source, |value| {
        value["dependencies"] = serde_json::json!({ "axios": "^1.1.3" });
    });
    assert_eq!(
        restored,
        "{\n  \"name\": \"demo\",\n\n  \"scripts\": {\n    \"test\": \"a\",\n\n\n    \"foo\": \"bar\"\n  },\n  \"dependencies\": {\n    \"axios\": \"^1.1.3\"\n  }\n}",
    );
}

#[test]
fn drops_the_blank_line_of_a_removed_member() {
    let source = "{\n  \"name\": \"demo\",\n\n  \"version\": \"1.0.0\"\n}";
    let restored = round_trip(source, |value| {
        value
            .as_object_mut()
            .unwrap()
            .remove("version");
    });
    assert_eq!(restored, "{\n  \"name\": \"demo\"\n}");
}

#[test]
fn matches_members_by_path_not_by_key() {
    let source = "{\n  \"a\": {\n    \"x\": 1,\n\n    \"y\": 2\n  },\n  \"b\": {\n    \"x\": 1,\n    \"y\": 2\n  }\n}";
    let restored = round_trip(source, |_| {});
    assert_eq!(restored, source);
}

#[test]
fn tracks_members_inside_arrays_and_escaped_keys() {
    let source = "{\n  \"list\": [\n    {\n      \"a\": \"[,{\\\"\",\n\n      \"b\\u0063\": 1\n    }\n  ]\n}";
    let restored = round_trip(source, |_| {});
    assert_eq!(restored, source.replace(r"b\u0063", "bc"));
}

#[test]
fn leaves_a_single_line_document_alone() {
    let source = "{\n  \"a\": 1,\n\n  \"b\": 2\n}";
    let compact = r#"{"a":1,"b":2}"#;
    assert_eq!(BlankLines::detect(source).restore(compact), compact);
}

#[test]
fn counts_crlf_line_breaks() {
    let source = "{\r\n  \"a\": 1,\r\n\r\n  \"b\": 2\r\n}";
    assert_eq!(round_trip(source, |_| {}), "{\n  \"a\": 1,\n\n  \"b\": 2\n}");
}
