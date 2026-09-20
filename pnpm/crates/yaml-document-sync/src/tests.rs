use super::{parse, sync};
use serde_json::json;

#[test]
fn synchronizes_manifest_shapes() {
    let mut failures = Vec::new();
    for (source, target) in [
        ("# empty\n", json!({"name":"fixture"})),
        ("name: fixture\n", json!({})),
        ("name: fixture\ncustom: null # comment\n", json!({"name":"fixture","custom":{"one":1}})),
        ("name: fixture\ncustom:\n  one: 1\n", json!({"name":"fixture","custom":false})),
        ("name: fixture\ncustom: {}\n", json!({"name":"fixture","custom":{"one":1}})),
        (
            "name: fixture\ncustom: {one: 1, two: 2}\n",
            json!({"name":"fixture","custom":{"two":3,"three":4}}),
        ),
        (
            "name: fixture\ncustom: {\n  one: 1, # first\n  two: 2\n}\n",
            json!({"name":"fixture","custom":{"one":1,"two":2,"three":3}}),
        ),
        ("name: fixture\ncustom: [one, two]\n", json!({"name":"fixture","custom":["one"]})),
        (
            "name: fixture\ncustom:\n  - one # first\n  - two\n",
            json!({"name":"fixture","custom":["one","three"]}),
        ),
        (
            "name: fixture\ncustom:\n  - one # first\n",
            json!({"name":"fixture","custom":["one","two"]}),
        ),
        (
            "name: fixture\ndependencies:\n  one: 1.0.0\n",
            json!({"name":"fixture","dependencies":{"two":"2.0.0"}}),
        ),
        (
            "name: fixture\ncustom:\n  'a:b': old # comment\n",
            json!({"name":"fixture","custom":{"a:b":"new"}}),
        ),
        (
            "name: fixture\ncustom:\n  null: value\n  42: number\n",
            json!({"name":"fixture","custom":{"null":"value","42":"number"},"version":"1.0.0"}),
        ),
    ] {
        let output = match sync(source, &target) {
            Ok(output) => output,
            Err(error) => {
                failures.push(format!("{source:?}: {error}"));
                continue;
            }
        };
        assert_eq!(parse(&output).unwrap(), target, "{source:?}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn collection_aliases_are_independent_when_edited() {
    let source =
        "name: fixture\ndefaults: &deps\n  alpha: 1.0.0 # keep\ndependencies: *deps # reference\n";
    for target in [
        json!({"name":"fixture","defaults":{"alpha":"1.0.0"},"dependencies":{"alpha":"2.0.0"}}),
        json!({"name":"fixture","defaults":{"alpha":"2.0.0"},"dependencies":{"alpha":"1.0.0"}}),
        json!({"name":"fixture","dependencies":{"alpha":"1.0.0"}}),
    ] {
        let output = sync(source, &target).unwrap();
        assert_eq!(parse(&output).unwrap(), target);
        assert!(output.contains("# reference"), "{output}");
    }
    let mut target = parse(source).unwrap();
    target["name"] = json!("renamed");
    assert_eq!(sync(source, &target).unwrap(), source.replace("fixture", "renamed"));
}

#[test]
fn edits_do_not_inject_yaml_through_keys_or_values() {
    let source = "name: fixture\ncustom: {existing: true}\n";
    let target = json!({"name":"fixture","custom":{"existing":true,"a: b\nc: d":"false","comma,key":"one,two","array":"[one, two]"}});
    let output = sync(source, &target).unwrap();
    assert_eq!(parse(&output).unwrap(), target);
}

#[test]
fn unicode_line_separators_round_trip() {
    let target = json!({"name":"fixture","custom":{"separators":"line\u{0085}next\u{2028}paragraph\u{2029}end"}});
    let output = sync("name: fixture\n", &target).unwrap();
    assert_eq!(parse(&output).unwrap(), target);
}

#[test]
fn preserves_sequence_and_flow_comments() {
    for (source, target, expected) in [
        (
            "items:\n  - one # keep\n  - two # remove\n",
            json!({"items":["one"]}),
            "items:\n  - one # keep\n",
        ),
        (
            "custom: {\n  one: 1, # first\n  two: 2 # second\n}\n",
            json!({"custom":{"one":1,"two":2,"three":3}}),
            "custom: {\n  one: 1, # first\n  two: 2, # second\n  three: 3\n}\n",
        ),
    ] {
        assert_eq!(sync(source, &target).unwrap(), expected);
    }
}

#[test]
fn preserves_yaml_metadata_while_adding_a_version() {
    for source in [
        "# project\nname: fixture\ncustom:\n  date: 2026-01-01 # date\n  binary: !!binary SGVsbG8= # binary\n",
        "name: fixture\ncustom:\n  defaults: &defaults {one: 1}\n  merged:\n    <<: *defaults\n    two: 2\n",
        "name: fixture\ncustom:\n  42: number\n  false: boolean\n  null: null-key\n",
    ] {
        let mut target = parse(source).unwrap();
        target["version"] = json!("1.0.0");
        assert_eq!(sync(source, &target).unwrap(), format!("{source}version: 1.0.0\n"));
    }
}

#[test]
fn fills_empty_documents_without_discarding_markers_or_comments() {
    for (source, expected) in [
        ("---\n# project\n", "---\n# project\nname: fixture\n"),
        ("---\n# project\n...\n# trailing\n", "---\n# project\nname: fixture\n...\n# trailing\n"),
        ("%YAML 1.2\n---\n", "%YAML 1.2\n---\nname: fixture\n"),
    ] {
        assert_eq!(sync(source, &json!({"name":"fixture"})).unwrap(), expected);
    }
}

#[test]
fn preserves_explicit_scalar_tags_in_alias_groups() {
    let source =
        "name: fixture\nmetadata:\n  binary: &bytes !!binary SGVsbG8= # data\n  copy: *bytes\n";
    let mut target = parse(source).unwrap();
    target["version"] = json!("1.0.0");
    let output = sync(source, &target).unwrap();
    assert!(output.contains("!!binary"), "{output}");
    assert!(output.contains("copy: *bytes"), "{output}");
}

#[test]
fn changing_one_tagged_scalar_keeps_its_aliases_typed() {
    let source = "name: fixture\nmetadata:\n  binary: &bytes !!binary SGVsbG8= # data\n  copy: *bytes # copy\n";
    let mut target = parse(source).unwrap();
    target["metadata"]["binary"] = json!("changed");
    let output = sync(source, &target).unwrap();
    assert_eq!(parse(&output).unwrap(), target);
    assert!(output.contains("copy: !!binary SGVsbG8= # copy"), "{output}");
}

#[test]
fn tagged_multiline_alias_values_survive_unrelated_edits() {
    let source = "name: fixture\nmetadata:\n  binary: &bytes !!binary | # encoded\n    SGVsbG8=\n  copy: *bytes\n";
    let mut target = parse(source).unwrap();
    target["version"] = json!("1.0.0");
    let output = sync(source, &target).unwrap();
    assert_eq!(parse(&output).unwrap(), target);
    assert!(output.contains("!!binary"), "{output}");
    assert!(output.contains("copy: *bytes"), "{output}");
    assert_eq!(output, format!("{source}version: 1.0.0\n"));
}

#[test]
fn changing_an_alias_of_a_tagged_block_scalar_preserves_its_source() {
    let source = "name: fixture\nmetadata:\n  binary: &bytes !!binary | # encoded\n    SGVsbG8=\n  copy: *bytes\n";
    let mut target = parse(source).unwrap();
    target["metadata"]["copy"] = json!("changed");
    let output = sync(source, &target).unwrap();
    assert_eq!(parse(&output).unwrap(), target);
    assert!(output.contains("!!binary"), "{output}");
    assert!(output.contains("# encoded"), "{output}");
}

#[test]
fn batches_mixed_mapping_and_sequence_edits() {
    for source in [
        "dependencies: {a: old, b: old, c: old, d: old}\nitems: [a, b, c, d]\n",
        "dependencies:\n  a: old\n  b: old\n  c: old\n  d: old\nitems:\n  - a\n  - b\n  - c\n  - d\n",
    ] {
        for keep in 0..16 {
            let mut target = parse(source).unwrap();
            let dependencies = target["dependencies"].as_object_mut().unwrap();
            dependencies.retain(|key, value| {
                *value = json!("new");
                let index = "abcd".find(key.as_str()).unwrap();
                keep & (1 << index) != 0
            });
            dependencies.insert("e".to_string(), json!("added"));
            dependencies.insert("f".to_string(), json!("added"));
            target["items"] = json!(["changed"]);
            assert_eq!(parse(&sync(source, &target).unwrap()).unwrap(), target);
        }
    }
}

#[test]
fn batches_insertions_at_shared_container_boundaries() {
    let source = "name: fixture\nnested:\n  existing: old\n";
    let target =
        json!({"name":"fixture","nested":{"existing":"old","added":"child"},"added":"parent"});
    assert_eq!(
        sync(source, &target).unwrap(),
        "name: fixture\nnested:\n  existing: old\n  added: child\nadded: parent\n",
    );
}

#[test]
fn rejects_mapping_keys_that_collide_after_coercion() {
    for (key, quoted) in [
        ("null", "null"),
        ("true", "true"),
        ("42", "42"),
        ("~", "null"),
        ("TRUE", "true"),
        ("0x2a", "42"),
    ] {
        let source = format!("metadata:\n  {key}: first\n  '{quoted}': second\n");
        assert!(parse(&source).is_err(), "{source}");
    }
}
