use super::root_settings_cover;
use std::fs;

fn cover(root_manifest: &str, nested_manifest: &str) -> bool {
    let workspace = tempfile::tempdir().unwrap();
    let nested = workspace.path().join("libs/lib");
    fs::create_dir_all(&nested).unwrap();
    fs::write(workspace.path().join("pnpm-workspace.yaml"), root_manifest).unwrap();
    fs::write(nested.join("pnpm-workspace.yaml"), nested_manifest).unwrap();
    root_settings_cover(workspace.path(), &nested).unwrap()
}

#[test]
fn settings_the_root_repeats_are_covered() {
    assert!(cover(
        "packages:\n  - libs/*\nallowBuilds:\n  bufferutil: true\n  esbuild: true\n",
        "allowBuilds:\n  bufferutil: true\n",
    ));
}

#[test]
fn the_nested_packages_field_is_not_a_setting() {
    assert!(cover("packages:\n  - libs/*\n", "packages:\n  - \"!**\"\n"));
}

#[test]
fn a_list_is_covered_by_a_list_holding_its_items() {
    assert!(cover(
        "publicHoistPattern:\n  - '*eslint*'\n  - '*types*'\n",
        "publicHoistPattern:\n  - '*types*'\n",
    ));
    assert!(!cover(
        "publicHoistPattern:\n  - '*eslint*'\n",
        "publicHoistPattern:\n  - '*types*'\n"
    ));
}

#[test]
fn a_setting_the_root_lacks_is_not_covered() {
    assert!(!cover("packages:\n  - libs/*\n", "overrides:\n  is-odd: 3.0.1\n"));
}

#[test]
fn a_setting_the_root_sets_differently_is_not_covered() {
    assert!(!cover("allowBuilds:\n  bufferutil: false\n", "allowBuilds:\n  bufferutil: true\n"));
    assert!(!cover("nodeLinker: hoisted\n", "nodeLinker: isolated\n"));
}

#[test]
fn a_missing_nested_manifest_is_covered() {
    let workspace = tempfile::tempdir().unwrap();
    fs::write(workspace.path().join("pnpm-workspace.yaml"), "").unwrap();
    assert!(root_settings_cover(workspace.path(), &workspace.path().join("absent")).unwrap());
}

#[test]
fn a_missing_root_manifest_covers_only_an_empty_one() {
    let workspace = tempfile::tempdir().unwrap();
    let nested = workspace.path().join("lib");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("pnpm-workspace.yaml"), "packages: []\n").unwrap();
    assert!(root_settings_cover(workspace.path(), &nested).unwrap());
    fs::write(nested.join("pnpm-workspace.yaml"), "nodeLinker: hoisted\n").unwrap();
    assert!(!root_settings_cover(workspace.path(), &nested).unwrap());
}
