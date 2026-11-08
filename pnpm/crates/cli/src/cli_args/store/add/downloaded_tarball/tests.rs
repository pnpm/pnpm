use super::{expand_tarball_directories, registry_package_ids, tarball_on_disk};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{fs, path::Path};

#[test]
fn a_semver_version_names_one_registry_package() {
    let manifest = json!({ "name": "acme", "version": "1.0.0" });
    assert_eq!(registry_package_ids(Some(&manifest), []), ["acme@1.0.0"]);
}

#[test]
fn a_v_prefixed_version_also_names_the_version_semver_writes() {
    let manifest = json!({ "name": "acme", "version": "v1.0.0" });
    assert_eq!(registry_package_ids(Some(&manifest), []), ["acme@v1.0.0", "acme@1.0.0"]);
}

#[test]
fn a_manifest_without_a_version_names_no_registry_package() {
    let manifest = json!({ "name": "acme" });
    assert_eq!(registry_package_ids(Some(&manifest), []), Vec::<String>::new());
}

#[test]
fn each_declared_prefix_names_a_registry_package() {
    let manifest = json!({ "name": "acme", "version": "v1.0.0" });
    assert_eq!(
        registry_package_ids(Some(&manifest), ["home", "work"]),
        ["acme@v1.0.0", "acme@1.0.0", "acme@home:1.0.0", "acme@work:1.0.0"],
    );
}

#[test]
fn a_version_semver_rejects_names_no_prefixed_registry_package() {
    let manifest = json!({ "name": "acme", "version": "latest" });
    assert_eq!(registry_package_ids(Some(&manifest), ["work"]), ["acme@latest"]);
}

#[test]
fn a_local_tarball_is_read_from_the_lockfile_dir() {
    let lockfile_dir = Path::new("/project");
    assert_eq!(
        tarball_on_disk("local-filesystem", "file:vendor/acme-1.0.0.tgz", lockfile_dir),
        Some(lockfile_dir.join("vendor/acme-1.0.0.tgz")),
    );
}

#[test]
fn a_registry_resolution_to_a_file_url_is_not_a_downloaded_tarball() {
    let lockfile_dir = Path::new("/project");
    assert_eq!(tarball_on_disk("npm-registry", "file:vendor/acme-1.0.0.tgz", lockfile_dir), None);
}

#[test]
fn a_directory_of_tarballs_expands_recursively_in_sorted_order() {
    let root = tempfile::tempdir().expect("create temp dir");
    let tarballs = root.path().join("tarballs");
    fs::create_dir_all(tarballs.join("@scope")).expect("create scoped subdir");
    fs::write(tarballs.join("zeta-1.0.0.tar"), b"").expect("write .tar");
    fs::write(tarballs.join("alpha-1.0.0.tar.gz"), b"").expect("write .tar.gz");
    fs::write(tarballs.join("@scope").join("pkg-1.0.0.tgz"), b"").expect("write .tgz");
    fs::write(tarballs.join("README.md"), b"ignored").expect("write non-tarball");

    let packages =
        vec!["./tarballs".to_owned(), r".\tarballs".to_owned(), "is-odd@3.0.1".to_owned()];
    let scoped_tgz = tarballs.join("@scope").join("pkg-1.0.0.tgz");
    let expected_tarballs = [
        format!("file:{}", tarballs.join("alpha-1.0.0.tar.gz").display()),
        format!("file:{}", tarballs.join("zeta-1.0.0.tar").display()),
        format!("file:{}", scoped_tgz.display()),
    ];
    assert_eq!(
        expand_tarball_directories(root.path(), &packages).expect("expand tarball directories"),
        [
            expected_tarballs[0].clone(),
            expected_tarballs[1].clone(),
            expected_tarballs[2].clone(),
            expected_tarballs[0].clone(),
            expected_tarballs[1].clone(),
            expected_tarballs[2].clone(),
            "is-odd@3.0.1".to_owned(),
        ],
    );
}

