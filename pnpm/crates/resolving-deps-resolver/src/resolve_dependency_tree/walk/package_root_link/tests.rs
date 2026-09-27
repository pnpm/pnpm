use super::{file_spec_to_package_root_link, link_file_deps_inside_package};
use pretty_assertions::assert_eq;
use std::sync::Arc;

#[test]
fn a_file_spec_inside_the_package_becomes_a_package_root_link() {
    for (spec, expected) in [
        ("file:./typings/css-tree", "link:<root>/typings/css-tree"),
        ("file:typings/css-tree", "link:<root>/typings/css-tree"),
        ("file:./a/../b/", "link:<root>/b"),
        (r"file:.\typings\css-tree", "link:<root>/typings/css-tree"),
    ] {
        assert_eq!(file_spec_to_package_root_link(spec).as_deref(), Some(expected), "{spec}");
    }
}

#[test]
fn a_spec_that_does_not_stay_inside_the_package_is_left_alone() {
    for spec in [
        "file:.",
        "file:./",
        "file:",
        "file:../sibling",
        "file:./a/../../sibling",
        "file:/abs/path",
        r"file:C:\abs\path",
        "file:~/dir",
        "file:./vendor/pkg-1.0.0.tgz",
        "file:child/C:/Users/Public",
        "link:./child",
        "^1.0.0",
    ] {
        assert_eq!(file_spec_to_package_root_link(spec), None, "{spec}");
    }
}

#[test]
fn only_the_file_deps_inside_the_package_are_rewritten() {
    let manifest = Arc::new(serde_json::json!({
        "name": "parent",
        "dependencies": { "child": "file:./child", "other": "^1.0.0" },
        "optionalDependencies": { "opt": "file:./opt" },
        "devDependencies": { "dev": "file:./dev" },
    }));

    let rewritten = link_file_deps_inside_package(manifest);

    assert_eq!(
        *rewritten,
        serde_json::json!({
            "name": "parent",
            "dependencies": { "child": "link:<root>/child", "other": "^1.0.0" },
            "optionalDependencies": { "opt": "link:<root>/opt" },
            "devDependencies": { "dev": "file:./dev" },
        }),
    );
}

#[test]
fn a_manifest_without_file_deps_keeps_its_arc() {
    let manifest = Arc::new(serde_json::json!({ "dependencies": { "other": "^1.0.0" } }));
    let rewritten = link_file_deps_inside_package(Arc::clone(&manifest));
    assert!(Arc::ptr_eq(&manifest, &rewritten));
}
