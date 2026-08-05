use super::{link_cached_engine_bins, package_dir, package_manager_engine_config};
use pacquet_config::Config;
use pacquet_graph_hasher::{host_arch, host_libc, host_platform};
use pacquet_store_dir::StoreDir;
use std::{fs, path::Path};

#[test]
fn cache_hit_relinks_missing_pnpm_bin() {
    let root = tempfile::TempDir::new().expect("tmp dir");
    let slot = root.path().join("slot");
    let pkg_dir = package_dir(&slot, "pnpm");
    fs::create_dir_all(pkg_dir.join("bin")).expect("create package bin dir");
    fs::write(
        pkg_dir.join("package.json"),
        r#"{"name":"pnpm","version":"6.16.0","bin":{"pnpm":"bin/pnpm.cjs"}}"#,
    )
    .expect("write manifest");
    fs::write(pkg_dir.join("bin").join("pnpm.cjs"), "#!/usr/bin/env node\n")
        .expect("write pnpm bin");
    let bin_dir = slot.join("bin");
    fs::create_dir_all(&bin_dir).expect("create stale bin dir");

    let linked = link_cached_engine_bins(&slot, "pnpm", false).expect("link bins");

    assert_eq!(linked, bin_dir);
    let pnpm_bin = bin_dir.join("pnpm");
    assert!(pnpm_bin.exists(), "expected pnpm bin at {}", pnpm_bin.display());
}

#[test]
fn cache_hit_relinks_legacy_wrapper_native_binary() {
    let root = tempfile::TempDir::new().expect("tmp dir");
    let slot = root.path().join("slot");
    let pkg_dir = package_dir(&slot, "@pnpm/exe");
    fs::create_dir_all(&pkg_dir).expect("create wrapper dir");
    fs::write(
        pkg_dir.join("package.json"),
        r#"{"name":"@pnpm/exe","version":"11.1.2","bin":{"pnpm":"pnpm"}}"#,
    )
    .expect("write manifest");
    write_host_native_binaries(&slot);
    let bin_dir = slot.join("bin");
    fs::create_dir_all(&bin_dir).expect("create stale bin dir");

    let linked = link_cached_engine_bins(&slot, "@pnpm/exe", true).expect("link bins");

    assert_eq!(linked, bin_dir);
    let wrapper_bin = pkg_dir.join(host_executable());
    assert!(wrapper_bin.exists(), "expected native wrapper at {}", wrapper_bin.display());
    let pnpm_bin = bin_dir.join("pnpm");
    assert!(pnpm_bin.exists(), "expected pnpm bin at {}", pnpm_bin.display());
}

#[test]
fn package_manager_engine_config_uses_global_store() {
    let root = tempfile::TempDir::new().expect("tmp dir");
    let project_store_root = root.path().join("repo-controlled-store");
    let global_pkg_dir = root.path().join("pnpm-home").join("global").join("v11");
    let config = Config {
        global_pkg_dir: Some(global_pkg_dir),
        store_dir: StoreDir::new(&project_store_root),
        ..Config::default()
    };

    let engine_config = package_manager_engine_config(&config).expect("engine config");

    let expected_store_root =
        root.path().join("pnpm-home").join("package-manager-store").join("v11");
    assert_eq!(engine_config.store_dir.root(), expected_store_root.as_path());
    assert!(
        !engine_config.store_dir.root().starts_with(&project_store_root),
        "engine store must not use project store at {}",
        project_store_root.display(),
    );
}

fn write_host_native_binaries(slot: &Path) {
    let executable = host_executable();
    for platform_dir_name in platform_package_dir_names() {
        let platform_dir = slot.join("node_modules").join("@pnpm").join(platform_dir_name);
        fs::create_dir_all(&platform_dir).expect("create platform dir");
        fs::write(platform_dir.join(executable), b"#!/bin/sh\necho pnpm\n")
            .expect("write native binary");
    }
}

fn host_executable() -> &'static str {
    if host_platform() == "win32" { "pnpm.exe" } else { "pnpm" }
}

fn platform_package_dir_names() -> [String; 2] {
    let platform = host_platform();
    let architecture = normalized_arch(platform, host_arch());
    let libc = host_libc();
    let legacy_platform = match platform {
        "darwin" => "macos",
        "win32" => "win",
        "linux" if libc == "musl" => "linuxstatic",
        "linux" => "linux",
        other => other,
    };
    let libc_suffix = if platform == "linux" && libc == "musl" { "-musl" } else { "" };
    [
        format!("{legacy_platform}-{architecture}"),
        format!("exe.{platform}-{architecture}{libc_suffix}"),
    ]
}

fn normalized_arch<'architecture>(
    platform: &str,
    architecture: &'architecture str,
) -> &'architecture str {
    if platform == "win32" && architecture == "ia32" { "x86" } else { architecture }
}
