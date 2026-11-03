use super::sync;
use crate::json5::parse;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

fn edit(source: &str, change: impl FnOnce(&mut Value)) -> String {
    let original = parse(source).unwrap();
    let mut target = original.clone();
    change(&mut target);
    let edited = sync(source, &original, &target).expect("the CST parses the source");
    eprintln!("EDITED:\n{edited}");
    assert_eq!(parse(&edited).unwrap(), target);
    edited
}

#[test]
fn changes_only_the_changed_string_and_keeps_its_quote() {
    let source = "// top\n{\n  name: 'fixture', // name note\n  version: '1.0.0',\n  \"custom\": -0x10,\n  quoted: \"it's\",\n}\n";
    let edited = edit(source, |value| {
        value["version"] = json!("2.0.0");
        value["quoted"] = json!(r#"it's "new""#);
    });
    assert_eq!(
        edited,
        "// top\n{\n  name: 'fixture', // name note\n  version: '2.0.0',\n  \"custom\": -0x10,\n  quoted: \"it's \\\"new\\\"\",\n}\n",
    );
}

#[test]
fn removes_a_property_and_keeps_the_comments_of_its_neighbours() {
    let source = "{\n  dependencies: {\n    alpha: '1.0.0', // alpha note\n    obsolete: '1.0.0',\n    // zulu note\n    zulu: '1.0.0',\n  },\n}";
    let edited = edit(source, |value| {
        value["dependencies"]
            .as_object_mut()
            .unwrap()
            .remove("obsolete");
    });
    assert_eq!(
        edited,
        "{\n  dependencies: {\n    alpha: '1.0.0', // alpha note\n    // zulu note\n    zulu: '1.0.0',\n  },\n}",
    );
}

#[test]
fn inserts_a_new_key_after_the_key_that_precedes_it_in_the_target() {
    let source = "{\n  dependencies: {\n    alpha: '1.0.0',\n    charlie: '1.0.0',\n  },\n}";
    let edited = edit(source, |value| {
        value["dependencies"] = json!({"alpha": "1.0.0", "bravo": "1.0.0", "charlie": "1.0.0"});
    });
    assert_eq!(
        edited,
        "{\n  dependencies: {\n    alpha: '1.0.0',\n    bravo: '1.0.0',\n    charlie: '1.0.0',\n  },\n}",
    );
}

#[test]
fn keeps_the_order_on_disk_and_chains_new_keys_after_their_predecessor() {
    let source = "{\n  deps: {\n    zeta: '1',\n    alpha: '1',\n    delta: '1',\n  },\n}";
    let edited = edit(source, |value| {
        value["deps"] =
            json!({"alpha": "1", "beta": "1", "charlie": "1", "delta": "1", "zeta": "1"});
    });
    assert_eq!(
        edited,
        "{\n  deps: {\n    zeta: '1',\n    alpha: '1',\n    beta: '1',\n    charlie: '1',\n    delta: '1',\n  },\n}",
    );
}

#[test]
fn new_keys_and_strings_follow_the_quoting_of_the_file() {
    let bare = "{\n  name: 'fixture',\n}";
    let edited = edit(bare, |value| {
        value["dependencies"] = json!({"@scope/pkg": "1.0.0", "is_odd": "it's"});
    });
    assert_eq!(
        edited,
        "{\n  name: 'fixture',\n  dependencies: {\n    '@scope/pkg': '1.0.0',\n    is_odd: 'it\\'s',\n  },\n}",
    );
    let quoted = "{\n  \"name\": \"fixture\"\n}";
    let edited = edit(quoted, |value| value["version"] = json!("1.0.0"));
    assert_eq!(edited, "{\n  \"name\": \"fixture\",\n  \"version\": \"1.0.0\"\n}");
}

#[test]
fn keeps_comments_in_a_container_that_empties_or_fills() {
    let emptied = edit("{\n  overrides: {\n    // note\n    foo: '1',\n  },\n}", |value| {
        value["overrides"] = json!({});
    });
    assert_eq!(emptied, "{\n  overrides: {\n    // note\n  },\n}");
    let filled = edit("{\n  files: [\n    // note\n  ],\n}", |value| {
        value["files"] = json!(["dist"]);
    });
    assert!(filled.contains("// note"), "{filled}");
}

#[test]
fn replaces_a_value_of_another_type_in_the_file_style() {
    let source = "{\n  flag: true,\n  count: 1,\n  empty: null,\n  name: 'x',\n  list: ['a'],\n  nested: { a: 1 },\n  gone: 'z',\n}";
    let edited = edit(source, |value| {
        value["flag"] = json!("yes");
        value["count"] = json!(2);
        value["empty"] = json!(["new"]);
        value["name"] = json!(5);
        value["list"] = json!({"new-key": "v"});
        value["nested"] = json!(false);
        value["gone"] = json!(null);
    });
    assert_eq!(
        edited,
        "{\n  flag: 'yes',\n  count: 2,\n  empty: ['new'],\n  name: 5,\n  list: {\n    'new-key': 'v'\n  },\n  nested: false,\n  gone: null,\n}",
    );
}

#[test]
fn grows_and_shrinks_arrays_in_place() {
    let source = "{\n  files: ['a', 'b', 'c'], // files note\n  keywords: ['x'],\n}";
    let edited = edit(source, |value| {
        value["files"] = json!(["a", "b"]);
        value["keywords"] = json!(["x", "y"]);
    });
    assert!(edited.contains("// files note"), "{edited}");
    assert!(edited.starts_with("{\n  files: ['a', 'b'],"), "{edited}");
}

#[test]
fn returns_none_for_json5_the_cst_cannot_parse() {
    let source = "{null_value: 1}";
    let original = parse(source).unwrap();
    assert_eq!(sync(source, &original, &json!({"null_value": 2})), None);
}
