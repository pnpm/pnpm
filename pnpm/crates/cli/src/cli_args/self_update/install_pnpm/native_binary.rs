use super::{
    Context, IntoDiagnostic, Path, PathBuf, Value, format_global_virtual_store_path, fs, host_arch,
    host_libc, host_platform, package_dir, parse_manifest, replace_executable,
};

/// Scope-local directory name of the `@pnpm/exe` platform package under
/// the legacy `<os>-<arch>` scheme (`macos-arm64`, `win-x86`,
/// `linux-x64`, `linuxstatic-x64`).
pub(in super::super) fn exe_platform_pkg_dir_name(
    platform: &str,
    arch: &str,
    libc: &str,
) -> String {
    let arch = normalized_arch(platform, arch);
    let os = match platform {
        "darwin" => "macos",
        "win32" => "win",
        "linux" => {
            if libc == "musl" {
                "linuxstatic"
            } else {
                "linux"
            }
        }
        other => other,
    };
    format!("{os}-{arch}")
}

/// Scope-local directory name of the platform package under the
/// `exe.<platform>-<arch>[-musl]` scheme — the convention pnpm v12 ships
/// its native binaries under.
pub(in super::super) fn exe_platform_pkg_dir_name_next(
    platform: &str,
    arch: &str,
    libc: &str,
) -> String {
    format!("exe.{}", native_target_name(platform, arch, libc))
}

/// The `<platform>-<arch>[-musl]` target a pnpm native binary is built for,
/// as the `exe.<target>` platform packages are named after it.
pub(in super::super) fn native_target_name(platform: &str, arch: &str, libc: &str) -> String {
    let arch = normalized_arch(platform, arch);
    let libc_suffix = if platform == "linux" && libc == "musl" { "-musl" } else { "" };
    format!("{platform}-{arch}{libc_suffix}")
}

fn normalized_arch<'a>(platform: &str, arch: &'a str) -> &'a str {
    if platform == "win32" && arch == "ia32" { "x86" } else { arch }
}

/// Link the host's native platform binary (`@pnpm/exe.<target>`) into the
/// wrapper package directory, replicating the wrapper's preinstall step
/// (skipped because the engine is installed with scripts disabled).
///
/// Errors loudly when the wrapper or its platform binary is missing, or
/// when the hard link fails: with scripts disabled, this manual linking is
/// the critical path, so a silent no-op would leave a "successful"
/// self-update with a non-functional `pnpm`.
pub(crate) fn link_exe_platform_binary(
    install_dir: &Path,
    wrapper_pkg_name: &str,
) -> miette::Result<()> {
    let wrapper_dir = package_dir(install_dir, wrapper_pkg_name);
    if !wrapper_dir.exists() {
        let wrapper_display = wrapper_dir.display();
        return Err(miette::miette!("the installed pnpm wrapper is missing at {wrapper_display}"));
    }
    let platform = host_platform();
    let executable = if platform == "win32" { "pnpm.exe" } else { "pnpm" };

    let (install_real_dir, wrapper_real_dir) = canonical_wrapper_dirs(install_dir, &wrapper_dir)?;
    let parent = wrapper_real_dir
        .parent()
        .ok_or_else(|| miette::miette!("the pnpm wrapper has no parent directory"))?;
    let scope_dir =
        if wrapper_pkg_name.starts_with('@') { parent.to_path_buf() } else { parent.join("@pnpm") };

    let src = find_native_binary(&scope_dir, platform, executable)?;
    let native_source_root = native_source_trust_root(&install_real_dir, wrapper_pkg_name);
    let src = validate_native_binary_source(&src, &native_source_root)?;
    let dest = wrapper_real_dir.join(executable);
    replace_executable(&src, &dest)
        .into_diagnostic()
        .wrap_err("link the native pnpm binary into the wrapper")?;

    if platform == "win32" {
        link_windows_aliases(&src, &wrapper_real_dir)?;
        rewrite_windows_bin_field(&wrapper_real_dir);
    }
    Ok(())
}

