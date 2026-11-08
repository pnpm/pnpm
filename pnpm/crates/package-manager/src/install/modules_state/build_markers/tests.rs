use super::{MarkerProbe, hash_dir_marker};
use pnpm_lockfile::PackageKey;
use std::fs;
use tempfile::tempdir;

#[test]
fn marker_probe_does_not_create_a_slot_lock() {
    let root = tempdir().expect("temporary store");
    let version_dir = root.path().join("foo@1.0.0");
    let slot_dir = version_dir.join("hash");
    let package_dir = slot_dir.join("node_modules/foo");
    fs::create_dir_all(&package_dir).expect("create package directory");
    fs::write(package_dir.join("package.json"), "{}").expect("write completion marker");
    let hash_dir = fs::read_dir(&version_dir)
        .expect("read version directory")
        .next()
        .expect("slot directory")
        .expect("read slot directory");
    let key: PackageKey = "foo@1.0.0".parse().expect("package key");

    let probe = hash_dir_marker(Ok(hash_dir), &key);

    assert!(matches!(probe, MarkerProbe::None));
    assert!(!slot_dir.join(".pnpm-build.lock").exists());
}
