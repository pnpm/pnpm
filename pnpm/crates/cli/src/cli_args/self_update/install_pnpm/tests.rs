use super::{
    PNPM_EXE_PACKAGE_NAME, PNPM_PACKAGE_NAME, assert_release_is_installable,
    exe_platform_pkg_dir_name, exe_platform_pkg_dir_name_next, link_exe_platform_binary,
    package_dir, pnpm_package_to_install, reuse_cached_engine,
};
use pacquet_graph_hasher::{host_arch, host_libc, host_platform};
use std::fs;

#[test]
fn legacy_platform_dir_names() {
    assert_eq!(exe_platform_pkg_dir_name("darwin", "arm64", "unknown"), "macos-arm64");
    assert_eq!(exe_platform_pkg_dir_name("darwin", "x64", "unknown"), "macos-x64");
    assert_eq!(exe_platform_pkg_dir_name("win32", "x64", "unknown"), "win-x64");
    assert_eq!(exe_platform_pkg_dir_name("win32", "ia32", "unknown"), "win-x86");
    assert_eq!(exe_platform_pkg_dir_name("linux", "x64", "glibc"), "linux-x64");
    assert_eq!(exe_platform_pkg_dir_name("linux", "x64", "musl"), "linuxstatic-x64");
    assert_eq!(exe_platform_pkg_dir_name("linux", "arm64", "musl"), "linuxstatic-arm64");
}

#[test]
fn next_platform_dir_names() {
    assert_eq!(exe_platform_pkg_dir_name_next("darwin", "arm64", "unknown"), "exe.darwin-arm64");
    assert_eq!(exe_platform_pkg_dir_name_next("win32", "ia32", "unknown"), "exe.win32-x86");
    assert_eq!(exe_platform_pkg_dir_name_next("linux", "x64", "glibc"), "exe.linux-x64");
    assert_eq!(exe_platform_pkg_dir_name_next("linux", "x64", "musl"), "exe.linux-x64-musl");
    assert_eq!(exe_platform_pkg_dir_name_next("linux", "arm64", "musl"), "exe.linux-arm64-musl");
}

#[test]
fn target_package_name_matches_pnpm_engine_layout() {
    assert_eq!(pnpm_package_to_install("12.0.0-alpha.1").name, PNPM_PACKAGE_NAME);
    assert_eq!(pnpm_package_to_install("12.0.0").name, PNPM_PACKAGE_NAME);
    assert_eq!(pnpm_package_to_install("11.10.0").name, PNPM_EXE_PACKAGE_NAME);
    assert_eq!(pnpm_package_to_install("10.34.4").name, PNPM_EXE_PACKAGE_NAME);
    assert_eq!(pnpm_package_to_install("6.17.1").name, PNPM_EXE_PACKAGE_NAME);
    assert_eq!(pnpm_package_to_install("6.16.0").name, PNPM_PACKAGE_NAME);
    assert_eq!(pnpm_package_to_install("5.18.10").name, PNPM_PACKAGE_NAME);
    assert_eq!(pnpm_package_to_install("not-semver").name, PNPM_EXE_PACKAGE_NAME);
}

#[test]
fn native_binary_linking_matches_pnpm_engine_layout() {
    assert!(pnpm_package_to_install("12.0.0-alpha.1").links_native_binary);
    assert!(pnpm_package_to_install("11.10.0").links_native_binary);
    assert!(pnpm_package_to_install("6.17.1").links_native_binary);
    assert!(!pnpm_package_to_install("6.16.0").links_native_binary);
    assert!(!pnpm_package_to_install("5.18.10").links_native_binary);
    assert!(pnpm_package_to_install("not-semver").links_native_binary);
}

/// Lay out a fake engine install: the `pnpm` wrapper and, under
/// `@pnpm/<host-platform-dir>`, the native binary the wrapper's preinstall
/// would normally link.
fn fake_engine_install(install_dir: &std::path::Path, with_native_binary: bool) {
    fake_engine_install_for(install_dir, PNPM_PACKAGE_NAME, with_native_binary);
}

fn fake_engine_install_for(
    install_dir: &std::path::Path,
    wrapper_pkg_name: &str,
    with_native_binary: bool,
) {
    let node_modules = install_dir.join("node_modules");
    fs::create_dir_all(package_dir(install_dir, wrapper_pkg_name)).expect("create wrapper dir");
    if with_native_binary {
        let platform_dir =
            exe_platform_pkg_dir_name_next(host_platform(), host_arch(), host_libc());
        let src_dir = node_modules.join("@pnpm").join(platform_dir);
        fs::create_dir_all(&src_dir).expect("create platform dir");
        fs::write(src_dir.join("pnpm"), b"#!/bin/sh\necho pnpm\n").expect("write native binary");
    }
}

