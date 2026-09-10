use super::{IndexMap, TempDir, WORKSPACE_MANIFEST_FILENAME, fs, patched_deps, run_patched_deps};

#[test]
fn patched_dependency_creates_block_when_absent() {
    let out = run_patched_deps(None, &[("is-positive@1.0.0", "patches/is-positive@1.0.0.patch")]);
    assert_eq!(out, "patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n");
}

#[test]
fn patched_dependency_quotes_scoped_keys_and_slash_paths() {
    let out = run_patched_deps(
        None,
        &[("@pnpm.e2e/console-log", "patches/@pnpm.e2e__console-log.patch")],
    );
    assert_eq!(
        out,
        "patchedDependencies:\n  '@pnpm.e2e/console-log': patches/@pnpm.e2e__console-log.patch\n",
    );
}

#[test]
fn patched_dependency_noops_when_unchanged() {
    use crate::{edit, model::Manifest};

    let original = "patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n";
    let deps = patched_deps(&[("is-positive@1.0.0", "patches/is-positive@1.0.0.patch")]);
    let mut manifest = Manifest::parse(Some(original)).unwrap();
    assert!(
        !edit::add_patched_dependencies(&mut manifest, &deps).unwrap(),
        "re-adding the same patch entry should report no change",
    );
    assert_eq!(manifest.into_text(), original);
}

#[test]
fn patched_dependency_removes_omitted_entries() {
    let original = "packages:\n  - '*'\n\npatchedDependencies:\n  is-negative@1.0.0: patches/is-negative@1.0.0.patch\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(
        Some(original),
        &[("is-positive@1.0.0", "patches/is-positive@1.0.0.patch")],
    );

    assert_eq!(
        out,
        "packages:\n  - '*'\n\npatchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n\ncatalog:\n  react: 18.2.0\n",
    );
}

#[test]
fn patched_dependency_removes_empty_block() {
    let original = "packages:\n  - '*'\n\npatchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(Some(original), &[]);

    assert_eq!(out, "packages:\n  - '*'\n\ncatalog:\n  react: 18.2.0\n");
}

#[test]
fn patched_dependency_remove_preserves_successor_comments() {
    let original = "packages:\n  - '*'\n\npatchedDependencies:\n  is-positive: patches/is-positive.patch\n\n# catalog pins\ncatalog:\n  react: 18.2.0\n";
    let out = run_patched_deps(Some(original), &[]);

    assert_eq!(out, "packages:\n  - '*'\n\n# catalog pins\ncatalog:\n  react: 18.2.0\n");
}

#[test]
fn patched_dependency_removes_empty_last_block() {
    let original = "packages:\n  - '*'\n\npatchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n";
    let out = run_patched_deps(Some(original), &[]);

    assert_eq!(out, "packages:\n  - '*'\n");
}

#[test]
fn patched_dependency_missing_decoded_block_returns_original_text_when_removing_block() {
    use crate::{edit, model::Manifest};

    let original = "packages:\n  - '*'\n";
    let mut manifest = Manifest::parse(Some(original)).unwrap();
    manifest.patched_dependencies = Some(IndexMap::from([(
        "is-positive".to_string(),
        "patches/is-positive.patch".to_string(),
    )]));

    assert!(edit::add_patched_dependencies(&mut manifest, &IndexMap::new()).unwrap());
    assert_eq!(manifest.into_text(), original);
}

#[test]
fn patched_dependency_missing_decoded_mapping_keeps_text_before_inserting_new_block() {
    use crate::{edit, model::Manifest};

    let original = "packages:\n  - '*'\n";
    let mut manifest = Manifest::parse(Some(original)).unwrap();
    manifest.patched_dependencies = Some(IndexMap::from([(
        "is-negative".to_string(),
        "patches/is-negative.patch".to_string(),
    )]));
    let deps = patched_deps(&[("is-positive", "patches/is-positive.patch")]);

    assert!(edit::add_patched_dependencies(&mut manifest, &deps).unwrap());

    let text = manifest.into_text();
    assert!(text.contains("packages:\n  - '*'\n"), "text: {text}");
    assert!(text.contains("is-positive: patches/is-positive.patch"), "text: {text}");
    assert!(!text.contains("is-negative"), "text: {text}");
}

#[test]
fn set_patched_dependencies_reports_read_errors() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::create_dir(&path).expect("create manifest dir");

    let err = crate::set_patched_dependencies(
        dir.path(),
        &patched_deps(&[("is-positive", "patches/is-positive.patch")]),
    )
    .expect_err("manifest directory should fail to read");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::Read { .. }));
}
