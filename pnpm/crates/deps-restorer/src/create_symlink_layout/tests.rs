use crate::{SkippedSnapshots, SymlinkPackageError, VirtualStoreLayout, create_symlink_layout};
use pnpm_lockfile::{PackageKey, PkgName, SnapshotDepRef};
use pretty_assertions::assert_eq;
use std::{collections::HashMap, fs, path::PathBuf};
use tempfile::tempdir;

fn pkg_name(input: &str) -> PkgName {
    PkgName::parse(input).expect("valid pkg name")
}

fn dep_ref(input: &str) -> SnapshotDepRef {
    input.parse().expect("valid snapshot dep ref")
}

/// A symlink in the slot's `node_modules` matches the alias and points
/// to `<layout.slot_dir(target)>/node_modules/<target-name>`. Trivial
/// path-shape assertion that anchors the rest of the test cases.
fn assert_symlink_shape(
    virtual_node_modules_dir: &std::path::Path,
    alias: &str,
    layout: &VirtualStoreLayout,
    target_key: &PackageKey,
) {
    let symlink_path = virtual_node_modules_dir.join(alias);
    let read = fs::read_link(&symlink_path)
        .unwrap_or_else(|err| panic!("read_link {symlink_path:?}: {err}"));
    let target_path =
        layout.slot_dir(target_key).join("node_modules").join(target_key.name.to_string());
    // pacquet writes the symlink contents as a path relative to the
    // link's parent dir. The expected on-disk contents are the same
    // relative form.
    let expected = pathdiff::diff_paths(&target_path, virtual_node_modules_dir)
        .expect("compute relative target");
    assert_eq!(read, expected);
}

/// `optionalDependencies` siblings whose target slot is **not** in
/// `skipped` get linked alongside the regular `dependencies` siblings.
/// This is the v11 install path: a snapshot like
/// `@typescript/native-preview` lists every platform variant under
/// `optionalDependencies`, and the installability pass leaves the
/// host-matching variant out of `skipped`. Without this, downstream
/// `getExePath`-style lookups fail because the matching binary slot
/// is missing from the consumer's slot `node_modules`.
#[test]
fn links_matching_optional_sibling_alongside_regular_deps() {
    let tmp = tempdir().expect("tempdir");
    let virtual_store_dir = tmp.path().to_path_buf();
    let layout = VirtualStoreLayout::legacy(
        virtual_store_dir,
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );

    let mut deps: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    deps.insert(pkg_name("plain-dep"), dep_ref("1.0.0"));

    let mut optional: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    optional.insert(pkg_name("matching-optional"), dep_ref("2.0.0"));

    let skipped = SkippedSnapshots::default();

    let virtual_node_modules_dir = tmp.path().join("self/node_modules");
    fs::create_dir_all(&virtual_node_modules_dir).unwrap();

    create_symlink_layout(
        Some(&deps),
        Some(&optional),
        true,
        &pkg_name("self"),
        &skipped,
        &layout,
        &virtual_node_modules_dir,
    )
    .expect("create_symlink_layout should succeed");

    assert_symlink_shape(
        &virtual_node_modules_dir,
        "plain-dep",
        &layout,
        &"plain-dep@1.0.0".parse().unwrap(),
    );
    assert_symlink_shape(
        &virtual_node_modules_dir,
        "matching-optional",
        &layout,
        &"matching-optional@2.0.0".parse().unwrap(),
    );
}

#[test]
fn excludes_optional_link_deps_when_optional_dependencies_are_disabled() {
    let tmp = tempdir().expect("tempdir");
    let lockfile_dir = tmp.path().join("project");
    fs::create_dir_all(&lockfile_dir).unwrap();
    let layout = VirtualStoreLayout::legacy(
        tmp.path().join("store"),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    )
    .with_lockfile_dir(&lockfile_dir);

    let deps = HashMap::from([(pkg_name("plain-dep"), dep_ref("1.0.0"))]);
    let optional = HashMap::from([(pkg_name("optional-link"), dep_ref("link:../optional-link"))]);
    let virtual_node_modules_dir = tmp.path().join("self/node_modules");

    create_symlink_layout(
        Some(&deps),
        Some(&optional),
        false,
        &pkg_name("self"),
        &SkippedSnapshots::default(),
        &layout,
        &virtual_node_modules_dir,
    )
    .expect("regular dependencies should still be linked");

    assert_symlink_shape(
        &virtual_node_modules_dir,
        "plain-dep",
        &layout,
        &"plain-dep@1.0.0".parse().unwrap(),
    );
    assert!(
        fs::symlink_metadata(virtual_node_modules_dir.join("optional-link")).is_err(),
        "an optional link must not be materialized under --no-optional",
    );
}

