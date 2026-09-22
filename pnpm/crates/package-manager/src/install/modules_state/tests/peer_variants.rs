use super::{
    record_hoisted_locations,
    tree_intact,
};
use pnpm_config::NodeLinker;
use pnpm_deps_restorer::hoisted_dep_graph::{
    LockfileToHoistedDepGraphOptions,
    lockfile_to_hoisted_dep_graph,
};
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::{
    Host,
    Modules,
    write_modules_manifest,
};
use std::fs;
use tempfile::tempdir;
use text_block_macros::text_block;

fn lockfile_with_peer_variants() -> Lockfile {
    serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  packages/a:"
        "    dependencies:"
        "      foo: {specifier: 1.0.0, version: 1.0.0(peer@1.0.0)}"
        "      peer: {specifier: 1.0.0, version: 1.0.0}"
        "  packages/b:"
        "    dependencies:"
        "      foo: {specifier: 1.0.0, version: 1.0.0(peer@2.0.0)}"
        "      peer: {specifier: 2.0.0, version: 2.0.0}"
        "packages:"
        "  foo@1.0.0: {resolution: {integrity: sha512-AA==}}"
        "  peer@1.0.0: {resolution: {integrity: sha512-AA==}}"
        "  peer@2.0.0: {resolution: {integrity: sha512-AA==}}"
        "snapshots:"
        "  foo@1.0.0(peer@1.0.0):"
        "    dependencies: {peer: 1.0.0}"
        "  foo@1.0.0(peer@2.0.0):"
        "    dependencies: {peer: 2.0.0}"
        "  peer@1.0.0: {}"
        "  peer@2.0.0: {}"
    })
    .unwrap()
}

#[test]
fn collapsed_peer_variants_reuse_the_hoisters_recorded_placements() {
    let dir = tempdir().unwrap();
    let lockfile = lockfile_with_peer_variants();
    let options = LockfileToHoistedDepGraphOptions {
        lockfile_dir: dir.path().to_path_buf(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let hoisted = lockfile_to_hoisted_dep_graph(&lockfile, None, &options).unwrap();
    let foo_locations: Vec<_> = hoisted.hoisted_locations
        .iter()
        .filter(|(key, _)| key.starts_with("foo@"))
        .collect();
    assert_eq!(foo_locations.len(), 1, "the hoister records one canonical peer variant");
    let foo_dir = dir.path().join(&foo_locations[0].1[0]);
    for package_dir in hoisted.graph.keys() {
        fs::create_dir_all(package_dir).unwrap();
    }
    write_modules_manifest::<Host>(
        &dir.path().join("node_modules"),
        Modules { hoisted_locations: Some(hoisted.hoisted_locations), ..Modules::default() },
    )
    .unwrap();

    assert!(tree_intact(dir.path(), NodeLinker::Hoisted, &lockfile));
    fs::remove_dir_all(foo_dir).unwrap();
    assert!(!tree_intact(dir.path(), NodeLinker::Hoisted, &lockfile));
}

#[test]
fn injected_peer_variants_and_different_patches_need_their_own_placements() {
    for (required, recorded) in [
        ("foo@file:foo(peer@1.0.0)", "foo@file:foo(peer@2.0.0)"),
        ("foo@1.0.0(patch_hash=aaa)(peer@1.0.0)", "foo@1.0.0(patch_hash=bbb)(peer@2.0.0)"),
        ("foo@2.0.0(peer@1.0.0)", "foo@1.0.0(peer@2.0.0)"),
    ] {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("node_modules/foo")).unwrap();
        record_hoisted_locations(dir.path(), &[(recorded, &["node_modules/foo"])]);
        let lockfile: Lockfile = serde_saphyr::from_str(&format!(
            "lockfileVersion: '9.0'\nimporters: {{}}\nsnapshots:\n  {required}: {{}}\n",
        ))
        .unwrap();
        assert!(!tree_intact(dir.path(), NodeLinker::Hoisted, &lockfile), "{required}");
    }
}