#[cfg(unix)]
#[test]
fn links_the_host_platform_binary_into_the_wrapper() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    fake_engine_install(temp.path(), true);

    link_exe_platform_binary(temp.path(), "pnpm").expect("linking should succeed");

    let dest = temp.path().join("node_modules").join("pnpm").join("pnpm");
    assert!(dest.exists(), "the native binary is linked into the wrapper");
    assert_eq!(fs::read(&dest).expect("read linked binary"), b"#!/bin/sh\necho pnpm\n");
    let mode = fs::metadata(&dest).expect("stat linked binary").permissions().mode();
    assert_eq!(mode & 0o777, 0o755, "the linked binary is executable");
}

#[cfg(unix)]
#[test]
fn links_the_host_platform_binary_into_scoped_exe_wrapper() {
    let temp = tempfile::tempdir().expect("tempdir");
    fake_engine_install_for(temp.path(), PNPM_EXE_PACKAGE_NAME, true);

    link_exe_platform_binary(temp.path(), PNPM_EXE_PACKAGE_NAME).expect("linking should succeed");

    let dest = package_dir(temp.path(), PNPM_EXE_PACKAGE_NAME).join("pnpm");
    assert!(dest.exists(), "the native binary is linked into the scoped wrapper");
    assert_eq!(fs::read(&dest).expect("read linked binary"), b"#!/bin/sh\necho pnpm\n");
}

/// Lay out the wrapper slot of a fake global-virtual-store engine install
/// (`links/<scope-or-@>/<name>/<version>/<hash>`) and return the slot
/// directory. The platform package is left to each test: the real installer
/// materializes it as a symlink to a sibling slot, which is exactly the
/// resolution under test.
#[cfg(unix)]
fn fake_gvs_wrapper_slot(
    links_dir: &std::path::Path,
    wrapper_pkg_name: &str,
) -> std::path::PathBuf {
    let slot = match wrapper_pkg_name.split_once('/') {
        Some((scope, name)) => links_dir.join(scope).join(name),
        None => links_dir.join("@").join(wrapper_pkg_name),
    }
    .join("12.0.0-alpha.7")
    .join("cafe0123");
    fs::create_dir_all(package_dir(&slot, wrapper_pkg_name)).expect("create wrapper dir");
    fs::create_dir_all(slot.join("node_modules").join("@pnpm")).expect("create scope dir");
    slot
}

/// Materialize the native platform package in its own sibling slot under
/// `links` and return the package directory the wrapper's platform symlink
/// should point at.
#[cfg(unix)]
fn fake_gvs_native_slot(links_dir: &std::path::Path) -> std::path::PathBuf {
    let platform_dir = exe_platform_pkg_dir_name_next(host_platform(), host_arch(), host_libc());
    let native_pkg_dir = links_dir
        .join("@pnpm")
        .join(&platform_dir)
        .join("12.0.0-alpha.7")
        .join("beef4567")
        .join("node_modules")
        .join("@pnpm")
        .join(&platform_dir);
    fs::create_dir_all(&native_pkg_dir).expect("create native package dir");
    fs::write(native_pkg_dir.join("pnpm"), b"#!/bin/sh\necho pnpm\n").expect("write native binary");
    native_pkg_dir
}

/// `packageManager` delegation installs the engine into the global virtual
/// store, where the unscoped `pnpm` wrapper sits under the `@` placeholder
/// scope and its platform package legitimately resolves into a sibling slot
/// — the trust root must widen to `links` instead of the wrapper's own slot.
#[cfg(unix)]
#[test]
fn links_native_binary_from_a_sibling_global_virtual_store_slot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let links_dir = temp.path().join("links");
    let slot = fake_gvs_wrapper_slot(&links_dir, "pnpm");
    let native_pkg_dir = fake_gvs_native_slot(&links_dir);
    let platform_dir = exe_platform_pkg_dir_name_next(host_platform(), host_arch(), host_libc());
    std::os::unix::fs::symlink(
        &native_pkg_dir,
        slot.join("node_modules").join("@pnpm").join(platform_dir),
    )
    .expect("symlink platform package to the sibling slot");

    link_exe_platform_binary(&slot, "pnpm").expect("linking should succeed");

    let dest = package_dir(&slot, "pnpm").join("pnpm");
    assert!(dest.exists(), "the native binary is linked into the wrapper");
    assert_eq!(fs::read(&dest).expect("read linked binary"), b"#!/bin/sh\necho pnpm\n");
}

