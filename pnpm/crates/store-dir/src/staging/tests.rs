use super::{remove_abandoned_staging_dirs, staging_dir_name};
use std::fs;
use tempfile::tempdir;

#[test]
fn a_staging_dir_name_is_short_and_hidden() {
    assert_eq!(staging_dir_name("1a2b3c4d"), ".pnpm-1a2b3c4d");
}

#[test]
fn a_staging_dir_an_install_may_still_use_is_kept() {
    let modules_dir = tempdir().expect("temp dir");
    let package_dir = modules_dir.path().join("esbuild");
    let staging = modules_dir
        .path()
        .join(staging_dir_name("1a2b3c4d"));
    fs::create_dir(&staging).expect("create staging dir");

    assert_eq!(remove_abandoned_staging_dirs(&package_dir).expect("sweep"), 0);
    assert!(staging.exists());
}

#[cfg(unix)]
#[test]
fn an_abandoned_staging_dir_is_removed_and_nothing_else() {
    let modules_dir = tempdir().expect("temp dir");
    let package_dir = modules_dir.path().join("esbuild");
    fs::create_dir(&package_dir).expect("create package dir");
    let abandoned = modules_dir
        .path()
        .join(staging_dir_name("1a2b3c4d"));
    let unrelated = modules_dir.path().join(".bin");
    for dir in [&abandoned, &unrelated] {
        fs::create_dir(dir).expect("create dir");
        age(dir);
    }

    assert_eq!(remove_abandoned_staging_dirs(&package_dir).expect("sweep"), 1);
    assert!(!abandoned.exists());
    assert!(package_dir.exists());
    assert!(unrelated.exists(), "a directory without the staging prefix stays");
}

#[cfg(unix)]
#[test]
fn a_staging_dir_a_build_still_writes_into_is_kept() {
    let modules_dir = tempdir().expect("temp dir");
    let package_dir = modules_dir.path().join("esbuild");
    let staging = modules_dir
        .path()
        .join(staging_dir_name("1a2b3c4d"));
    fs::create_dir_all(staging.join("build")).expect("create staging dir");
    fs::write(staging.join("build/out.o"), "").expect("write build output");
    age(&staging.join("build"));
    age(&staging);

    assert_eq!(remove_abandoned_staging_dirs(&package_dir).expect("sweep"), 0);
    assert!(staging.exists());
}

/// Make `dir` look unchanged for two days.
#[cfg(unix)]
fn age(dir: &std::path::Path) {
    let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_hours(48);
    fs::File::open(dir)
        .and_then(|handle| handle.set_modified(two_days_ago))
        .expect("age the dir");
}