/// Resolve the platform binary by its explicit adjacent path in the
/// real virtual store, not via a `node_modules` walk (which a
/// repo-controlled store-dir could shadow). `@pnpm/exe`'s parent is
/// already `@pnpm`; the unscoped `pnpm` descends into `@pnpm`.
fn canonical_wrapper_dirs(
    install_dir: &Path,
    wrapper_dir: &Path,
) -> miette::Result<(PathBuf, PathBuf)> {
    let install_real_dir = fs::canonicalize(install_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve the pnpm install dir at {}", install_dir.display()))?;
    let wrapper_real_dir = fs::canonicalize(wrapper_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve the pnpm wrapper at {}", wrapper_dir.display()))?;
    if !wrapper_real_dir.starts_with(&install_real_dir) {
        let wrapper_display = wrapper_dir.display();
        let install_display = install_dir.display();
        return Err(miette::miette!(
            "the installed pnpm wrapper at {} resolves outside {}",
            wrapper_display,
            install_display
        ));
    }
    Ok((install_real_dir, wrapper_real_dir))
}

/// The host's `@pnpm/exe` platform binary under `scope_dir`, in either
/// package-directory spelling.
fn find_native_binary(
    scope_dir: &Path,
    platform: &str,
    executable: &str,
) -> miette::Result<PathBuf> {
    let arch = host_arch();
    let libc = host_libc();
    let candidate_dir_names = [
        exe_platform_pkg_dir_name(platform, arch, libc),
        exe_platform_pkg_dir_name_next(platform, arch, libc),
    ];
    candidate_dir_names
        .iter()
        .map(|dir_name| scope_dir.join(dir_name).join(executable))
        .find(|candidate| candidate.exists())
        .ok_or_else(|| {
            miette::miette!("no @pnpm/exe.{platform}-{arch} native binary was found for this host")
        })
}

/// Aliases (pn / pnpx / pnx) must be .exe hardlinks of the native
/// binary, not .cmd wrappers — cmd-shim's Bash shim mangles a .cmd
/// target under MSYS2 / Git Bash. The native binary detects which
/// name it was launched as and prepends `dlx` for pnpx / pnx.
fn link_windows_aliases(src: &Path, wrapper_real_dir: &Path) -> miette::Result<()> {
    for alias in ["pn", "pnpx", "pnx"] {
        replace_executable(src, &wrapper_real_dir.join(format!("{alias}.exe")))
            .into_diagnostic()
            .wrap_err_with(|| format!("link the {alias} alias into the wrapper"))?;
    }
    Ok(())
}

fn native_source_trust_root(install_real_dir: &Path, wrapper_pkg_name: &str) -> PathBuf {
    // In the global virtual store, the wrapper and platform binary live
    // in sibling slots under `links`; self-update installs keep both
    // under the one install dir.
    global_virtual_store_root_from_slot(install_real_dir, wrapper_pkg_name)
        .unwrap_or_else(|| install_real_dir.to_path_buf())
}

// Recognizes a slot by re-deriving its `links`-relative path with
// [`format_global_virtual_store_path`] — the same formatter that laid the
// slot out — so this walk can't drift from the layout (e.g. the `@`
// placeholder scope segment unscoped packages sit under).
fn global_virtual_store_root_from_slot(slot_dir: &Path, package_name: &str) -> Option<PathBuf> {
    let hash = slot_dir.file_name()?.to_str()?;
    let version = slot_dir.parent()?.file_name()?.to_str()?;
    node_semver::Version::parse(version).ok()?;

    let mut cursor = slot_dir;
    for segment in format_global_virtual_store_path(package_name, version, hash).split('/').rev() {
        if cursor.file_name()?.to_str()? != segment {
            return None;
        }
        cursor = cursor.parent()?;
    }
    (cursor.file_name()?.to_str()? == "links").then(|| cursor.to_path_buf())
}

fn validate_native_binary_source(src: &Path, source_root: &Path) -> miette::Result<PathBuf> {
    let src_display = src.display().to_string();
    let link_meta = fs::symlink_metadata(src)
        .into_diagnostic()
        .wrap_err_with(|| format!("inspect the native pnpm binary at {src_display}"))?;
    if link_meta.file_type().is_symlink() {
        return Err(miette::miette!("the native pnpm binary at {src_display} is a symlink"));
    }
    let src_real = fs::canonicalize(src)
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve the native pnpm binary at {src_display}"))?;
    if !src_real.starts_with(source_root) {
        let source_root_display = source_root.display().to_string();
        return Err(miette::miette!(
            "the native pnpm binary at {src_display} resolves outside {source_root_display}"
        ));
    }
    let src_real_display = src_real.display().to_string();
    let meta = fs::metadata(&src_real)
        .into_diagnostic()
        .wrap_err_with(|| format!("inspect the native pnpm binary at {src_real_display}"))?;
    if !meta.is_file() {
        return Err(miette::miette!(
            "the native pnpm binary at {src_real_display} is not a regular file"
        ));
    }
    Ok(src_real)
}

/// Point the Windows wrapper's `bin` field at the `.exe` variants (the
/// npm shim generator reads `bin` at install time). Written via a temp
/// file + rename so the content-addressed, hard-linked `package.json`
/// blob is not mutated in place.
fn rewrite_windows_bin_field(wrapper_dir: &Path) {
    let pkg_json_path = wrapper_dir.join("package.json");
    let Ok(text) = fs::read_to_string(&pkg_json_path) else {
        return;
    };
    let Ok(mut pkg) = parse_manifest(&text) else {
        return;
    };
    let Some(bin) = pkg.get_mut("bin").and_then(Value::as_object_mut) else {
        return;
    };
    for (name, target) in
        [("pnpm", "pnpm.exe"), ("pn", "pn.exe"), ("pnpx", "pnpx.exe"), ("pnx", "pnx.exe")]
    {
        bin.insert(name.to_string(), Value::String(target.to_string()));
    }
    let Ok(serialized) = serde_json::to_string_pretty(&pkg) else {
        return;
    };
    let temp_path = pkg_json_path.with_extension("json.pnpm-tmp");
    if fs::write(&temp_path, serialized).is_err() {
        let _ = fs::remove_file(&temp_path);
        return;
    }
    if fs::rename(&temp_path, &pkg_json_path).is_err() {
        let _ = fs::remove_file(&temp_path);
    }
}
