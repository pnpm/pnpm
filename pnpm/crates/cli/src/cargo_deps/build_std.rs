use super::{
    lockfile::{LockedPackages, parse_lockfile},
    resolution::configs_in_scope,
};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_network::redact_and_sanitize_multiline;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(super) async fn include_packages(
    root: &Path,
    checkout: Option<&Path>,
    packages: LockedPackages,
) -> Result<LockedPackages> {
    if !is_enabled(root, checkout)? {
        return Ok(packages);
    }
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let sysroot = sysroot(&root)?;
        packages.merge(read_packages(&sysroot)?)
    })
    .await
    .into_diagnostic()
    .wrap_err("join Cargo build-std dependency discovery")?
}

fn is_enabled(root: &Path, checkout: Option<&Path>) -> Result<bool> {
    for contents in configs_in_scope(root, checkout) {
        if requests_build_std(&contents?)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn requests_build_std(contents: &str) -> Result<bool> {
    let document: toml::Table = toml::from_str(contents)
        .into_diagnostic()
        .wrap_err("parse Cargo configuration for build-std")?;
    let Some(crates) = document
        .get("unstable")
        .and_then(|unstable| unstable.get("build-std"))
    else {
        return Ok(false);
    };
    let crates = crates
        .as_array()
        .ok_or_else(|| miette::miette!("Cargo unstable.build-std must be an array"))?;
    Ok(!crates.is_empty())
}

pub(super) fn sysroot(root: &Path) -> Result<PathBuf> {
    let output = Command::new("rustc")
        .current_dir(root)
        .args(["--print", "sysroot"])
        .output()
        .into_diagnostic()
        .wrap_err("find Rust sysroot for Cargo build-std")?;
    if !output.status.success() {
        let stderr = redact_and_sanitize_multiline(&String::from_utf8_lossy(&output.stderr));
        let stderr = stderr.trim();
        let root = root.display();
        return Err(miette::miette!("find Rust sysroot for {root}: {stderr}"));
    }
    let path =
        String::from_utf8(output.stdout).into_diagnostic().wrap_err("decode Rust sysroot")?;
    if path.trim().is_empty() {
        let root = root.display();
        return Err(miette::miette!("rustc returned an empty sysroot for {root}"));
    }
    Ok(PathBuf::from(path.trim()))
}

fn read_packages(sysroot: &Path) -> Result<LockedPackages> {
    let path = sysroot.join("lib/rustlib/src/rust/library/Cargo.lock");
    let contents = fs::read_to_string(&path)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "read {} for Cargo build-std; install the toolchain's rust-src component",
                path.display(),
            )
        })?;
    parse_lockfile(&contents, pnpm_cargo_resolver::CRATES_IO_SPARSE_INDEX)
        .wrap_err_with(|| format!("parse {} for Cargo build-std", path.display()))
}

#[cfg(test)]
mod tests;
