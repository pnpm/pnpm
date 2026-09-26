use super::{
    super::VirtualStoreLayout, LinkHashParityFixture, make_config, package_metadata,
    snapshot_with_link,
};
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, RegistryResolution, SnapshotEntry,
};
use pretty_assertions::{assert_eq, assert_ne};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[test]
fn slot_dir_engine_agnostic_with_empty_allow_build_policy() {
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let key: PackageKey = "left-pad@1.0.0".parse().unwrap();
    let mut packages = HashMap::new();
    packages.insert(
        key.clone(),
        PackageMetadata {
            resolution: LockfileResolution::Registry(RegistryResolution {
                integrity: "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
                    .parse()
                    .expect("parse integrity"),
                revision: None,
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
        },
    );
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let policy = crate::AllowBuildPolicy::default();
    let darwin = VirtualStoreLayout::new(
        &config,
        Some("darwin-arm64-node20"),
        Some(&snapshots),
        Some(&packages),
        Some(&policy),
        None,
    )
    .slot_dir(&key);
    let linux = VirtualStoreLayout::new(
        &config,
        Some("linux-x64-node22"),
        Some(&snapshots),
        Some(&packages),
        Some(&policy),
        None,
    )
    .slot_dir(&key);
    assert_eq!(
        darwin, linux,
        "pure-JS snapshot must share one GVS slot across engines when gating is active",
    );
}
#[test]
fn full_pkg_id_keeps_patch_hash_when_present() {
    let patched_key: PackageKey =
        "foo@1.0.0(patch_hash=abc)(react@18.0.0)".parse().expect("parse patched key");
    let metadata_key = patched_key.without_peer();
    let mut packages = HashMap::new();
    packages.insert(
        metadata_key,
        PackageMetadata {
            resolution: LockfileResolution::Registry(RegistryResolution {
                integrity: "sha512-PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP"
                    .parse()
                    .expect("parse integrity"),
                revision: None,
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
        },
    );
    let mut snapshots = HashMap::new();
    snapshots.insert(patched_key.clone(), SnapshotEntry::default());

    let graph = crate::virtual_store_layout::graph_hash::lockfile_to_dep_graph(
        &snapshots,
        Some(&packages),
        None,
    );
    let node = graph
        .get(&patched_key.to_string())
        .expect("patched snapshot node");
    assert!(
        node.full_pkg_id.starts_with("foo@1.0.0(patch_hash=abc):"),
        "full_pkg_id must keep the patch-hash segment; got {:?}",
        node.full_pkg_id,
    );
}
#[test]
fn full_pkg_id_of_a_custom_resolution_follows_its_integrity() {
    let key: PackageKey = "dep-a@1.0.0".parse().expect("parse key");
    let full_pkg_id = |resolution: serde_json::Value| {
        let metadata: PackageMetadata =
            serde_json::from_value(serde_json::json!({ "resolution": resolution }))
                .expect("parse custom package metadata");
        let packages = HashMap::from([(key.clone(), metadata)]);
        crate::virtual_store_layout::graph_hash::full_pkg_id_of(&key, Some(&packages))
    };

    assert_eq!(
        full_pkg_id(serde_json::json!({ "type": "custom:served", "integrity": "sha512-old" })),
        "dep-a@1.0.0:sha512-old",
    );
    let by_url =
        |url: &str| full_pkg_id(serde_json::json!({ "type": "custom:served", "url": url }));
    assert_ne!(by_url("https://example.test/a.tgz"), by_url("https://example.test/b.tgz"));
}
#[test]
fn link_hash_matches_the_shared_typescript_fixture() {
    let fixture: LinkHashParityFixture = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fixtures/gvs-link-hash-parity.json"
    )))
    .expect("parse shared GVS link hash fixture");
    let cases = if cfg!(windows) { &fixture.win32 } else { &fixture.posix };
    let package_key: PackageKey = fixture.package.key.parse().expect("parse fixture package key");
    let packages = HashMap::from([(
        package_key.without_peer(),
        package_metadata(
            LockfileResolution::Registry(RegistryResolution {
                integrity: fixture.package.integrity.parse().expect("parse fixture integrity"),
                revision: None,
            }),
            Some(&fixture.package.version),
        ),
    )]);
    let config = make_config(
        true,
        PathBuf::from("project/node_modules/.pnpm"),
        PathBuf::from("store/links"),
    );

    for case in cases {
        let snapshots = HashMap::from([(
            package_key.clone(),
            snapshot_with_link(&fixture.package.alias, &case.target),
        )]);
        let graph = crate::virtual_store_layout::graph_hash::lockfile_to_dep_graph(
            &snapshots,
            Some(&packages),
            Some(Path::new(&case.lockfile_dir)),
        );
        let link_node = graph
            .get(&fixture.package.key)
            .and_then(|node| node.children.get(&fixture.package.alias))
            .unwrap_or_else(|| panic!("{}: fixture link node", case.name));
        assert_eq!(link_node, &case.expected_link_node, "{}: normalized link node", case.name);

        let slot = VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            Some(&crate::AllowBuildPolicy::default()),
            Some(Path::new(&case.lockfile_dir)),
        )
        .slot_dir(&package_key);
        let relative_slot = slot
            .strip_prefix(Path::new("store/links"))
            .unwrap_or_else(|_| panic!("{}: strip GVS root from {slot:?}", case.name))
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        assert_eq!(relative_slot, case.expected_slot, "{}: GVS slot hash", case.name);
        assert_eq!(
            package_key.name.to_string(),
            fixture.package.name,
            "fixture package name must match its key",
        );
    }
}

/// A package `sideEffectsCacheExclude` names is built in every project,
/// so its global-virtual-store slot must not carry one project's build
/// into another.
#[test]
fn side_effects_cache_exclude_gives_each_project_its_own_slot() {
    let excluded: PackageKey = "java@1.0.0".parse().unwrap();
    let shared: PackageKey = "left-pad@1.0.0".parse().unwrap();
    let registry = || {
        LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
                .parse()
                .expect("parse integrity"),
            revision: None,
        })
    };
    let snapshots = HashMap::from([
        (excluded.clone(), SnapshotEntry::default()),
        (shared.clone(), SnapshotEntry::default()),
    ]);
    let packages = HashMap::from([
        (excluded.without_peer(), package_metadata(registry(), None)),
        (shared.without_peer(), package_metadata(registry(), None)),
    ]);
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let policy = crate::AllowBuildPolicy::default()
        .with_side_effects_cache_exclude(&["java".to_string()])
        .expect("valid pattern");
    let layout_in = |lockfile_dir: &str| {
        VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            Some(&policy),
            Some(Path::new(lockfile_dir)),
        )
    };

    let (in_project_a, in_project_b) = (layout_in("/home/user/a"), layout_in("/home/user/b"));

    assert_ne!(in_project_a.slot_dir(&excluded), in_project_b.slot_dir(&excluded));
    assert_eq!(in_project_a.slot_dir(&shared), in_project_b.slot_dir(&shared));
}
