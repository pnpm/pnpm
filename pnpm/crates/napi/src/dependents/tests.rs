use std::path::Path;

use pretty_assertions::assert_eq;
use serde_json::{Value, json};

use super::{DependentsOptions, RenderDependentsInput, build_trees, render_dependents};

/// A workspace whose root project depends on `dep@1.0.0`, which in turn
/// depends on `nested@2.0.0`. Only the lockfile and the manifests the tree
/// walk reads are written — the walk never resolves or fetches.
fn write_workspace(dir: &Path) {
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"root-project","version":"1.0.0","dependencies":{"dep":"1.0.0"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("pnpm-lock.yaml"),
        r"lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      dep:
        specifier: 1.0.0
        version: 1.0.0

packages:

  dep@1.0.0:
    resolution: {integrity: sha512-dep}

  nested@2.0.0:
    resolution: {integrity: sha512-nested}

snapshots:

  dep@1.0.0:
    dependencies:
      nested: 2.0.0

  nested@2.0.0: {}
",
    )
    .unwrap();
    let nested_dir = dir.join("node_modules/.pnpm/nested@2.0.0/node_modules/nested");
    std::fs::create_dir_all(&nested_dir).unwrap();
    std::fs::write(
        nested_dir.join("package.json"),
        r#"{"name":"nested","version":"2.0.0","componentId":{"scope":"acme.utils","name":"nested"}}"#,
    )
    .unwrap();
}

fn options(dir: &Path, packages: &[&str]) -> DependentsOptions {
    DependentsOptions {
        dir: dir.to_string_lossy().into_owned(),
        packages: packages.iter().map(|name| (*name).to_string()).collect(),
        project_dirs: None,
        exclude_project_patterns: None,
        modules_dir: None,
        include_dependencies: None,
        include_dev_dependencies: None,
        include_optional_dependencies: None,
        registries: None,
        virtual_store_dir_max_length: None,
        manifest_fields: None,
    }
}

#[test]
fn reports_the_chain_from_the_importer_down_to_the_searched_package() {
    let workspace = tempfile::tempdir().unwrap();
    write_workspace(workspace.path());

    let trees = build_trees(&options(workspace.path(), &["nested"])).unwrap();

    assert_eq!(trees.len(), 1);
    assert_eq!(trees[0].name, "nested");
    assert_eq!(trees[0].version, "2.0.0");
    let dependents = &trees[0].dependents;
    assert_eq!(dependents.len(), 1);
    assert_eq!(dependents[0].name, "dep");
    let via_dep = dependents[0].dependents.as_ref().expect("dep has a dependent of its own");
    assert_eq!(via_dep.len(), 1);
    assert_eq!(via_dep[0].name, "root-project");
}

/// The excluded importer is the only one that reaches `dep`, so excluding
/// it leaves nothing to report — proof the pattern is applied to the walk
/// roots rather than only to the rendered output.
#[test]
fn excluded_importers_are_not_walked() {
    let workspace = tempfile::tempdir().unwrap();
    write_workspace(workspace.path());
    let mut opts = options(workspace.path(), &["nested"]);
    opts.exclude_project_patterns = Some(vec![".".to_string()]);

    let trees = build_trees(&opts).unwrap();

    assert!(trees.is_empty());
}

#[test]
fn a_package_no_importer_reaches_has_no_tree() {
    let workspace = tempfile::tempdir().unwrap();
    write_workspace(workspace.path());

    let trees = build_trees(&options(workspace.path(), &["absent"])).unwrap();

    assert!(trees.is_empty());
}

#[test]
fn a_directory_without_a_lockfile_reports_no_dependents() {
    let workspace = tempfile::tempdir().unwrap();

    let trees = build_trees(&options(workspace.path(), &["nested"])).unwrap();

    assert!(trees.is_empty());
}

#[test]
fn manifest_fields_are_projected_onto_the_matched_package() {
    let workspace = tempfile::tempdir().unwrap();
    write_workspace(workspace.path());
    let mut opts = options(workspace.path(), &["nested"]);
    opts.manifest_fields = Some(vec!["componentId".to_string()]);

    let trees = build_trees(&opts).unwrap();

    assert_eq!(
        trees[0].manifest.as_ref().and_then(|manifest| manifest.get("componentId")),
        Some(&json!({ "scope": "acme.utils", "name": "nested" })),
    );
}

#[test]
fn without_manifest_fields_no_manifest_is_read() {
    let workspace = tempfile::tempdir().unwrap();
    write_workspace(workspace.path());

    let trees = build_trees(&options(workspace.path(), &["nested"])).unwrap();

    assert!(trees[0].manifest.is_none());
}

#[test]
fn render_uses_the_display_name_the_caller_wrote_back() {
    let trees = json!([{
        "name": "nested",
        "version": "2.0.0",
        "displayName": "acme.utils/nested",
        "dependents": [{ "name": "dep", "version": "1.0.0", "depField": "dependencies" }],
    }]);

    let rendered = render_dependents(
        trees,
        Some(RenderDependentsInput { format: None, depth: None, long: None }),
    )
    .unwrap();

    assert!(rendered.contains("acme.utils/nested@2.0.0"), "rendered: {rendered}");
    assert!(rendered.contains("dep@1.0.0"), "rendered: {rendered}");
}

#[test]
fn render_round_trips_the_json_format() {
    let trees = json!([{ "name": "nested", "version": "2.0.0", "dependents": [] }]);

    let rendered = render_dependents(
        trees,
        Some(RenderDependentsInput { format: Some("json".to_string()), depth: None, long: None }),
    )
    .unwrap();

    let parsed: Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(parsed[0]["name"], "nested");
}

/// A tree deeper than the walk could ever produce is refused rather than
/// recursed into: deserialization and the renderers both recurse over
/// `dependents`, and exhausting the stack takes the host process with it.
#[test]
fn an_over_deep_tree_is_rejected_instead_of_recursed_into() {
    let mut node = json!({ "name": "leaf", "version": "1.0.0" });
    for _ in 0..(pnpm_deps_inspection::MAX_WALK_DEPTH + 2) {
        node = json!({ "name": "n", "version": "1.0.0", "dependents": [node] });
    }

    let error = render_dependents(json!([node]), None).unwrap_err();

    assert!(error.reason.contains("nests dependents more than"), "{}", error.reason);
}

/// The boundary itself: a tree nested exactly as deep as the walk can go
/// is accepted, so the guard rejects only what a real tree could not be.
#[test]
fn a_tree_at_the_depth_limit_still_renders() {
    let mut node = json!({ "name": "leaf", "version": "1.0.0" });
    for _ in 0..pnpm_deps_inspection::MAX_WALK_DEPTH {
        node = json!({ "name": "n", "version": "1.0.0", "dependents": [node] });
    }

    let rendered = render_dependents(json!([node]), None).unwrap();

    assert!(rendered.contains("leaf@1.0.0"), "rendered: {rendered}");
}

#[test]
fn an_unknown_render_format_is_rejected() {
    let error = render_dependents(
        json!([]),
        Some(RenderDependentsInput { format: Some("yaml".to_string()), depth: None, long: None }),
    )
    .unwrap_err();

    assert!(error.reason.contains("unknown dependents render format"), "{}", error.reason);
}
