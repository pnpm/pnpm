use super::{resolves_float, to_string};
use serde_json::json;

#[test]
fn version_like_floats_are_single_quoted() {
    let yaml = to_string(&json!({ "a": "9.0", "b": "11.5.2", "c": "1.0.0" })).unwrap();
    assert_eq!(yaml, "a: '9.0'\n\nb: 11.5.2\n\nc: 1.0.0\n");
}

#[test]
fn leading_indicator_and_at_force_single_quotes() {
    let yaml = to_string(&json!({
        "node": ">=10",
        "name": "@scope/pkg@1.0.0",
    }))
    .unwrap();
    assert_eq!(yaml, "name: '@scope/pkg@1.0.0'\n\nnode: '>=10'\n");
}

#[test]
fn booleans_and_numbers_render_plain() {
    let yaml = to_string(&json!({ "hasBin": true, "max": 1000 })).unwrap();
    assert_eq!(yaml, "hasBin: true\n\nmax: 1000\n");
}

#[test]
fn ambiguous_words_are_quoted() {
    let yaml = to_string(&json!({ "a": "true", "b": "null", "c": "yes", "d": "no" })).unwrap();
    assert_eq!(yaml, "a: 'true'\n\nb: 'null'\n\nc: yes\n\nd: no\n");
}

#[test]
fn blank_lines_between_top_level_and_named_section_entries() {
    let yaml = to_string(&json!({
        "lockfileVersion": "9.0",
        "packages": {
            "a@1.0.0": { "x": 1 },
            "b@2.0.0": { "y": 2 },
        },
    }))
    .unwrap();
    assert_eq!(
        yaml,
        "lockfileVersion: '9.0'\n\npackages:\n\n  a@1.0.0:\n    x: 1\n\n  b@2.0.0:\n    y: 2\n",
    );
}

#[test]
fn single_line_keys_render_flow() {
    let yaml = to_string(&json!({
        "resolution": { "integrity": "sha512-abc" },
        "engines": { "node": ">=10" },
        "cpu": ["x64"],
        "os": ["darwin", "linux"],
    }))
    .unwrap();
    assert_eq!(
        yaml,
        "cpu: [x64]\n\nengines: {node: '>=10'}\n\nos: [darwin, linux]\n\nresolution: {integrity: sha512-abc}\n",
    );
}

#[test]
fn variations_resolution_renders_block_not_flow() {
    let yaml = to_string(&json!({
        "resolution": { "type": "variations", "variants": ["a"] },
    }))
    .unwrap();
    assert_eq!(yaml, "resolution:\n  type: variations\n  variants:\n    - a\n");
}

#[test]
fn nan_resolves_as_float() {
    assert!(resolves_float(".nan"));
    assert!(resolves_float("-.inf"));
    assert!(resolves_float("3.14"));
    assert!(!resolves_float("3.14.15"));
}

#[test]
fn timestamp_strings_are_quoted() {
    let yaml = to_string(&json!({ "t": "2021-01-01" })).unwrap();
    assert_eq!(yaml, "t: '2021-01-01'\n");
}
