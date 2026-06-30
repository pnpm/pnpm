use super::{exe_platform_pkg_dir_name, exe_platform_pkg_dir_name_next, link_exe_platform_binary};
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

/// Lay out a fake engine install: the `pnpm` wrapper and, under
/// `@pnpm/<host-platform-dir>`, the native binary the wrapper's preinstall
/// would normally link.
fn fake_engine_install(install_dir: &std::path::Path, with_native_binary: bool) {
    let node_modules = install_dir.join("node_modules");
    fs::create_dir_all(node_modules.join("pnpm")).expect("create wrapper dir");
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

#[test]
fn link_errors_when_the_native_binary_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    // Wrapper present, but no `@pnpm/<platform>` native binary — linking must
    // fail loudly rather than leave a broken "successful" self-update.
    fake_engine_install(temp.path(), false);

    assert!(link_exe_platform_binary(temp.path(), "pnpm").is_err());
}
