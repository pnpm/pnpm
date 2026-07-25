use super::VirtualStoreLayout;
use pacquet_config::Config;
use pacquet_lockfile::{
    DirectoryResolution, LockfileResolution, PackageKey, PackageMetadata, PkgName,
    RegistryResolution, SnapshotDepRef, SnapshotEntry,
};
use pretty_assertions::{assert_eq, assert_ne};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

/// Build a `Config` test-double with the GVS-relevant fields
/// wired explicitly. `gvs_dir` populates `global_virtual_store_dir`
/// for the GVS-on path; `virtual_store_dir` stays at the
/// project-local default for the GVS-off path.
fn make_config(gvs: bool, virtual_store_dir: PathBuf, gvs_dir: PathBuf) -> Config {
    let mut config = Config::new();
    config.enable_global_virtual_store = gvs;
    config.virtual_store_dir = virtual_store_dir;
    config.global_virtual_store_dir = gvs_dir;
    config
}

fn registry_metadata(integrity: &str) -> PackageMetadata {
    PackageMetadata {
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
    }
}

#[test]
fn slot_dir_uses_flat_name_when_gvs_off() {
    let config = make_config(
        false,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let layout = VirtualStoreLayout::new(&config, Some("ignored"), None, None, None, None);
    let key: PackageKey = "@scope/foo@1.2.3".parse().unwrap();
    assert_eq!(
        layout.slot_dir(&key),
        PathBuf::from("/tmp/proj/node_modules/.pnpm/@scope+foo@1.2.3"),
    );
}

#[test]
fn slot_dir_uses_gvs_layout_when_gvs_on() {
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let key: PackageKey = "@scope/foo@1.2.3".parse().unwrap();
    let mut packages = HashMap::new();
    packages.insert(
        key.clone(),
        PackageMetadata {
            resolution: LockfileResolution::Registry(RegistryResolution {
                integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                    .parse()
                    .expect("parse integrity"),
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
    let layout = VirtualStoreLayout::new(
        &config,
        Some("darwin-arm64-node20"),
        Some(&snapshots),
        Some(&packages),
        None,
        None,
    );
    let slot = layout.slot_dir(&key);
    let stripped = slot
        .strip_prefix("/tmp/store/links/@scope/foo/1.2.3/")
        .expect("slot dir must live under <root>/<scope>/<name>/<version>/ when GVS is on");
    assert_eq!(
        stripped.to_string_lossy().len(),
        64,
        "trailing hash component must be a full sha256 hex digest",
    );
}

/// Unscoped packages get an `@/` prefix so every entry in the
/// shared store sits at the same `<scope>/<name>/<version>/<hash>`
/// depth — easier `readdir`-driven traversal.
#[test]
fn slot_dir_prefixes_unscoped_with_at_slash_under_gvs() {
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let key: PackageKey = "foo@1.0.0".parse().unwrap();
    let mut packages = HashMap::new();
    packages.insert(
        key.clone(),
        PackageMetadata {
            resolution: LockfileResolution::Registry(RegistryResolution {
                integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                    .parse()
                    .expect("parse integrity"),
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
    let layout = VirtualStoreLayout::new(
        &config,
        Some("linux-x64-node22"),
        Some(&snapshots),
        Some(&packages),
        None,
        None,
    );
    let slot = layout.slot_dir(&key);
    let _ = slot
        .strip_prefix("/tmp/store/links/@/foo/1.0.0/")
        .expect("unscoped GVS slots live under <root>/@/<name>/<version>/<hash>");
}

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
fn slot_dir_engine_specific_when_snapshot_is_built() {
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let key: PackageKey = "native-pkg@1.0.0".parse().unwrap();
    let mut packages = HashMap::new();
    packages.insert(
        key.clone(),
        PackageMetadata {
            resolution: LockfileResolution::Registry(RegistryResolution {
                integrity: "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC"
                    .parse()
                    .expect("parse integrity"),
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
    let allowed: std::collections::HashSet<String> =
        std::iter::once("native-pkg".to_string()).collect();
    let policy = crate::AllowBuildPolicy::new(allowed, std::collections::HashSet::new(), false);
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
    assert_ne!(darwin, linux, "builder snapshot must partition GVS slot by engine string");
}

#[test]
fn missing_metadata_keeps_source_dep_path_untrusted_for_gvs() {
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let key: PackageKey = "spoofed@git-hosted#abc123".parse().unwrap();
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let packages = HashMap::new();
    let allowed: HashSet<String> = std::iter::once("spoofed".to_string()).collect();
    let policy = crate::AllowBuildPolicy::new(allowed, HashSet::new(), false);
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
    assert_eq!(darwin, linux, "source depPath with missing metadata must not be name-allowed");
}

/// Per-snapshot `engines.runtime` resolution: two builder
/// siblings that pin *different* Node majors must land on
/// different GVS slots even when given the same install-wide
/// fallback engine. The bin linker spawns each pinning package's
/// lifecycle scripts through its own downloaded Node, so anchoring
/// the engine portion of the hash to a single install-wide value
/// would produce the wrong side-effects-cache key for cross-pinning
/// installs.
#[test]
fn cross_pinning_siblings_get_distinct_slots() {
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );

    let pins_22: PackageKey = "pins-22@1.0.0".parse().unwrap();
    let pins_20: PackageKey = "pins-20@1.0.0".parse().unwrap();
    let node22_key: PackageKey = "node@runtime:22.11.0".parse().unwrap();
    let node20_key: PackageKey = "node@runtime:20.18.0".parse().unwrap();

    let mut packages = HashMap::new();
    let integrities = [
        (
            pins_22.clone(),
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ),
        (
            pins_20.clone(),
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
        ),
        (
            node22_key.clone(),
            "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
        ),
        (
            node20_key.clone(),
            "sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD",
        ),
    ];
    for (key, integrity_str) in integrities {
        packages.insert(
            key,
            PackageMetadata {
                resolution: LockfileResolution::Registry(RegistryResolution {
                    integrity: integrity_str.parse().expect("parse integrity"),
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
    }

    // Two builder siblings, each with `dependencies.node:
    // runtime:<major>` — the desugared form the resolver writes
    // for a manifest-level `engines.runtime` declaration.
    let mut pins_22_deps = HashMap::new();
    pins_22_deps.insert(
        PkgName::parse("node").expect("parse pkg name"),
        SnapshotDepRef::Plain("runtime:22.11.0".parse().expect("parse ver-peer")),
    );
    let pins_22_snapshot =
        SnapshotEntry { dependencies: Some(pins_22_deps), ..SnapshotEntry::default() };

    let mut pins_20_deps = HashMap::new();
    pins_20_deps.insert(
        PkgName::parse("node").expect("parse pkg name"),
        SnapshotDepRef::Plain("runtime:20.18.0".parse().expect("parse ver-peer")),
    );
    let pins_20_snapshot =
        SnapshotEntry { dependencies: Some(pins_20_deps), ..SnapshotEntry::default() };

    let mut snapshots = HashMap::new();
    snapshots.insert(pins_22.clone(), pins_22_snapshot);
    snapshots.insert(pins_20.clone(), pins_20_snapshot);
    snapshots.insert(node22_key, SnapshotEntry::default());
    snapshots.insert(node20_key, SnapshotEntry::default());

    // Both siblings are approved builders so the engine portion
    // of the hash isn't dropped by the engine-agnostic gating.
    let allowed: std::collections::HashSet<String> =
        ["pins-22".to_string(), "pins-20".to_string()].into_iter().collect();
    let policy = crate::AllowBuildPolicy::new(allowed, std::collections::HashSet::new(), false);

    // Same install-wide fallback for both layout queries — the
    // divergence has to come from the per-snapshot pin lookup.
    let layout = VirtualStoreLayout::new(
        &config,
        Some("darwin;arm64;node24"),
        Some(&snapshots),
        Some(&packages),
        Some(&policy),
        None,
    );
    let slot_22 = layout.slot_dir(&pins_22);
    let slot_20 = layout.slot_dir(&pins_20);
    assert_ne!(slot_22, slot_20, "cross-pinning builders must land on distinct GVS slots");
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

    let graph = super::lockfile_to_dep_graph(&snapshots, Some(&packages));
    let node = graph.get(&patched_key).expect("patched snapshot node");
    assert!(
        node.full_pkg_id.starts_with("foo@1.0.0(patch_hash=abc):"),
        "full_pkg_id must keep the patch-hash segment; got {:?}",
        node.full_pkg_id,
    );
}

#[test]
fn gvs_version_segment_renders_file_deps_as_undefined() {
    let semver: PackageKey = "foo@1.2.3".parse().unwrap();
    assert_eq!(super::gvs_version_segment(&semver.suffix), "1.2.3");

    let file_dep: PackageKey = "b@file:packages/b".parse().unwrap();
    assert_eq!(super::gvs_version_segment(&file_dep.suffix), "undefined");
}

#[test]
fn empty_context_projection_preserves_legacy_gvs_path() {
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let key: PackageKey = "consumer@1.0.0".parse().unwrap();
    let snapshots = HashMap::from([(key.clone(), SnapshotEntry::default())]);
    let packages = HashMap::from([(
        key.clone(),
        registry_metadata(
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ),
    )]);
    let legacy =
        VirtualStoreLayout::new(&config, None, Some(&snapshots), Some(&packages), None, None);
    let empty_projection = BTreeMap::new();
    let contextual = VirtualStoreLayout::new(
        &config,
        None,
        Some(&snapshots),
        Some(&packages),
        None,
        Some(&empty_projection),
    );

    assert_eq!(contextual.slot_dir(&key), legacy.slot_dir(&key));
    assert!(contextual.context_modules_dir().is_none());
}

#[test]
fn context_hash_changes_with_projected_target_version() {
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let consumer: PackageKey = "consumer@1.0.0".parse().unwrap();
    let target_v1: PackageKey = "ambient@1.0.0".parse().unwrap();
    let target_v2: PackageKey = "ambient@2.0.0".parse().unwrap();
    let consumer_metadata = registry_metadata(
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );
    let target_metadata = registry_metadata(
        "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
    );

    let make_layout = |target: &PackageKey| {
        let snapshots = HashMap::from([
            (consumer.clone(), SnapshotEntry::default()),
            (target.clone(), SnapshotEntry::default()),
        ]);
        let packages = HashMap::from([
            (consumer.clone(), consumer_metadata.clone()),
            (target.clone(), target_metadata.clone()),
        ]);
        let projection = BTreeMap::from([("ambient".to_string(), target.clone())]);
        VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            None,
            Some(&projection),
        )
    };
    let v1_layout = make_layout(&target_v1);
    let v2_layout = make_layout(&target_v2);
    let v1_rel = v1_layout.slot_dir(&consumer).strip_prefix("/tmp/store/links").unwrap().to_owned();
    let v2_rel = v2_layout.slot_dir(&consumer).strip_prefix("/tmp/store/links").unwrap().to_owned();
    let v1_parts = v1_rel.iter().collect::<Vec<_>>();
    let v2_parts = v2_rel.iter().collect::<Vec<_>>();

    assert_eq!(v1_parts[0], "contexts");
    assert_eq!(v2_parts[0], "contexts");
    assert_ne!(v1_parts[1], v2_parts[1], "target version must partition the context namespace");
    assert_eq!(
        &v1_parts[2..],
        &v2_parts[2..],
        "the consumer's dependency-only inner slot identity must stay unchanged",
    );
}

/// `collect_injected_deps` maps each `file:` snapshot's source path to
/// its slot package dir (lockfile-relative), skipping registry
/// snapshots and skipped `file:` snapshots, and aggregating all peer
/// variants of one source project under one key.
#[test]
fn collect_injected_deps_maps_file_snapshots_to_slots() {
    let lockfile_dir = std::path::Path::new("/ws");
    let layout = super::VirtualStoreLayout::legacy("/ws/node_modules/.pnpm", 120);

    let variant_a: PackageKey = "@scope/comp2@file:comp2(react@16.14.0)".parse().unwrap();
    let variant_b: PackageKey = "@scope/comp2@file:comp2(react@17.0.2)".parse().unwrap();
    let other: PackageKey = "@scope/comp3@file:./comp3".parse().unwrap();
    let registry: PackageKey = "react@16.14.0".parse().unwrap();
    let skipped_key: PackageKey = "@scope/skipped@file:skipped".parse().unwrap();

    let mut snapshots = HashMap::new();
    for key in [&variant_a, &variant_b, &other, &registry, &skipped_key] {
        snapshots.insert(key.clone(), SnapshotEntry::default());
    }
    // A `file:` tarball snapshot: present in `snapshots` but with a
    // tarball resolution — must NOT be treated as an injected project.
    let tarball_key: PackageKey = "tar-dep@file:vendor/dep.tgz".parse().unwrap();
    snapshots.insert(tarball_key, SnapshotEntry::default());
    let mut packages = HashMap::new();
    for key in [&variant_a, &variant_b, &other, &skipped_key] {
        packages.insert(
            key.without_peer(),
            PackageMetadata {
                resolution: DirectoryResolution { directory: key.suffix.version().to_string() }
                    .into(),
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
    }
    let mut skipped = crate::SkippedSnapshots::new();
    skipped.insert_installability(skipped_key);

    let injected = super::collect_injected_deps(
        &layout,
        lockfile_dir,
        Some(&snapshots),
        Some(&packages),
        &skipped,
        None,
    );

    assert_eq!(injected.len(), 2, "registry + skipped snapshots must not appear: {injected:?}");
    let comp2 = &injected["comp2"];
    assert_eq!(comp2.len(), 2, "both peer variants of comp2 must be present");
    for target in comp2 {
        assert!(
            target.starts_with("node_modules/.pnpm/")
                && target.ends_with("/node_modules/@scope/comp2"),
            "target must be a lockfile-relative slot package dir; got {target:?}",
        );
    }
    // A `file:./comp3` source normalizes to the importer id `comp3`.
    assert_eq!(injected["comp3"].len(), 1);

    // No snapshots section → empty map.
    assert!(
        super::collect_injected_deps(&layout, lockfile_dir, None, Some(&packages), &skipped, None)
            .is_empty(),
    );

    // Hoisted mode: targets come from the walker's hoisted locations
    // (keyed by full depPath), not from virtual-store slots; entries
    // the walker never placed are dropped.
    let mut hoisted = std::collections::BTreeMap::new();
    hoisted.insert(variant_a.to_string(), vec!["node_modules/@scope/comp2".to_string()]);
    let injected_hoisted = super::collect_injected_deps(
        &layout,
        lockfile_dir,
        Some(&snapshots),
        Some(&packages),
        &skipped,
        Some(&hoisted),
    );
    assert_eq!(injected_hoisted.len(), 1, "unplaced sources dropped: {injected_hoisted:?}");
    assert_eq!(injected_hoisted["comp2"], vec!["node_modules/@scope/comp2".to_string()]);
}