#[test]
fn skips_optional_siblings_that_are_in_skipped() {
    let tmp = tempdir().expect("tempdir");
    let virtual_store_dir = tmp.path().to_path_buf();
    let layout = VirtualStoreLayout::legacy(
        virtual_store_dir,
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );

    let mut optional: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    optional.insert(pkg_name("matching-optional"), dep_ref("2.0.0"));
    optional.insert(pkg_name("mismatched-optional"), dep_ref("3.0.0"));

    let mut skipped_set = std::collections::HashSet::<PackageKey>::new();
    skipped_set.insert("mismatched-optional@3.0.0".parse().unwrap());
    let skipped = SkippedSnapshots::from_set(skipped_set);

    let virtual_node_modules_dir = tmp.path().join("self/node_modules");
    fs::create_dir_all(&virtual_node_modules_dir).unwrap();

    create_symlink_layout(
        None,
        Some(&optional),
        true,
        &pkg_name("self"),
        &skipped,
        &layout,
        &virtual_node_modules_dir,
    )
    .expect("create_symlink_layout should succeed");

    // `symlink_metadata` reports the link itself, not the target —
    // crucial for this assertion because the slot the link points to
    // is never created in this test (the symlink is intentionally
    // dangling). `Path::exists()` would follow the link and return
    // false despite the link existing.
    assert!(
        fs::symlink_metadata(virtual_node_modules_dir.join("matching-optional"))
            .is_ok_and(|m| m.is_symlink()),
        "matching optional sibling must be linked",
    );
    assert!(
        fs::symlink_metadata(virtual_node_modules_dir.join("mismatched-optional")).is_err(),
        "skipped optional sibling must not be linked (would dangle)",
    );
}

