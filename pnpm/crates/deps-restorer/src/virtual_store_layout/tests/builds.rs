use super::{
    super::VirtualStoreLayout, LinkHashParityFixture, make_config, package_metadata,
    snapshot_with_link,
};
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, RegistryResolution, SnapshotEntry,
};
use pretty_assertions::assert_eq;
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
    let node = graph.get(&patched_key.to_string()).expect("patched snapshot node");
    assert!(
        node.full_pkg_id.starts_with("foo@1.0.0(patch_hash=abc):"),
        "full_pkg_id must keep the patch-hash segment; got {:?}",
        node.full_pkg_id,
    );
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
