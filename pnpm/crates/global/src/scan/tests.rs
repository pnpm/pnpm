use super::{
    GlobalPackageInfo, find_global_package, get_installed_bin_names,
    get_installed_bin_names_with_fs, read_direct_dependency_aliases, read_installed_packages,
    scan_global_packages,
};
use pnpm_cmd_shim::FsReadFile;
use pnpm_package_manifest::PackageManifestError;
use serde_json::json;
use std::{io, path::Path};
use tempfile::TempDir;

fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::create_dir_all(path.parent().expect("json path has a parent")).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn package_group(install_dir: &Path, aliases: &[&str]) -> GlobalPackageInfo {
    GlobalPackageInfo {
        hash: "hashkey".to_string(),
        install_dir: install_dir.to_path_buf(),
        dependencies: aliases
            .iter()
            .map(|alias| ((*alias).to_string(), "1.0.0".to_string()))
            .collect(),
    }
}

/// Populate `install_dir` as a global group holding a downloaded Node.js
/// runtime: the manifest stores it under `engines.runtime` (the shape the
/// manifest writer folds `node: runtime:<v>` into), and the runtime is
/// materialized under `node_modules/node` with a synthesized `bin`.
fn write_runtime_group(install_dir: &Path) {
    write_json(
        &install_dir.join("package.json"),
        &json!({
            "engines": {
                "runtime": { "name": "node", "version": "22.11.0", "onFail": "download" },
            },
        }),
    );
    write_json(
        &install_dir.join("node_modules/node/package.json"),
        &json!({ "name": "node", "version": "22.11.0", "bin": { "node": "bin/node" } }),
    );
}

#[test]
fn runtime_engines_are_reified_as_a_direct_dependency() {
    let tmp = TempDir::new().unwrap();
    write_runtime_group(tmp.path());

    assert_eq!(read_direct_dependency_aliases(tmp.path()), vec!["node".to_string()]);

    let pkgs = read_installed_packages(tmp.path());
    assert_eq!(pkgs.len(), 1);
    assert_eq!(pkgs[0].location, tmp.path().join("node_modules/node"));
    assert_eq!(pkgs[0].manifest.get("bin"), Some(&json!({ "node": "bin/node" })));
}

#[test]
fn engines_runtime_without_download_is_not_treated_as_installed() {
    // A group whose manifest merely declares an engine *check*
    // (`onFail: "warn"`) has not downloaded a runtime, so it must not be
    // reified into a dependency and mistaken for an installed runtime.
    let tmp = TempDir::new().unwrap();
    write_json(
        &tmp.path().join("package.json"),
        &json!({
            "engines": {
                "runtime": { "name": "node", "version": "22.11.0", "onFail": "warn" },
            },
        }),
    );

    assert!(read_direct_dependency_aliases(tmp.path()).is_empty());
    assert!(read_installed_packages(tmp.path()).is_empty());
}

#[test]
fn ordinary_engines_node_range_is_not_reified() {
    // A plain `engines.node` version constraint (very common in real
    // packages) is not a downloaded runtime, so reification must leave it
    // out of the dependency aliases and installed packages entirely.
    let tmp = TempDir::new().unwrap();
    write_json(&tmp.path().join("package.json"), &json!({ "engines": { "node": ">=18" } }));

    assert!(read_direct_dependency_aliases(tmp.path()).is_empty());
    assert!(read_installed_packages(tmp.path()).is_empty());
}

#[test]
fn installed_bin_names_accepts_a_readable_binless_manifest() {
    let tmp = TempDir::new().unwrap();
    write_json(
        &tmp.path().join("node_modules/binless/package.json"),
        &json!({ "name": "binless", "version": "1.0.0" }),
    );
    let info = package_group(tmp.path(), &["binless"]);

    assert_eq!(get_installed_bin_names(&info).unwrap(), Vec::<String>::new());
}

#[test]
fn installed_bin_names_rejects_a_missing_declared_alias_manifest() {
    let tmp = TempDir::new().unwrap();
    let info = package_group(tmp.path(), &["missing"]);

    assert!(get_installed_bin_names(&info).is_err());
}

#[test]
fn installed_bin_names_rejects_a_malformed_declared_alias_manifest() {
    let tmp = TempDir::new().unwrap();
    let manifest_path = tmp.path().join("node_modules/malformed/package.json");
    std::fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    std::fs::write(manifest_path, "{ not valid JSON").unwrap();
    let info = package_group(tmp.path(), &["malformed"]);

    assert!(get_installed_bin_names(&info).is_err());
}

#[test]
fn installed_bin_names_does_not_return_a_partial_set_when_one_manifest_is_missing() {
    let tmp = TempDir::new().unwrap();
    write_json(
        &tmp.path().join("node_modules/readable/package.json"),
        &json!({
            "name": "readable",
            "version": "1.0.0",
            "bin": { "readable-command": "bin/cli.js" },
        }),
    );
    let info = package_group(tmp.path(), &["readable", "missing"]);

    assert!(get_installed_bin_names(&info).is_err());

    write_json(
        &tmp.path().join("node_modules/missing/package.json"),
        &json!({
            "name": "missing",
            "version": "1.0.0",
            "bin": { "missing-command": "bin/cli.js" },
        }),
    );
    assert_eq!(
        get_installed_bin_names(&info).unwrap(),
        vec!["missing-command".to_string(), "readable-command".to_string()],
    );
}

#[test]
fn installed_bin_names_preserves_permission_denied_manifest_reads() {
    struct PermissionDeniedManifestRead;

    impl FsReadFile for PermissionDeniedManifestRead {
        fn read_file(_: &Path) -> io::Result<Vec<u8>> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }

    let tmp = TempDir::new().unwrap();
    let info = package_group(tmp.path(), &["unreadable"]);
    let error = get_installed_bin_names_with_fs::<PermissionDeniedManifestRead>(&info)
        .expect_err("permission denied must fail ownership enumeration");

    assert!(
        matches!(error, PackageManifestError::Io(error) if error.kind() == io::ErrorKind::PermissionDenied)
    );
}

#[cfg(unix)]
#[test]
fn scan_finds_a_globally_installed_runtime() {
    let global_dir = TempDir::new().unwrap();
    let install_dir = global_dir.path().join("install-abc");
    write_runtime_group(&install_dir);
    std::os::unix::fs::symlink(&install_dir, global_dir.path().join("hashkey")).unwrap();

    let groups = scan_global_packages(global_dir.path()).unwrap();
    assert_eq!(groups.len(), 1);
    assert!(groups[0].has_alias("node"));
    assert_eq!(get_installed_bin_names(&groups[0]).unwrap(), vec!["node".to_string()],);

    assert!(find_global_package(global_dir.path(), "node").unwrap().is_some());
}
