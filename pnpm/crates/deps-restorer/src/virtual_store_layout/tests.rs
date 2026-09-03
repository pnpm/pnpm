use super::{VirtualStoreLayout, global_virtual_store_version_dir};
use pnpm_config::Config;
use pnpm_lockfile::{
    DirectoryResolution, LockfileResolution, PackageKey, PackageMetadata, PkgName,
    RegistryResolution, SnapshotDepRef, SnapshotEntry, TarballResolution,
};
use pretty_assertions::{assert_eq, assert_ne};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::{Path, PathBuf},
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

/// [`PackageMetadata`] carrying only the fields the layout reads.
fn package_metadata(resolution: LockfileResolution, version: Option<&str>) -> PackageMetadata {
    PackageMetadata {
        resolution,
        version: version.map(str::to_string),
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
fn gvs_version_dir_requires_exact_name_and_version_components() {
    let root = PathBuf::from("store").join("links");
    let scoped: PackageKey = "@scope/foo@1.2.3".parse().expect("parse scoped key");
    let unscoped: PackageKey = "foo@1.2.3".parse().expect("parse unscoped key");
    assert_eq!(
        global_virtual_store_version_dir(&root, &scoped, None),
        Some(root.join("@scope").join("foo").join("1.2.3")),
    );
    assert_eq!(
        global_virtual_store_version_dir(&root, &unscoped, None),
        Some(root.join("@").join("foo").join("1.2.3")),
    );

    for key in ["../other@1.2.3", "@scope/../other@1.2.3", r"@scope\evil/foo@1.2.3"] {
        let key: PackageKey = key.parse().expect("parse unsafe package name");
        assert_eq!(global_virtual_store_version_dir(&root, &key, None), None);
    }

    let registry = RegistryResolution {
        integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
            .parse()
            .expect("parse integrity"),
        revision: None,
    };
    for version in ["../@/other/1.2.3", r"..\@\other\1.2.3"] {
        let metadata =
            package_metadata(LockfileResolution::Registry(registry.clone()), Some(version));
        assert_eq!(global_virtual_store_version_dir(&root, &unscoped, Some(&metadata)), None);
    }
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

    let graph = super::lockfile_to_dep_graph(&snapshots, Some(&packages), None);
    let node = graph.get(&patched_key.to_string()).expect("patched snapshot node");
    assert!(
        node.full_pkg_id.starts_with("foo@1.0.0(patch_hash=abc):"),
        "full_pkg_id must keep the patch-hash segment; got {:?}",
        node.full_pkg_id,
    );
}

#[test]
fn gvs_version_segment_anchors_directory_deps() {
    let semver: PackageKey = "foo@1.2.3".parse().unwrap();
    assert_eq!(super::gvs_version_segment(None, &semver.suffix), "1.2.3");

    // The lockfile records no version for a directory snapshot, and the
    // install path that resolves from manifests does know it — so the
    // segment has to be the same either way, or a re-install would
    // relocate the package.
    let dir_dep: PackageKey = "b@file:packages/b".parse().unwrap();
    let dir_metadata = package_metadata(
        LockfileResolution::Directory(DirectoryResolution { directory: "packages/b".to_string() }),
        None,
    );
    assert_eq!(super::gvs_version_segment(Some(&dir_metadata), &dir_dep.suffix), "directory");

    // A local tarball is content-addressed and does carry a version.
    let tarball_dep: PackageKey = "tar-dep@file:vendor/dep.tgz".parse().unwrap();
    let tarball_metadata = package_metadata(
        LockfileResolution::Tarball(TarballResolution {
            tarball: "file:vendor/dep.tgz".to_string(),
            integrity: None,
            revision: None,
            git_hosted: None,
            path: None,
        }),
        Some("0.0.0"),
    );
    assert_eq!(super::gvs_version_segment(Some(&tarball_metadata), &tarball_dep.suffix), "0.0.0");
}

/// A lockfile that keeps a `file:` snapshot but drops or mismatches its
/// `packages:` entry leaves no resolution to read. The segment and the
/// project scope have to reach the same verdict from what is left, or
/// the snapshot takes the anchored segment while missing the scope —
/// which is the collision the scope exists to prevent.
#[test]
fn a_file_snapshot_without_metadata_is_still_scoped() {
    let dir_dep: PackageKey = "b@file:packages/b".parse().unwrap();

    assert_eq!(super::gvs_version_segment(None, &dir_dep.suffix), "directory");
    assert_eq!(
        super::local_directory_scope(None, &dir_dep.suffix, Some("/home/user/a")),
        Some("/home/user/a"),
    );

    // The same holds when the entry is present but carries neither a
    // directory resolution nor a version to fall back on.
    let bare = package_metadata(
        LockfileResolution::Tarball(TarballResolution {
            tarball: "file:packages/b".to_string(),
            integrity: None,
            revision: None,
            git_hosted: None,
            path: None,
        }),
        None,
    );
    assert_eq!(super::gvs_version_segment(Some(&bare), &dir_dep.suffix), "directory");
    assert_eq!(
        super::local_directory_scope(Some(&bare), &dir_dep.suffix, Some("/home/user/a")),
        Some("/home/user/a"),
    );
}

/// Two unrelated projects that both depend on a `./dep` directory
/// resolve to the same snapshot key, name, and (absent) version — only
/// the lockfile directory tells their slots apart.
#[test]
fn directory_deps_get_a_slot_per_project() {
    let key: PackageKey = "dep@file:dep".parse().unwrap();
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let mut packages = HashMap::new();
    packages.insert(
        key.without_peer(),
        package_metadata(
            LockfileResolution::Directory(DirectoryResolution { directory: "dep".to_string() }),
            None,
        ),
    );

    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let slot_in = |lockfile_dir: &str| {
        super::VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            None,
            Some(Path::new(lockfile_dir)),
        )
        .slot_dir(&key)
    };

    let in_project_a = slot_in("/home/user/a");
    let in_project_b = slot_in("/home/user/b");
    assert_ne!(in_project_a, in_project_b);
    // The slot is `<root>/@/dep/<version>/<hash>`, so the version segment is
    // the hash directory's parent. Compared as a component rather than a
    // substring: the separator is native, `\` on Windows.
    assert_eq!(
        in_project_a.parent().and_then(Path::file_name),
        Some(OsStr::new("directory")),
        "directory deps take the anchored version segment; got {in_project_a:?}",
    );
}

/// Build a registry snapshot carrying one `link:` dependency.
fn snapshot_with_link(alias: &str, target: &str) -> SnapshotEntry {
    let mut dependencies = HashMap::new();
    dependencies.insert(
        alias.parse::<PkgName>().unwrap(),
        format!("link:{target}").parse::<SnapshotDepRef>().unwrap(),
    );
    SnapshotEntry { dependencies: Some(dependencies), ..SnapshotEntry::default() }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkHashParityFixture {
    package: LinkHashFixturePackage,
    posix: Vec<LinkHashFixtureCase>,
    win32: Vec<LinkHashFixtureCase>,
}

#[derive(Deserialize)]
struct LinkHashFixturePackage {
    key: String,
    name: String,
    version: String,
    alias: String,
    integrity: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkHashFixtureCase {
    name: String,
    lockfile_dir: String,
    target: String,
    expected_link_node: String,
    expected_slot: String,
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
        let graph = super::lockfile_to_dep_graph(
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

/// A `link:` dependency is materialized as a symlink out of the store,
/// so the slot holding it belongs to whichever directory the lockfile's
/// relative target resolves to. Two projects linking *different*
/// directories must not land on one slot — the peer suffix that tells
/// their dep paths apart is deliberately absent from the GVS hash, so
/// without the link scope they would collide and share whichever
/// symlink was written first.
#[test]
fn snapshots_with_link_deps_get_a_slot_per_link_target() {
    let key: PackageKey = "react-dom@18.3.1(react@fake-react)".parse().unwrap();
    let mut packages = HashMap::new();
    packages.insert(
        key.without_peer(),
        package_metadata(
            LockfileResolution::Tarball(TarballResolution {
                tarball: "https://registry.npmjs.org/react-dom/-/react-dom-18.3.1.tgz".to_string(),
                integrity: None,
                revision: None,
                git_hosted: None,
                path: None,
            }),
            Some("18.3.1"),
        ),
    );
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let slot_for = |target: &str| {
        let mut snapshots = HashMap::new();
        snapshots.insert(key.clone(), snapshot_with_link("react", target));
        VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            None,
            Some(Path::new("/home/user/proj")),
        )
        .slot_dir(&key)
    };

    assert_ne!(slot_for("../react-a"), slot_for("../react-b"));
}

/// A linked target affects every package whose dependency graph reaches
/// it, not just the snapshot that declares the `link:` edge. Otherwise
/// an ancestor slot can be reused with a child slot from another project.
#[test]
fn link_targets_propagate_through_transitive_ancestor_slots() {
    let parent_key: PackageKey = "wrapper@1.0.0".parse().unwrap();
    let child_key: PackageKey = "react-dom@18.3.1(react@fake-react)".parse().unwrap();
    let packages = HashMap::from([
        (parent_key.without_peer(), registry_metadata("PARENT")),
        (child_key.without_peer(), registry_metadata("CHILD")),
    ]);
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let slots_for = |target: &str| {
        let parent = SnapshotEntry {
            dependencies: Some(HashMap::from([(
                alias("react-dom"),
                SnapshotDepRef::Alias(child_key.clone()),
            )])),
            ..SnapshotEntry::default()
        };
        let snapshots = HashMap::from([
            (parent_key.clone(), parent),
            (child_key.clone(), snapshot_with_link("react", target)),
        ]);
        let layout = VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            None,
            Some(Path::new("/home/user/proj")),
        );
        (layout.slot_dir(&parent_key), layout.slot_dir(&child_key))
    };

    let (parent_a, child_a) = slots_for("../react-a");
    let (parent_b, child_b) = slots_for("../react-b");
    assert_ne!(child_a, child_b);
    assert_ne!(parent_a, parent_b);
}

#[test]
fn link_dependency_alias_participates_in_the_slot_hash() {
    let key: PackageKey = "consumer@1.0.0".parse().unwrap();
    let packages = HashMap::from([(key.without_peer(), registry_metadata("CONSUMER"))]);
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let slot_for = |alias: &str| {
        let snapshots = HashMap::from([(key.clone(), snapshot_with_link(alias, "../shared"))]);
        VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            None,
            Some(Path::new("/home/user/proj")),
        )
        .slot_dir(&key)
    };

    assert_ne!(slot_for("linked"), slot_for("renamed"));
}

/// The scope is the *resolved target*, not the project, so workspaces
/// that link the same directory keep sharing one slot — the case that
/// matters for a toolchain linked into many workspaces. Contrast
/// [`directory_deps_get_a_slot_per_project`], where the project itself
/// is the only thing that can tell two slots apart.
#[test]
fn link_deps_resolving_to_one_directory_share_a_slot_across_projects() {
    let key: PackageKey = "react-dom@18.3.1(react@shared)".parse().unwrap();
    let mut snapshots = HashMap::new();
    // Both projects sit one level under `/home/user`, so `../shared`
    // resolves to `/home/user/shared` from either.
    snapshots.insert(key.clone(), snapshot_with_link("react", "../shared"));
    let mut packages = HashMap::new();
    packages.insert(
        key.without_peer(),
        package_metadata(
            LockfileResolution::Tarball(TarballResolution {
                tarball: "https://registry.npmjs.org/react-dom/-/react-dom-18.3.1.tgz".to_string(),
                integrity: None,
                revision: None,
                git_hosted: None,
                path: None,
            }),
            Some("18.3.1"),
        ),
    );
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let slot_in = |lockfile_dir: &str| {
        VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            None,
            Some(Path::new(lockfile_dir)),
        )
        .slot_dir(&key)
    };

    assert_eq!(slot_in("/home/user/a"), slot_in("/home/user/b"));
}

/// The link scope applies only to snapshots that actually carry a
/// `link:` dependency: every other slot has to keep hashing exactly as
/// it did before, or an upgrade would re-materialize the whole store.
#[test]
fn snapshots_without_link_deps_keep_their_slot() {
    let key: PackageKey = "react-dom@18.3.1".parse().unwrap();
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let mut packages = HashMap::new();
    packages.insert(
        key.without_peer(),
        package_metadata(
            LockfileResolution::Tarball(TarballResolution {
                tarball: "https://registry.npmjs.org/react-dom/-/react-dom-18.3.1.tgz".to_string(),
                integrity: None,
                revision: None,
                git_hosted: None,
                path: None,
            }),
            Some("18.3.1"),
        ),
    );
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let slot_in = |lockfile_dir: &str| {
        VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            None,
            Some(Path::new(lockfile_dir)),
        )
        .slot_dir(&key)
    };

    assert_eq!(slot_in("/home/user/a"), slot_in("/home/user/b"));
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

/// Digests taken from pnpm's own `iterateHashedGraphNodes` over the
/// same graph. Both CLIs share one global virtual store, so a snapshot
/// inside a dependency cycle has to land on the same slot whichever of
/// them installed it.
#[test]
fn cyclic_gvs_slots_match_pnpm() {
    let expected = [
        ("a@1.0.0", "@/a/1.0.0/3bf76a728c7e155a17137e13ca7af820eb13a0ff4003fed5c39649aac0309905"),
        ("b@1.0.0", "@/b/1.0.0/728ab1a600ca69780231a2630496c2e83a88c82e965d09ac193fa448c0148a6d"),
        ("m@1.0.0", "@/m/1.0.0/d671219222f32dfed181f294115e23fc07805b7b78048002872f131f69c07983"),
        ("n@1.0.0", "@/n/1.0.0/92dec6e2ee818106a34b91bea178347566cc47fe25208afa683831ca5d494605"),
        ("p@1.0.0", "@/p/1.0.0/b666adb72829d0d822cf2005226985c0b19de632b569fcd75363adf141af3c41"),
        ("q@1.0.0", "@/q/1.0.0/49a1cd21058b93255957e5accf04137fe74ef88364f96f4b1116e3ca75083b6a"),
        ("x@1.0.0", "@/x/1.0.0/4230805547fe5208b58758d53b200bc13c5475e3324af75adc87538fcf247857"),
        ("y@1.0.0", "@/y/1.0.0/4fba92559776b8cd9c4a541f07da9fcc95c8a8ea427f4116de7b2f0606f0f03d"),
    ];
    assert_eq!(
        cyclic_slot_suffixes(),
        expected
            .iter()
            .map(|(key, suffix)| ((*key).to_string(), (*suffix).to_string()))
            .collect::<Vec<_>>(),
    );
}

/// The `HashMap`s a lockfile parses into iterate in a different order
/// on every run, so a layout that walked them directly would move a
/// cycle's slots around and make each repeat install re-import whatever
/// landed on a fresh one ([pnpm/pnpm#13316](https://github.com/pnpm/pnpm/issues/13316)).
#[test]
fn cyclic_gvs_slots_are_stable_across_runs() {
    let first = cyclic_slot_suffixes();
    for _ in 0..64 {
        assert_eq!(cyclic_slot_suffixes(), first);
    }
}

fn cyclic_slot_suffixes() -> Vec<(String, String)> {
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let (snapshots, packages) = cyclic_snapshots();
    // An empty policy allows no builds, which makes every snapshot hash
    // engine-agnostically — the digests below then hold on any host.
    let policy = crate::AllowBuildPolicy::default();
    let layout = VirtualStoreLayout::new(
        &config,
        Some("darwin-arm64-node20"),
        Some(&snapshots),
        Some(&packages),
        Some(&policy),
        None,
    );
    let mut suffixes: Vec<(String, String)> = snapshots
        .keys()
        .map(|snapshot_key| {
            let slot = layout.slot_dir(snapshot_key);
            let suffix = slot
                .strip_prefix("/tmp/store/links")
                .expect("slot under the global virtual store")
                .to_string_lossy()
                // `slot_dir` builds the tail from native components, so
                // compare against the `/`-separated form the GVS path is
                // canonically written in.
                .replace('\\', "/");
            (snapshot_key.to_string(), suffix)
        })
        .collect();
    suffixes.sort();
    suffixes
}

/// Two cyclic subgraphs whose digests only come out right when the
/// hasher visits a snapshot's children the way pnpm's
/// `{...dependencies, ...optionalDependencies}` object does.
///
/// `a` reaches the `p` ↔ `x` cycle through two regular dependencies —
/// aliased `c` and `p` — so it pins the order *within* a section.
/// `b` reaches the `n` ↔ `y` cycle through a regular `n` and an
/// optional `c`, whose alias sorts first, so it pins that
/// `optionalDependencies` still come last.
fn cyclic_snapshots() -> (HashMap<PackageKey, SnapshotEntry>, HashMap<PackageKey, PackageMetadata>)
{
    let snapshots = HashMap::from([
        (
            key("a@1.0.0"),
            SnapshotEntry {
                dependencies: Some(HashMap::from([
                    (alias("c"), SnapshotDepRef::Alias(key("q@1.0.0"))),
                    (alias("p"), plain("1.0.0")),
                ])),
                ..Default::default()
            },
        ),
        (
            key("b@1.0.0"),
            SnapshotEntry {
                dependencies: Some(HashMap::from([(alias("n"), plain("1.0.0"))])),
                optional_dependencies: Some(HashMap::from([(
                    alias("c"),
                    SnapshotDepRef::Alias(key("m@1.0.0")),
                )])),
                ..Default::default()
            },
        ),
        (key("m@1.0.0"), depends_on("y")),
        (key("n@1.0.0"), depends_on("y")),
        (key("p@1.0.0"), depends_on("x")),
        (key("q@1.0.0"), depends_on("x")),
        (key("x@1.0.0"), depends_on("p")),
        (key("y@1.0.0"), depends_on("n")),
    ]);
    let packages = snapshots
        .keys()
        .map(|snapshot_key| {
            let lead = snapshot_key.name.to_string().to_uppercase();
            (snapshot_key.clone(), registry_metadata(&lead))
        })
        .collect();
    (snapshots, packages)
}

fn key(text: &str) -> PackageKey {
    text.parse().expect("parse package key")
}

fn alias(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse alias")
}

fn plain(version: &str) -> SnapshotDepRef {
    SnapshotDepRef::Plain(version.parse().expect("parse version"))
}

fn depends_on(name: &str) -> SnapshotEntry {
    SnapshotEntry {
        dependencies: Some(HashMap::from([(alias(name), plain("1.0.0"))])),
        ..Default::default()
    }
}

/// Registry metadata whose integrity starts with `lead`, so each
/// package contributes a distinct `full_pkg_id` to the digests.
fn registry_metadata(lead: &str) -> PackageMetadata {
    let integrity = format!("sha512-{lead}{}", "A".repeat(91));
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: integrity.parse().expect("parse integrity"),
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
    }
}
