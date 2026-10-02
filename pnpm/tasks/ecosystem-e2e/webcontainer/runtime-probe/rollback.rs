use crate::metadata_file::MetadataFile;
use std::{fs, path::Path};

pub fn check(directory: &Path) {
    check_manifest_write(directory);
    let manifest = directory.join("rollback-package.json");
    fs::write(&manifest, "original").unwrap();
    MetadataFile::capture(manifest.clone())
        .unwrap()
        .restore()
        .unwrap();
    let snapshot = MetadataFile::capture(manifest.clone()).unwrap();
    fs::write(&manifest, "changed").unwrap();
    let error = snapshot
        .restore()
        .unwrap_err()
        .to_string();
    assert!(error.contains("pinned-directory metadata rollback"));
    assert_eq!(fs::read_to_string(&manifest).unwrap(), "changed");
    let snapshot = MetadataFile::capture(manifest.clone()).unwrap();
    let victim = directory.join("rollback-victim");
    fs::write(&victim, "untouched").unwrap();
    fs::remove_file(&manifest).unwrap();
    pnpm_fs::create_symlink(&victim, &manifest, false).unwrap();
    assert!(snapshot.restore().is_err());
    assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched");
    fs::remove_file(manifest).unwrap();
    fs::remove_file(victim).unwrap();
    println!("Unsupported metadata rollback reports failure without unsafe writes");
}

fn check_manifest_write(directory: &Path) {
    let path = directory.join("package.json");
    let mut manifest = pnpm_package_manifest::PackageManifest::from_value(
        path.clone(),
        serde_json::json!({"name": "wasm-probe"}),
    );
    manifest.save().unwrap();
    pnpm_fs::file_mode::set_path_permissions(&path, 0o600).unwrap();
    manifest.value_mut()["version"] = serde_json::json!("1.0.0");
    manifest.save().unwrap();
    assert_eq!(pnpm_fs::copy_permissions(&path).unwrap() & 0o777, 0o600);
    let contents: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(contents["version"], "1.0.0");
    fs::remove_file(path).unwrap();
    println!("Manifest creation and rewrite preserve private permissions");
}
