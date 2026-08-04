use super::{PackedPkgInfo, create_publish_summary, extract_bundled_dependencies};
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn builds_summary_with_hashes() {
    let manifest = json!({ "name": "@scope/pkg", "version": "1.2.3" });
    let contents = vec!["package.json".to_owned(), "index.js".to_owned()];
    let info = PackedPkgInfo {
        published_manifest: &manifest,
        tarball_path: "/tmp/scope-pkg-1.2.3.tgz",
        contents: &contents,
        unpacked_size: 42,
    };
    let summary = create_publish_summary(&info, b"hello world");
    assert_eq!(summary.id, "@scope/pkg@1.2.3");
    assert_eq!(summary.filename, "scope-pkg-1.2.3.tgz");
    assert_eq!(summary.entry_count, 2);
    assert_eq!(summary.size, 11);
    assert_eq!(summary.shasum, "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed");
    assert!(summary.integrity.starts_with("sha512-"));
}

#[test]
fn bundled_true_expands_to_dependency_names() {
    let manifest = json!({
        "bundleDependencies": true,
        "dependencies": { "a": "1", "b": "2" },
    });
    let mut names = extract_bundled_dependencies(&manifest);
    names.sort();
    assert_eq!(names, vec!["a".to_owned(), "b".to_owned()]);
}

#[test]
fn bundled_array_filters_to_strings() {
    let manifest = json!({ "bundledDependencies": ["a", 5, "b"] });
    assert_eq!(extract_bundled_dependencies(&manifest), vec!["a".to_owned(), "b".to_owned()]);
}

#[test]
fn null_bundled_dependencies_falls_back_to_bundle_dependencies() {
    let manifest = json!({
        "bundledDependencies": null,
        "bundleDependencies": ["a", "b"],
    });
    assert_eq!(extract_bundled_dependencies(&manifest), vec!["a".to_owned(), "b".to_owned()]);
}
