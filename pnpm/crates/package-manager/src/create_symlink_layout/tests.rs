use crate::{
    SkippedSnapshots, SymlinkPackageError, VirtualStoreLayout, create_symlink_layout,
    materialize_global_virtual_store_context,
};
use pacquet_config::Config;
use pacquet_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgName, RegistryResolution, SnapshotDepRef,
    SnapshotEntry,
};
use pretty_assertions::assert_eq;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
    process::Command,
};
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
        pacquet_config::default_virtual_store_dir_max_length() as usize,
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
fn skips_optional_siblings_that_are_in_skipped() {
    let tmp = tempdir().expect("tempdir");
    let virtual_store_dir = tmp.path().to_path_buf();
    let layout = VirtualStoreLayout::legacy(
        virtual_store_dir,
        pacquet_config::default_virtual_store_dir_max_length() as usize,
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
        pacquet_config::default_virtual_store_dir_max_length() as usize,
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
        pacquet_config::default_virtual_store_dir_max_length() as usize,
    );
    let skipped = SkippedSnapshots::default();
    let virtual_node_modules_dir = tmp.path().join("self/node_modules");
    fs::create_dir_all(&virtual_node_modules_dir).unwrap();

    create_symlink_layout(
        None,
        None,
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
        pacquet_config::default_virtual_store_dir_max_length() as usize,
    );

    let mut deps: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    deps.insert(pkg_name("string-width-cjs"), dep_ref("string-width@4.2.3"));

    let skipped = SkippedSnapshots::default();
    let virtual_node_modules_dir = tmp.path().join("self/node_modules");
    fs::create_dir_all(&virtual_node_modules_dir).unwrap();

    create_symlink_layout(
        Some(&deps),
        None,
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
        pacquet_config::default_virtual_store_dir_max_length() as usize,
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

#[test]
fn context_projection_resolves_for_cjs_and_esm_with_declared_child_precedence() {
    let tmp = tempdir().expect("tempdir");
    let mut config = Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = tmp.path().join("store/links");
    config.virtual_store_dir = tmp.path().join("node_modules/.pnpm");

    let consumer: PackageKey = "consumer@1.0.0".parse().unwrap();
    let ambient_consumer: PackageKey = "ambient-consumer@1.0.0".parse().unwrap();
    let declared: PackageKey = "ambient@1.0.0".parse().unwrap();
    let projected: PackageKey = "ambient@2.0.0".parse().unwrap();
    let snapshots = HashMap::from([
        (consumer.clone(), SnapshotEntry::default()),
        (ambient_consumer.clone(), SnapshotEntry::default()),
        (declared.clone(), SnapshotEntry::default()),
        (projected.clone(), SnapshotEntry::default()),
    ]);
    let metadata = |integrity: &str| PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: integrity.parse().expect("parse integrity"),
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    };
    let packages = HashMap::from([
        (
            consumer.clone(),
            metadata(
                "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            ),
        ),
        (
            ambient_consumer.clone(),
            metadata(
                "sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD",
            ),
        ),
        (
            declared.clone(),
            metadata(
                "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
            ),
        ),
        (
            projected.clone(),
            metadata(
                "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
            ),
        ),
    ]);
    let projection = BTreeMap::from([("ambient".to_string(), projected.clone())]);
    let layout = VirtualStoreLayout::new(
        &config,
        None,
        Some(&snapshots),
        Some(&packages),
        None,
        Some(&projection),
    );

    let package_dir =
        |key: &PackageKey| layout.slot_dir(key).join("node_modules").join(key.name.to_string());
    let consumer_dir = package_dir(&consumer);
    let ambient_consumer_dir = package_dir(&ambient_consumer);
    let declared_dir = package_dir(&declared);
    let projected_dir = package_dir(&projected);
    for dir in [&consumer_dir, &ambient_consumer_dir, &declared_dir, &projected_dir] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(
        declared_dir.join("package.json"),
        br#"{"name":"ambient","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(declared_dir.join("index.js"), b"module.exports = 'declared'").unwrap();
    fs::write(
        projected_dir.join("package.json"),
        br#"{"name":"ambient","version":"2.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(projected_dir.join("index.js"), b"module.exports = 'projected'").unwrap();
    fs::write(consumer_dir.join("package.json"), br#"{"name":"consumer","version":"1.0.0"}"#)
        .unwrap();
    fs::write(consumer_dir.join("cjs.cjs"), b"process.stdout.write(require('ambient'))").unwrap();
    fs::write(
        consumer_dir.join("esm.mjs"),
        b"import ambient from 'ambient'; process.stdout.write(ambient)",
    )
    .unwrap();
    fs::write(
        ambient_consumer_dir.join("package.json"),
        br#"{"name":"ambient-consumer","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(ambient_consumer_dir.join("cjs.cjs"), b"process.stdout.write(require('ambient'))")
        .unwrap();
    fs::write(
        ambient_consumer_dir.join("esm.mjs"),
        b"import ambient from 'ambient'; process.stdout.write(ambient)",
    )
    .unwrap();

    let dependencies = HashMap::from([(pkg_name("ambient"), dep_ref("1.0.0"))]);
    create_symlink_layout(
        Some(&dependencies),
        None,
        &pkg_name("consumer"),
        &SkippedSnapshots::new(),
        &layout,
        &layout.slot_dir(&consumer).join("node_modules"),
    )
    .unwrap();
    materialize_global_virtual_store_context(&layout, &SkippedSnapshots::new(), &[]).unwrap();

    for (package_dir, expected) in
        [(&consumer_dir, "declared"), (&ambient_consumer_dir, "projected")]
    {
        for entrypoint in ["cjs.cjs", "esm.mjs"] {
            let output = Command::new("node").arg(package_dir.join(entrypoint)).output().unwrap();
            assert!(
                output.status.success(),
                "{entrypoint} failed: {}",
                String::from_utf8_lossy(&output.stderr),
            );
            assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
        }
    }
}
