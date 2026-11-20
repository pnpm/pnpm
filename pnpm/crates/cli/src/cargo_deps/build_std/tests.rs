use super::{
    is_enabled,
    read_packages,
    requests_build_std,
};
use crate::cargo_deps::lockfile::{
    LockedCrate,
    LockedPackages,
    parse_lockfile,
};
use sha2::{
    Digest,
    Sha256,
};
use std::fs;
use tempfile::TempDir;

#[test]
fn build_std_is_only_enabled_by_a_nonempty_crate_list() {
    for (contents, expected) in [
        ("[unstable]\nbindeps = true\n", false),
        ("[unstable]\nbuild-std = []\n", false),
        ("[unstable]\nbuild-std = [\"std\"]\n", true),
    ] {
        assert_eq!(requests_build_std(contents).unwrap(), expected);
    }
}

#[test]
fn a_nested_workspace_inherits_the_build_std_request() {
    let parent = TempDir::new().unwrap();
    fs::create_dir(parent.path().join(".cargo")).unwrap();
    fs::write(parent.path().join(".cargo/config"), "[unstable]\nbuild-std = [\"std\"]\n").unwrap();
    let child = parent.path().join("child");
    fs::create_dir(&child).unwrap();

    let enabled = is_enabled(&child, None).unwrap();
    eprintln!("The child workspace must inherit build-std: {enabled}");
    assert!(enabled);
}

/// The inherited configuration is often the developer's own, which is
/// commonly a symlink into a dotfiles repository.
#[cfg(unix)]
#[test]
fn a_symlinked_configuration_in_an_ancestor_enables_build_std() {
    let parent = TempDir::new().unwrap();
    fs::create_dir(parent.path().join(".cargo")).unwrap();
    let elsewhere = parent.path().join("dotfiles-config.toml");
    fs::write(&elsewhere, "[unstable]\nbuild-std = [\"std\"]\n").unwrap();
    std::os::unix::fs::symlink(&elsewhere, parent.path().join(".cargo/config.toml")).unwrap();
    let child = parent.path().join("child");
    fs::create_dir(&child).unwrap();

    let enabled = is_enabled(&child, Some(&child)).unwrap();
    eprintln!("A symlinked configuration must still be read: {enabled}");
    assert!(enabled);
}

#[test]
fn build_std_dependencies_share_identical_locked_project_crates() {
    let sysroot = TempDir::new().unwrap();
    let library = sysroot.path().join("lib/rustlib/src/rust/library");
    fs::create_dir_all(&library).unwrap();
    let checksum = format!("{:x}", Sha256::digest(b"crate source"));
    fs::write(library.join("Cargo.lock"), format!(
        "version = 4\n[[package]]\nname = \"std\"\nversion = \"0.0.0\"\n[[package]]\nname = \"cfg-if\"\nversion = \"1.0.4\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = {checksum:?}\n",
    )).unwrap();
    let project = LockedPackages {
        crates: vec![LockedCrate {
            name: "cfg-if".to_string(),
            version: "1.0.4".to_string(),
            checksum: checksum.clone(),
        }],
        git: Vec::new(),
    };

    let merged = project
        .merge(read_packages(sysroot.path()).unwrap())
        .unwrap();

    assert_eq!(merged.crates.len(), 1);
    assert_eq!(merged.crates[0].checksum, checksum);
}

#[test]
fn conflicting_project_and_standard_library_crates_are_rejected() {
    let package = LockedCrate {
        name: "cfg-if".to_string(),
        version: "1.0.4".to_string(),
        checksum: format!("{:x}", Sha256::digest(b"one")),
    };
    let mut other = package.clone();
    other.checksum = format!("{:x}", Sha256::digest(b"two"));
    let project = LockedPackages { crates: vec![package], git: Vec::new() };
    let library = LockedPackages { crates: vec![other], git: Vec::new() };

    let error = project.merge(library).unwrap_err();

    eprintln!("A conflicting checksum must fail: {error:?}");
    assert!(error.to_string().contains("conflicting registry package cfg-if-1.0.4"));
}

#[test]
fn standard_library_sources_are_independent_of_the_project_registry() {
    let sysroot = TempDir::new().unwrap();
    let library = sysroot.path().join("lib/rustlib/src/rust/library");
    fs::create_dir_all(&library).unwrap();
    let checksum = format!("{:x}", Sha256::digest(b"crate source"));
    let package = format!(
        "version = 4\n[[package]]\nname = \"cfg-if\"\nversion = \"1.0.4\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = {checksum:?}\n",
    );
    fs::write(library.join("Cargo.lock"), &package).unwrap();
    let index_url = "https://registry.example.test/";
    let project_lock = package.replace(
        "registry+https://github.com/rust-lang/crates.io-index",
        &pnpm_cargo_resolver::registry_source(index_url),
    );
    let project = parse_lockfile(&project_lock, index_url).unwrap();

    let merged = project
        .merge(read_packages(sysroot.path()).unwrap())
        .unwrap();

    assert_eq!(merged.crates.len(), 1);
    assert_eq!(merged.crates[0].checksum, checksum);
}
