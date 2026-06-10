use super::VirtualStoreLayout;
use pacquet_config::Config;
use pacquet_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgName, RegistryResolution, SnapshotDepRef,
    SnapshotEntry,
};
use pretty_assertions::{assert_eq, assert_ne};
use std::{
    collections::{HashMap, HashSet},
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

/// With GVS off, the layout reproduces today's flat-name layout
/// (`<virtual_store_dir>/<flat-name>`) — proving the helper is a
/// drop-in for the legacy path.
#[test]
fn slot_dir_uses_flat_name_when_gvs_off() {
    let config = make_config(
        false,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let layout = VirtualStoreLayout::new(&config, Some("ignored"), None, None, None);
    let key: PackageKey = "@scope/foo@1.2.3".parse().unwrap();
    assert_eq!(
        layout.slot_dir(&key),
        PathBuf::from("/tmp/proj/node_modules/.pnpm/@scope+foo@1.2.3"),
    );
}

/// With GVS on and a single snapshot, the layout produces the
/// `<root>/<scope>/<name>/<version>/<hash>` shape upstream's tests
/// assert against. The hash is opaque; we only check the prefix
/// and depth.
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
    );
    let slot = layout.slot_dir(&key);
    // Shape: `/tmp/store/links/@scope/foo/1.2.3/<64-hex>`.
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
    );
    let slot = layout.slot_dir(&key);
    let _ = slot
        .strip_prefix("/tmp/store/links/@/foo/1.0.0/")
        .expect("unscoped GVS slots live under <root>/@/<name>/<version>/<hash>");
}

/// End-to-end gating check: a pure-JS snapshot's GVS slot is
/// engine-agnostic when an empty `AllowBuildPolicy` is supplied
/// (matches upstream's
/// [`enableGlobalVirtualStore: true` → `allowBuilds ??= {}`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts#L342-L344)
/// shape). Two installs that differ only in the `engine` string
/// produce the *same* slot directory.
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
    )
    .slot_dir(&key);
    let linux = VirtualStoreLayout::new(
        &config,
        Some("linux-x64-node22"),
        Some(&snapshots),
        Some(&packages),
        Some(&policy),
    )
    .slot_dir(&key);
    assert_eq!(
        darwin, linux,
        "pure-JS snapshot must share one GVS slot across engines when gating is active",
    );
}

/// Symmetric to [`slot_dir_engine_agnostic_with_empty_allow_build_policy`]:
/// when the snapshot is in `allow_builds`, the engine *is* part
/// of the slot path. Two installs that differ in `engine` end up
/// in different directories.
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
    )
    .slot_dir(&key);
    let linux = VirtualStoreLayout::new(
        &config,
        Some("linux-x64-node22"),
        Some(&snapshots),
        Some(&packages),
        Some(&policy),
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
    )
    .slot_dir(&key);
    let linux = VirtualStoreLayout::new(
        &config,
        Some("linux-x64-node22"),
        Some(&snapshots),
        Some(&packages),
        Some(&policy),
    )
    .slot_dir(&key);
    assert_eq!(darwin, linux, "source depPath with missing metadata must not be name-allowed");
}

/// Per-snapshot `engines.runtime` resolution: two builder
/// siblings that pin *different* Node majors must land on
/// different GVS slots even when given the same install-wide
/// fallback engine. Mirrors the upstream behaviour in
/// [`@pnpm/deps.graph-hasher`'s `readSnapshotRuntimePin`
/// branch](https://github.com/pnpm/pnpm/blob/HEAD/deps/graph-hasher/src/index.ts).
/// The bin linker spawns each pinning package's lifecycle scripts
/// through its own downloaded Node, so anchoring the engine
/// portion of the hash to a single install-wide value would
/// produce the wrong side-effects-cache key for cross-pinning
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
    // runtime:<major>` — the desugared form upstream's resolver
    // writes for a manifest-level `engines.runtime` declaration.
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
    );
    let slot_22 = layout.slot_dir(&pins_22);
    let slot_20 = layout.slot_dir(&pins_20);
    assert_ne!(slot_22, slot_20, "cross-pinning builders must land on distinct GVS slots");
}

/// `lockfile_to_dep_graph` builds each node's `pkg_id_with_patch_hash`
/// via upstream's `getPkgIdWithPatchHash` semantics — strip the
/// peer-graph suffix but **keep** the `(patch_hash=…)` segment. Two
/// patched snapshots with different peer suffixes therefore land on
/// one `pkg_id_with_patch_hash`, mirroring pnpm's side-effects-cache
/// keying. See
/// [`getPkgIdWithPatchHash`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/src/index.ts#L63-L70).
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

/// The GVS version segment is the semver for a registry dep, and
/// the literal `undefined` for an injected `file:` directory dep —
/// mirroring pnpm's `nameVerFromPkgSnapshot` → `undefined` and
/// keeping the `:` out of the slot path. See pnpm/pnpm#12038.
#[test]
fn gvs_version_segment_renders_file_deps_as_undefined() {
    let semver: PackageKey = "foo@1.2.3".parse().unwrap();
    assert_eq!(super::gvs_version_segment(&semver.suffix), "1.2.3");

    let file_dep: PackageKey = "b@file:packages/b".parse().unwrap();
    assert_eq!(super::gvs_version_segment(&file_dep.suffix), "undefined");
}