#[test]
fn a_directory_named_like_an_archive_still_expands() {
    let root = tempfile::tempdir().expect("create temp dir");
    let archive_named_dir = root.path().join("cache.tar");
    fs::create_dir_all(&archive_named_dir).expect("create cache.tar dir");
    fs::write(archive_named_dir.join("pkg-1.0.0.tgz"), b"").expect("write .tgz");

    let packages = vec!["./cache.tar".to_owned()];
    assert_eq!(
        expand_tarball_directories(root.path(), &packages).expect("expand archive-named dir"),
        [format!("file:{}", archive_named_dir.join("pkg-1.0.0.tgz").display())],
    );
}

#[test]
fn a_directory_with_package_json_or_no_tarballs_or_a_bare_package_name_stays_unexpanded() {
    let root = tempfile::tempdir().expect("create temp dir");
    let local_pkg = root.path().join("local-pkg");
    fs::create_dir_all(&local_pkg).expect("create local package dir");
    fs::write(local_pkg.join("package.json"), "{}").expect("write package.json");
    fs::write(local_pkg.join("bundled-1.0.0.tgz"), b"").expect("write bundled tarball");
    let empty_dir = root.path().join("empty");
    fs::create_dir_all(&empty_dir).expect("create empty dir");
    let bare_named_dir = root.path().join("lodash");
    fs::create_dir_all(&bare_named_dir).expect("create bare-named dir");
    fs::write(bare_named_dir.join("lodash-4.17.21.tgz"), b"").expect("write tarball");

    let packages = vec!["./local-pkg".to_owned(), "./empty".to_owned(), "lodash".to_owned()];
    assert_eq!(
        expand_tarball_directories(root.path(), &packages).expect("expand directories"),
        packages,
    );
}

#[test]
fn a_symlink_cycle_in_a_tarball_directory_is_visited_once() {
    let root = tempfile::tempdir().expect("create temp dir");
    let tarballs = root.path().join("tarballs");
    let nested = tarballs.join("nested");
    fs::create_dir_all(&nested).expect("create nested dir");
    fs::write(tarballs.join("pkg-1.0.0.tgz"), b"").expect("write tarball");
    pnpm_fs::symlink_dir(&tarballs, &nested.join("loop")).expect("create symlink cycle");

    let packages = vec!["./tarballs".to_owned()];
    assert_eq!(
        expand_tarball_directories(root.path(), &packages).expect("expand symlink cycle"),
        [format!("file:{}", tarballs.join("pkg-1.0.0.tgz").display())],
    );
}

#[test]
fn a_symlink_outside_the_tarball_directory_is_ignored() {
    let root = tempfile::tempdir().expect("create temp dir");
    let tarballs = root.path().join("tarballs");
    let outside = root.path().join("outside");
    fs::create_dir_all(&tarballs).expect("create tarballs dir");
    fs::create_dir_all(&outside).expect("create outside dir");
    fs::write(tarballs.join("inside-1.0.0.tgz"), b"").expect("write inside tarball");
    fs::write(outside.join("outside-1.0.0.tgz"), b"").expect("write outside tarball");
    pnpm_fs::symlink_dir(&outside, &tarballs.join("escaped")).expect("link outside dir");

    let packages = vec!["./tarballs".to_owned()];
    assert_eq!(
        expand_tarball_directories(root.path(), &packages).expect("expand confined dir"),
        [format!("file:{}", tarballs.join("inside-1.0.0.tgz").display())],
    );
}

#[cfg(unix)]
#[test]
fn an_unreadable_subdirectory_in_a_tarball_directory_returns_an_error() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("create temp dir");
    let tarballs = root.path().join("tarballs");
    let locked = tarballs.join("locked");
    fs::create_dir_all(&locked).expect("create locked dir");
    fs::write(tarballs.join("inside-1.0.0.tgz"), b"").expect("write inside tarball");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("lock subdir");
    if fs::read_dir(&locked).is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("restore subdir");
        return;
    }

    let packages = vec!["./tarballs".to_owned()];
    let err = expand_tarball_directories(root.path(), &packages)
        .expect_err("unreadable subdirectory must fail");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("restore subdir");
    assert!(err.to_string().contains("read tarball directory"), "err={err}");
}