/// A dep whose alias matches the slot's own package name is a
/// self-link that Node's resolver doesn't need, so it is excluded.
/// Tests both buckets — `dependencies` and `optionalDependencies` —
/// because either can list the self-name in the wild.
#[test]
fn skips_dep_entries_whose_alias_matches_self_name() {
    let tmp = tempdir().expect("tempdir");
    let virtual_store_dir = tmp.path().to_path_buf();
    let layout = VirtualStoreLayout::legacy(
        virtual_store_dir,
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );

    let mut deps: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    deps.insert(pkg_name("self"), dep_ref("1.0.0"));

    let mut optional: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    optional.insert(pkg_name("self"), dep_ref("1.0.0"));

    let skipped = SkippedSnapshots::default();
    let virtual_node_modules_dir = tmp.path().join("self/node_modules");
    fs::create_dir_all(&virtual_node_modules_dir).unwrap();

    create_symlink_layout(
        Some(&deps),
        Some(&optional),
        true,
        &pkg_name("self"),
        &skipped,
        &layout,
        &virtual_node_modules_dir,
    )
    .expect("create_symlink_layout should succeed");

    let entries: Vec<PathBuf> = fs::read_dir(&virtual_node_modules_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(entries.is_empty(), "self-named entries must not become symlinks; got {entries:?}");
}

#[test]
fn both_dep_maps_absent_is_a_noop() {
    let tmp = tempdir().expect("tempdir");
    let virtual_store_dir = tmp.path().to_path_buf();
    let layout = VirtualStoreLayout::legacy(
        virtual_store_dir,
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );
    let skipped = SkippedSnapshots::default();
    let virtual_node_modules_dir = tmp.path().join("self/node_modules");
    fs::create_dir_all(&virtual_node_modules_dir).unwrap();

    create_symlink_layout(
        None,
        None,
        true,
        &pkg_name("self"),
        &skipped,
        &layout,
        &virtual_node_modules_dir,
    )
    .expect("create_symlink_layout should succeed with no deps");

    let entries: Vec<_> = fs::read_dir(&virtual_node_modules_dir).unwrap().collect();
    assert!(entries.is_empty(), "no symlinks should be created when both dep maps are absent");
}

#[test]
fn alias_dep_links_under_alias_but_resolves_via_target() {
    let tmp = tempdir().expect("tempdir");
    let virtual_store_dir = tmp.path().to_path_buf();
    let layout = VirtualStoreLayout::legacy(
        virtual_store_dir,
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );

    let mut deps: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    deps.insert(pkg_name("string-width-cjs"), dep_ref("string-width@4.2.3"));

    let skipped = SkippedSnapshots::default();
    let virtual_node_modules_dir = tmp.path().join("self/node_modules");
    fs::create_dir_all(&virtual_node_modules_dir).unwrap();

    create_symlink_layout(
        Some(&deps),
        None,
        true,
        &pkg_name("self"),
        &skipped,
        &layout,
        &virtual_node_modules_dir,
    )
    .expect("create_symlink_layout should succeed");

    let symlink_path = virtual_node_modules_dir.join("string-width-cjs");
    let read = fs::read_link(&symlink_path).expect("read_link");
    let target_path = layout
        .slot_dir(&"string-width@4.2.3".parse().unwrap())
        .join("node_modules")
        .join("string-width");
    // The on-disk contents are the path from the link's parent dir
    // to the slot dir (relative encoding).
    let expected = pathdiff::diff_paths(&target_path, &virtual_node_modules_dir)
        .expect("compute relative target");
    assert_eq!(read, expected);
}

/// A dependency alias that is a scoped path traversal
/// (`@x/../../.../OUTSIDE`) must be rejected before any symlink is
/// created, rather than escaping the slot's `node_modules`.
/// `PkgName::parse` accepts such a name (its `bare` field keeps the
/// `../` segments), so the guard has to live at the join.
#[test]
fn rejects_traversal_dependency_alias() {
    let tmp = tempdir().expect("tempdir");
    let virtual_store_dir = tmp.path().to_path_buf();
    let layout = VirtualStoreLayout::legacy(
        virtual_store_dir,
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );

    let traversal = format!("@x/{}OUTSIDE", "../".repeat(20));
    let mut deps: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    deps.insert(pkg_name(&traversal), dep_ref("1.0.0"));

    let skipped = SkippedSnapshots::default();
    let virtual_node_modules_dir = tmp.path().join("self/node_modules");
    fs::create_dir_all(&virtual_node_modules_dir).unwrap();

    let error = create_symlink_layout(
        Some(&deps),
        None,
        true,
        &pkg_name("self"),
        &skipped,
        &layout,
        &virtual_node_modules_dir,
    )
    .expect_err("traversal alias must be rejected");
    assert!(matches!(error, SymlinkPackageError::InvalidAlias(_)), "got {error:?}");

    // The guard fires before any symlink is created, so nothing was
    // linked into (or out of) the slot's node_modules.
    let linked = fs::read_dir(&virtual_node_modules_dir).unwrap().count();
    assert_eq!(linked, 0);
}

/// A `link:` dependency has no slot of its own, so the link inside the
/// consumer's slot has to point at the directory the lockfile names,
/// resolved against the lockfile dir.
///
/// Without this the dependency is absent from the slot entirely. That
/// stays invisible while the slot sits under the importer's
/// `node_modules` — Node's upward walk reaches the importer's own copy
/// of the link — and breaks the moment the global virtual store moves
/// the slot into the shared store, where no such walk exists.
#[test]
fn links_a_link_dep_to_its_target_outside_the_store() {
    let tmp = tempdir().expect("tempdir");
    let lockfile_dir = tmp.path().join("proj");
    let link_target = tmp.path().join("sibling");
    fs::create_dir_all(&lockfile_dir).unwrap();
    fs::create_dir_all(&link_target).unwrap();

    let layout = VirtualStoreLayout::legacy(
        tmp.path().to_path_buf(),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    )
    .with_lockfile_dir(&lockfile_dir);

    let mut deps: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    deps.insert(pkg_name("linked"), dep_ref("link:../sibling"));

    let virtual_node_modules_dir = tmp.path().join("self/node_modules");
    fs::create_dir_all(&virtual_node_modules_dir).unwrap();

    create_symlink_layout(
        Some(&deps),
        None,
        true,
        &pkg_name("self"),
        &SkippedSnapshots::default(),
        &layout,
        &virtual_node_modules_dir,
    )
    .expect("link dep must be materialized");

    let symlink_path = virtual_node_modules_dir.join("linked");
    let resolved = fs::canonicalize(&symlink_path)
        .unwrap_or_else(|err| panic!("canonicalize {symlink_path:?}: {err}"));
    assert_eq!(resolved, fs::canonicalize(&link_target).unwrap());
}

/// Without a lockfile directory there is nothing to resolve the
/// lockfile-relative target against, so the link is left alone rather
/// than guessed at — the pre-existing behaviour for every caller that
/// builds a layout with no lockfile context.
#[test]
fn skips_a_link_dep_when_no_lockfile_dir_is_known() {
    let tmp = tempdir().expect("tempdir");
    let layout = VirtualStoreLayout::legacy(
        tmp.path().to_path_buf(),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );

    let mut deps: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    deps.insert(pkg_name("linked"), dep_ref("link:../sibling"));

    let virtual_node_modules_dir = tmp.path().join("self/node_modules");
    fs::create_dir_all(&virtual_node_modules_dir).unwrap();

    create_symlink_layout(
        Some(&deps),
        None,
        true,
        &pkg_name("self"),
        &SkippedSnapshots::default(),
        &layout,
        &virtual_node_modules_dir,
    )
    .expect("no lockfile dir is not an error");

    assert_eq!(fs::read_dir(&virtual_node_modules_dir).unwrap().count(), 0);
}
