use super::{json, packlist, tempdir, touch, write};
use serde_json::Value;
use std::path::Path;

fn packed(root: &Path, manifest: &Value) -> Vec<String> {
    let mut out = packlist(root, manifest).unwrap();
    out.sort();
    out
}

fn manifest() -> Value {
    json!({ "name": "x", "version": "0.0.0" })
}

fn names(expected: &[&str]) -> Vec<String> {
    expected
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn issue_tree_omits_swap_and_apple_double_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, ".index.js.swp");
    touch(root, "._index.js");
    touch(root, "lib/a.js");
    touch(root, "lib/._a.js");
    touch(root, "lib/.a.js.swp");

    assert_eq!(packed(root, &manifest()), names(&["index.js", "lib/a.js", "package.json"]));
}

#[test]
fn anchored_defaults_apply_at_depth() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "build/config.gypi");
    touch(root, "build/other.js");
    touch(root, "BUILD.js");
    touch(root, ".lock-wscript");
    touch(root, ".wafpickle-1");
    touch(root, "archived-packages/x");
    touch(root, "sub/build/config.gypi");
    touch(root, "sub/archived-packages/x");
    touch(root, "lib/.lock-wscript");
    touch(root, "lib/.wafpickle-2");
    touch(root, "lib/keep.js");

    assert_eq!(
        packed(root, &manifest()),
        names(&["BUILD.js", "build/other.js", "lib/keep.js", "package.json"]),
    );
}

#[test]
fn defaults_match_case_insensitively() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "a.js");
    touch(root, ".foo.SWP");
    touch(root, "._Upper");

    assert_eq!(packed(root, &manifest()), names(&["a.js", "package.json"]));
}

#[test]
fn apple_double_and_ds_store_directories_exclude_their_contents() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "a.js");
    touch(root, "._dir/inner.js");
    touch(root, "lib/._dir/inner.js");
    touch(root, "lib/.DS_Store/child.js");
    touch(root, "other/.DS_Store");

    assert_eq!(packed(root, &manifest()), names(&["a.js", "package.json"]));
}

#[test]
fn root_npmignore_negation_reincludes_root_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "._Root");
    touch(root, "build/config.gypi");
    touch(root, "lib/._a.js");
    write(root, ".npmignore", "!._*\n!/build/config.gypi\n!lib/._a.js\n");

    assert_eq!(packed(root, &manifest()), names(&["._Root", "build/config.gypi", "package.json"]));
}

#[test]
fn files_field_keeps_child_defaults_but_reincludes_root_ones() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, ".index.js.swp");
    touch(root, "._Root");
    touch(root, "build/config.gypi");
    touch(root, "lib/a.js");
    touch(root, "lib/._a.js");
    touch(root, "lib/.a.js.swp");

    let packed_with = |files: Value| {
        let manifest = json!({ "name": "x", "version": "0.0.0", "files": files });
        packed(root, &manifest)
    };

    assert_eq!(packed_with(json!(["lib"])), names(&["lib/a.js", "package.json"]));
    assert_eq!(
        packed_with(json!(["**"])),
        names(&["._Root", ".index.js.swp", "build/config.gypi", "lib/a.js", "package.json"]),
    );
    assert_eq!(packed_with(json!(["build"])), names(&["build/config.gypi", "package.json"]));
    assert_eq!(
        packed_with(json!(["*"])),
        names(&["._Root", ".index.js.swp", "build/config.gypi", "lib/a.js", "package.json"]),
    );
}

#[test]
fn files_field_naming_a_default_excluded_file_ships_it() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, ".lock-wscript");
    touch(root, "lib/._a.js");
    touch(root, "lib/b.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "files": ["lib/._a.js", ".lock-wscript"],
    });

    assert_eq!(packed(root, &manifest), names(&[".lock-wscript", "lib/._a.js", "package.json"]));
}

/// npm drops `sub/._dir/keep.js` even when `files` names it. Shipping it is
/// the existing behavior of the named-only walk, unchanged by the default rules.
#[test]
fn files_field_naming_a_file_in_an_apple_double_directory_ships_it() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "sub/._dir/keep.js");
    touch(root, "sub/._dir/drop.js");

    let manifest = json!({ "name": "x", "version": "0.0.0", "files": ["sub/._dir/keep.js"] });

    assert_eq!(packed(root, &manifest), names(&["package.json", "sub/._dir/keep.js"]));
}

#[test]
fn default_rules_match_ascii_case_only() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, ".loc\u{212A}-wscript");
    touch(root, "archived-pac\u{212A}ages/data.json");

    assert_eq!(
        packed(root, &manifest()),
        names(&[".loc\u{212A}-wscript", "archived-pac\u{212A}ages/data.json", "package.json"]),
    );
}

#[test]
fn default_rules_match_ascii_case_only_below_a_candidate_directory() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "config.gypi/.loc\u{212A}-wscript");
    touch(root, "config.gypi/.wafpic\u{212A}le-1");
    touch(root, "config.gypi/.lock-wscript");

    assert_eq!(
        packed(root, &manifest()),
        names(&[
            "config.gypi/.loc\u{212A}-wscript",
            "config.gypi/.wafpic\u{212A}le-1",
            "package.json",
        ]),
    );
}

#[test]
fn main_and_bin_reinclude_default_excluded_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "._main.js");
    touch(root, "build/config.gypi");
    touch(root, "._other.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "main": "._main.js",
        "bin": { "x": "build/config.gypi" },
    });

    assert_eq!(packed(root, &manifest), names(&["._main.js", "build/config.gypi", "package.json"]));
}

#[test]
fn empty_files_field_ships_only_package_json() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "._Root");
    touch(root, "lib/a.js");

    let manifest = json!({ "name": "x", "version": "0.0.0", "files": [] });

    assert_eq!(packed(root, &manifest), names(&["package.json"]));
}