#[cfg(unix)]
#[test]
fn links_native_binary_from_a_sibling_slot_into_the_scoped_wrapper() {
    let temp = tempfile::tempdir().expect("tempdir");
    let links_dir = temp.path().join("links");
    let slot = fake_gvs_wrapper_slot(&links_dir, PNPM_EXE_PACKAGE_NAME);
    let native_pkg_dir = fake_gvs_native_slot(&links_dir);
    let platform_dir = exe_platform_pkg_dir_name_next(host_platform(), host_arch(), host_libc());
    std::os::unix::fs::symlink(
        &native_pkg_dir,
        slot.join("node_modules").join("@pnpm").join(platform_dir),
    )
    .expect("symlink platform package to the sibling slot");

    link_exe_platform_binary(&slot, PNPM_EXE_PACKAGE_NAME).expect("linking should succeed");

    assert!(package_dir(&slot, PNPM_EXE_PACKAGE_NAME).join("pnpm").exists());
}

/// A platform-package symlink that leaves `links` entirely must still be
/// rejected even when the wrapper sits in a global-virtual-store slot.
#[cfg(unix)]
#[test]
fn rejects_native_binary_that_escapes_the_global_virtual_store() {
    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    let links_dir = temp.path().join("links");
    let slot = fake_gvs_wrapper_slot(&links_dir, "pnpm");
    let platform_dir = exe_platform_pkg_dir_name_next(host_platform(), host_arch(), host_libc());
    let outside_pkg_dir = outside.path().join(&platform_dir);
    fs::create_dir_all(&outside_pkg_dir).expect("create outside package dir");
    fs::write(outside_pkg_dir.join("pnpm"), b"outside").expect("write outside binary");
    std::os::unix::fs::symlink(
        &outside_pkg_dir,
        slot.join("node_modules").join("@pnpm").join(platform_dir),
    )
    .expect("symlink platform package outside the store");

    let err = link_exe_platform_binary(&slot, "pnpm").expect_err("escaped native source rejected");
    assert!(err.to_string().contains("resolves outside"), "unexpected error: {err:?}");
    assert!(!package_dir(&slot, "pnpm").join("pnpm").exists());
}

#[cfg(unix)]
#[test]
fn rejects_wrapper_symlink_that_escapes_the_install_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    let outside_wrapper = outside.path().join("exe");
    fs::create_dir_all(&outside_wrapper).expect("create outside wrapper");
    fs::write(outside_wrapper.join("pnpm"), b"outside").expect("write outside placeholder");

    fs::create_dir_all(temp.path().join("node_modules").join("@pnpm")).expect("create scope dir");
    std::os::unix::fs::symlink(&outside_wrapper, package_dir(temp.path(), PNPM_EXE_PACKAGE_NAME))
        .expect("symlink wrapper outside install dir");

    let err = link_exe_platform_binary(temp.path(), PNPM_EXE_PACKAGE_NAME)
        .expect_err("escaped wrapper must be rejected");
    assert!(err.to_string().contains("resolves outside"), "unexpected error: {err:?}");
    assert_eq!(fs::read(outside_wrapper.join("pnpm")).expect("read outside file"), b"outside");
}

#[cfg(unix)]
#[test]
fn rejects_native_binary_symlink_that_escapes_the_install_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    let outside_binary = outside.path().join("pnpm");
    fs::write(&outside_binary, b"outside").expect("write outside binary");
    fake_engine_install(temp.path(), false);

    let platform_dir = exe_platform_pkg_dir_name_next(host_platform(), host_arch(), host_libc());
    let src_dir = temp.path().join("node_modules").join("@pnpm").join(platform_dir);
    fs::create_dir_all(&src_dir).expect("create platform dir");
    std::os::unix::fs::symlink(&outside_binary, src_dir.join("pnpm"))
        .expect("symlink native binary outside install dir");

    let err =
        link_exe_platform_binary(temp.path(), "pnpm").expect_err("escaped native source rejected");
    assert!(err.to_string().contains("is a symlink"), "unexpected error: {err:?}");
    assert!(!package_dir(temp.path(), "pnpm").join("pnpm").exists());
    assert_eq!(fs::read(outside_binary).expect("read outside file"), b"outside");
}

#[cfg(unix)]
#[test]
fn rejects_native_binary_scope_symlink_that_escapes_the_install_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    fake_engine_install(temp.path(), false);

    let platform_dir = exe_platform_pkg_dir_name_next(host_platform(), host_arch(), host_libc());
    let outside_scope = outside.path().join("@pnpm");
    let outside_platform_dir = outside_scope.join(platform_dir);
    fs::create_dir_all(&outside_platform_dir).expect("create outside platform dir");
    fs::write(outside_platform_dir.join("pnpm"), b"outside").expect("write outside binary");
    std::os::unix::fs::symlink(&outside_scope, temp.path().join("node_modules").join("@pnpm"))
        .expect("symlink native scope outside install dir");

    let err =
        link_exe_platform_binary(temp.path(), "pnpm").expect_err("escaped native source rejected");
    assert!(err.to_string().contains("resolves outside"), "unexpected error: {err:?}");
    assert!(!package_dir(temp.path(), "pnpm").join("pnpm").exists());
}

#[test]
fn link_errors_when_the_native_binary_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    // Wrapper present, but no `@pnpm/<platform>` native binary — linking must
    // fail loudly rather than leave a broken "successful" self-update.
    fake_engine_install(temp.path(), false);

    assert!(link_exe_platform_binary(temp.path(), "pnpm").is_err());
}

/// Write a wrapper `package.json` recording `version` so
/// [`super::installed_version`] reads it back.
fn write_wrapper_version(install_dir: &std::path::Path, wrapper_pkg_name: &str, version: &str) {
    let manifest = format!(r#"{{"name":"{wrapper_pkg_name}","version":"{version}"}}"#);
    fs::write(package_dir(install_dir, wrapper_pkg_name).join("package.json"), manifest)
        .expect("write wrapper package.json");
}

#[cfg(unix)]
#[test]
fn reuse_cached_engine_accepts_a_healthy_slot() {
    let temp = tempfile::tempdir().expect("tempdir");
    fake_engine_install_for(temp.path(), PNPM_EXE_PACKAGE_NAME, true);
    write_wrapper_version(temp.path(), PNPM_EXE_PACKAGE_NAME, "11.10.0");

    assert!(reuse_cached_engine(temp.path(), pnpm_package_to_install("11.10.0"), "11.10.0"));
    // The relink repaired the slot in place: the native binary is now linked.
    assert!(package_dir(temp.path(), PNPM_EXE_PACKAGE_NAME).join("pnpm").exists());
}

#[test]
fn reuse_cached_engine_rejects_a_version_mismatch() {
    let temp = tempfile::tempdir().expect("tempdir");
    fake_engine_install_for(temp.path(), PNPM_EXE_PACKAGE_NAME, true);
    write_wrapper_version(temp.path(), PNPM_EXE_PACKAGE_NAME, "11.9.0");

    assert!(!reuse_cached_engine(temp.path(), pnpm_package_to_install("11.10.0"), "11.10.0"));
}

/// The pin is committed and shared while the wrapper is not, so a release whose
/// `@pnpm/exe` cannot run must be refused for every wrapper — refusing only the
/// one that breaks would let a JS user pin it for the whole team.
#[test]
fn assert_release_is_installable_refuses_the_broken_releases() {
    for version in ["11.12.0", "11.13.0"] {
        let err = assert_release_is_installable(version).unwrap_err();
        assert!(err.to_string().contains("broken release"), "{err}");
    }
}

#[test]
fn assert_release_is_installable_allows_every_other_release() {
    for version in ["11.11.0", "11.13.1", "12.0.0"] {
        assert_release_is_installable(version).unwrap();
    }
}

/// A slot left by an older layout whose wrapper symlink escapes the slot
/// (e.g. into a shared global virtual store) must not be reused — the
/// caller falls through to a fresh install instead of aborting the whole
/// self-update on the wrapper-containment guard.
#[cfg(unix)]
#[test]
fn reuse_cached_engine_rejects_a_wrapper_that_escapes_the_slot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    let outside_wrapper = outside.path().join("exe");
    fs::create_dir_all(&outside_wrapper).expect("create outside wrapper");
    fs::write(outside_wrapper.join("package.json"), r#"{"name":"@pnpm/exe","version":"11.10.0"}"#)
        .expect("write outside wrapper manifest");

    fs::create_dir_all(temp.path().join("node_modules").join("@pnpm")).expect("create scope dir");
    std::os::unix::fs::symlink(&outside_wrapper, package_dir(temp.path(), PNPM_EXE_PACKAGE_NAME))
        .expect("symlink wrapper outside slot");

    // The recorded version matches, but the wrapper resolves outside the
    // slot, so the slot is not reusable.
    assert!(!reuse_cached_engine(temp.path(), pnpm_package_to_install("11.10.0"), "11.10.0"));
}
