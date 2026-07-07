//! Install pnpm into the global packages directory for a self-update.
//!
//! The engine is installed into a fresh directory under the global
//! packages dir (visible to `pnpm ls -g`), native target installs have the
//! host's platform binary linked into the wrapper (replicating the wrapper's
//! preinstall, which is skipped because the engine is installed with scripts
//! disabled), and the caller links the bins + hash symlink.

use crate::{State, cli_args::add::add_package};
use miette::{Context, IntoDiagnostic};
use pacquet_config::{Config, PackageManagerBootstrap};
use pacquet_global::{clean_orphaned_install_dirs, create_install_dir, find_global_package};
use pacquet_graph_hasher::{host_arch, host_libc, host_platform};
use pacquet_package_is_installable::SupportedArchitectures;
use pacquet_package_manifest::DependencyGroup;
use pacquet_registry::PinnedVersion;
use pacquet_reporter::Reporter;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

use super::SelfUpdateError;

/// From v12 the unscoped `pnpm` package is itself the native engine
/// (equal content to `@pnpm/exe`), so v12+ installs converge on `pnpm`.
pub(crate) const PNPM_PACKAGE_NAME: &str = "pnpm";
pub(crate) const PNPM_EXE_PACKAGE_NAME: &str = "@pnpm/exe";

const PNPM_EXE_INTRODUCED: (u64, u64, u64) = (6, 17, 1);

/// The package-manager components marked buildable when installing the
/// engine (`{ '@pnpm/exe': true, 'pnpm': true }`), so the `ENGINE_NAME` is
/// folded into their global-virtual-store hash and each platform resolves
/// to its own slot instead of colliding.
pub(crate) const PNPM_ALLOW_BUILDS: [&str; 2] = ["pnpm", "@pnpm/exe"];

#[derive(Clone, Copy)]
pub(crate) struct PnpmPackageToInstall {
    pub name: &'static str,
    pub links_native_binary: bool,
}

pub(super) struct InstallPnpmResult {
    pub install_dir: PathBuf,
    pub package_name: &'static str,
    pub already_existed: bool,
}

/// Install the target pnpm engine into the global packages directory. Returns
/// the install directory and whether the requested version was already
/// present (in which case nothing is downloaded and the caller just
/// relinks it).
pub(super) async fn install_pnpm<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    version: &str,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<InstallPnpmResult> {
    let package = pnpm_package_to_install(version);
    let package_name = package.name;
    let global_pkg_dir = base_config.global_pkg_dir.clone().ok_or(SelfUpdateError::NoGlobalDir)?;
    fs::create_dir_all(&global_pkg_dir)
        .into_diagnostic()
        .wrap_err("create the global packages directory")?;
    clean_orphaned_install_dirs(&global_pkg_dir);

    if let Some(existing) = find_global_package(&global_pkg_dir, package_name)
        .into_diagnostic()
        .wrap_err("scan global packages")?
        && reuse_cached_engine(&existing.install_dir, package, version)
    {
        return Ok(InstallPnpmResult {
            install_dir: existing.install_dir,
            package_name,
            already_existed: true,
        });
    }

    let install_dir = create_install_dir(&global_pkg_dir)
        .into_diagnostic()
        .wrap_err("create the global install dir")?;
    let outcome = Box::pin(run_install::<Reporter>(
        base_config,
        &install_dir,
        package_name,
        version,
        supported_architectures,
        false,
    ))
    .await
    .and_then(|()| {
        if package.links_native_binary {
            link_exe_platform_binary(&install_dir, package_name)
        } else {
            Ok(())
        }
    });
    if let Err(err) = outcome {
        let _ = fs::remove_dir_all(&install_dir);
        return Err(err);
    }
    Ok(InstallPnpmResult { install_dir, package_name, already_existed: false })
}

/// The installed wrapper's recorded version, or `None` when the install is
/// absent or unreadable.
fn installed_version(install_dir: &Path, package_name: &str) -> Option<String> {
    let pkg_json = package_dir(install_dir, package_name).join("package.json");
    let text = fs::read_to_string(pkg_json).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.get("version").and_then(Value::as_str).map(ToString::to_string)
}

/// Whether an existing global slot at `install_dir` can be reused for
/// `version` instead of reinstalling: it records the target version and,
/// for native engines, its wrapper's platform binary relinks cleanly.
///
/// The relink is best-effort *on the reuse decision only* — it does not
/// weaken any check. [`link_exe_platform_binary`]'s wrapper-containment
/// guard still runs and still refuses to link when the wrapper resolves
/// outside `install_dir`; on refusal it links nothing. This helper simply
/// reports that refusal as "not reusable" (returning `false`) instead of
/// propagating it as a hard error, so the caller falls through to a fresh
/// install rather than aborting the whole self-update. Nothing from the
/// rejected slot is linked or executed: the fresh install downloads and
/// signature-verifies the engine into a new self-contained slot (see
/// `verify_pnpm_engine_identity` in the `self-update` handler), and the
/// rejected slot is never returned.
///
/// A slot whose wrapper escapes the slot is what an older global-virtual-
/// store layout legitimately produces (the wrapper symlinks into the
/// shared `<store>/links` tree), so this recovery is the common upgrade
/// path, not only an adversarial one.
fn reuse_cached_engine(install_dir: &Path, package: PnpmPackageToInstall, version: &str) -> bool {
    if installed_version(install_dir, package.name).as_deref() != Some(version) {
        return false;
    }
    !package.links_native_binary || link_exe_platform_binary(install_dir, package.name).is_ok()
}

pub(crate) fn pnpm_package_to_install(pnpm_version: &str) -> PnpmPackageToInstall {
    let Some(version) = node_semver::Version::parse(pnpm_version).ok() else {
        return PnpmPackageToInstall { name: PNPM_EXE_PACKAGE_NAME, links_native_binary: true };
    };
    if version.major >= 12 {
        return PnpmPackageToInstall { name: PNPM_PACKAGE_NAME, links_native_binary: true };
    }
    if version_gte(&version, PNPM_EXE_INTRODUCED) {
        PnpmPackageToInstall { name: PNPM_EXE_PACKAGE_NAME, links_native_binary: true }
    } else {
        PnpmPackageToInstall { name: PNPM_PACKAGE_NAME, links_native_binary: false }
    }
}

fn version_gte(version: &node_semver::Version, minimum: (u64, u64, u64)) -> bool {
    (version.major, version.minor, version.patch) >= minimum
}

/// Install a pnpm engine wrapper into a fresh group directory, mirroring the
/// global-add group install but with scripts disabled (the native binary
/// is linked manually afterwards) and no build-approval prompt.
///
/// When `enable_global_virtual_store` is `true` the engine is installed
/// into the shared global virtual store (`<store>/links/...`) — the layout
/// `pnpm with` reuses across invocations — with the package-manager
/// components ([`PNPM_ALLOW_BUILDS`]) marked buildable so the
/// `ENGINE_NAME` is folded into their GVS hash. When `false` the engine is
/// self-contained inside `install_dir` (the `self-update` global install).
/// In both cases scripts are disabled and the native binary is linked
/// manually by the caller via [`link_exe_platform_binary`].
pub(crate) async fn run_install<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    install_dir: &Path,
    package_name: &str,
    version: &str,
    supported_architectures: Option<SupportedArchitectures>,
    enable_global_virtual_store: bool,
) -> miette::Result<()> {
    let mut cfg = base_config.clone();
    // Resolve and fetch the engine bytes through the trusted
    // package-manager bootstrap registry/network/auth, never the
    // repository-controlled project settings — otherwise a malicious
    // project `.npmrc` could redirect the downloaded pnpm bytes to an
    // attacker registry. This mirrors how `config_deps` resolves the
    // package manager and how the engine signature is verified.
    apply_package_manager_bootstrap(&mut cfg, &base_config.package_manager_bootstrap);
    cfg.modules_dir = install_dir.join("node_modules");
    cfg.virtual_store_dir = install_dir.join("node_modules").join(".pnpm");
    cfg.enable_global_virtual_store = enable_global_virtual_store;
    cfg.lockfile = true;
    cfg.workspace_dir = None;
    cfg.supported_architectures = supported_architectures;
    // The engine is installed with scripts disabled — the wrapper's
    // preinstall (which links the platform binary) is replicated by
    // `link_exe_platform_binary`, so running it here is both unnecessary
    // and a code-execution surface during a privileged install.
    cfg.ignore_scripts = true;
    cfg.dangerously_allow_all_builds = false;
    cfg.strict_dep_builds = false;
    if enable_global_virtual_store {
        // The engine lands in the shared GVS, so mark the package-manager
        // components ([`PNPM_ALLOW_BUILDS`]) buildable so the `ENGINE_NAME`
        // enters their GVS hash and each platform gets its own slot.
        // Scripts still don't run (`ignore_scripts` above).
        cfg.global_virtual_store_dir = base_config.store_dir.links();
        cfg.allow_builds.clear();
        for name in PNPM_ALLOW_BUILDS {
            cfg.allow_builds.insert(name.to_string(), true);
        }
    } else {
        cfg.allow_builds.clear();
    }
    // Drop repo-controlled resolution-rewrite settings so a project's
    // `pnpm-workspace.yaml` can't change the engine's installed dependency
    // graph. The top-level engine components are signature-verified, but
    // the installed closure must stay the published one.
    cfg.overrides = None;
    cfg.package_extensions = None;
    cfg.catalogs = None;
    cfg.patched_dependencies = None;

    let config: &'static Config = Config::leak(cfg);
    let manifest_path = install_dir.join("package.json");
    let state = State::init(manifest_path, config, false)
        .wrap_err("initialize the self-update install state")?;
    add_package::<Reporter, _, _>(
        state,
        &format!("{package_name}@{version}"),
        PinnedVersion::Patch,
        None,
        false,
        config.supported_architectures.clone(),
        || std::iter::once(DependencyGroup::Prod),
    )
    .await
}

/// Scope-local directory name of the `@pnpm/exe` platform package under
/// the legacy `<os>-<arch>` scheme (`macos-arm64`, `win-x86`,
/// `linux-x64`, `linuxstatic-x64`).
pub(super) fn exe_platform_pkg_dir_name(platform: &str, arch: &str, libc: &str) -> String {
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
pub(super) fn exe_platform_pkg_dir_name_next(platform: &str, arch: &str, libc: &str) -> String {
    let arch = normalized_arch(platform, arch);
    let libc_suffix = if platform == "linux" && libc == "musl" { "-musl" } else { "" };
    format!("exe.{platform}-{arch}{libc_suffix}")
}

fn normalized_arch<'a>(platform: &str, arch: &'a str) -> &'a str {
    if platform == "win32" && arch == "ia32" { "x86" } else { arch }
}

#[cfg(test)]
mod tests;

/// Apply the trusted package-manager bootstrap registry/network/auth onto
/// `cfg`, so the engine install can't be redirected by repo-controlled
/// project settings. Mirrors the routing in
/// [`crate::config_deps`]'s `for_package_manager` context.
fn apply_package_manager_bootstrap(cfg: &mut Config, bootstrap: &PackageManagerBootstrap) {
    cfg.registry.clone_from(&bootstrap.registry);
    cfg.registries.clone_from(&bootstrap.registries);
    cfg.proxy.clone_from(&bootstrap.proxy);
    cfg.tls.clone_from(&bootstrap.tls);
    cfg.tls_by_uri.clone_from(&bootstrap.tls_by_uri);
    cfg.auth_headers = std::sync::Arc::clone(&bootstrap.auth_headers);
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
    let arch = host_arch();
    let libc = host_libc();
    let executable = if platform == "win32" { "pnpm.exe" } else { "pnpm" };

    // Resolve the platform binary by its explicit adjacent path in the
    // real virtual store, not via a `node_modules` walk (which a
    // repo-controlled store-dir could shadow). `@pnpm/exe`'s parent is
    // already `@pnpm`; the unscoped `pnpm` descends into `@pnpm`.
    let install_real_dir = fs::canonicalize(install_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve the pnpm install dir at {}", install_dir.display()))?;
    let wrapper_real_dir = fs::canonicalize(&wrapper_dir)
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
    let parent = wrapper_real_dir
        .parent()
        .ok_or_else(|| miette::miette!("the pnpm wrapper has no parent directory"))?;
    let scope_dir =
        if wrapper_pkg_name.starts_with('@') { parent.to_path_buf() } else { parent.join("@pnpm") };

    let candidate_dir_names = [
        exe_platform_pkg_dir_name(platform, arch, libc),
        exe_platform_pkg_dir_name_next(platform, arch, libc),
    ];
    let src = candidate_dir_names
        .iter()
        .map(|dir_name| scope_dir.join(dir_name).join(executable))
        .find(|candidate| candidate.exists())
        .ok_or_else(|| {
            miette::miette!("no @pnpm/exe.{platform}-{arch} native binary was found for this host")
        })?;
    let native_source_root = native_source_trust_root(&install_real_dir, wrapper_pkg_name);
    let src = validate_native_binary_source(&src, &native_source_root)?;
    let dest = wrapper_real_dir.join(executable);
    force_link(&src, &dest)
        .into_diagnostic()
        .wrap_err("link the native pnpm binary into the wrapper")?;

    if platform == "win32" {
        // Aliases (pn / pnpx / pnx) must be .exe hardlinks of the native
        // binary, not .cmd wrappers — cmd-shim's Bash shim mangles a .cmd
        // target under MSYS2 / Git Bash. The native binary detects which
        // name it was launched as and prepends `dlx` for pnpx / pnx.
        for alias in ["pn", "pnpx", "pnx"] {
            force_link(&src, &wrapper_real_dir.join(format!("{alias}.exe")))
                .into_diagnostic()
                .wrap_err_with(|| format!("link the {alias} alias into the wrapper"))?;
        }
        rewrite_windows_bin_field(&wrapper_real_dir);
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

fn global_virtual_store_root_from_slot(slot_dir: &Path, package_name: &str) -> Option<PathBuf> {
    let mut cursor = slot_dir.parent()?;
    let version = cursor.file_name()?.to_str()?;
    node_semver::Version::parse(version).ok()?;

    cursor = cursor.parent()?;
    let mut package_segments = package_name.split('/').rev();
    let name = package_segments.next()?;
    if cursor.file_name()?.to_str()? != name {
        return None;
    }
    for segment in package_segments {
        cursor = cursor.parent()?;
        if cursor.file_name()?.to_str()? != segment {
            return None;
        }
    }

    let root = cursor.parent()?;
    (root.file_name()?.to_str()? == "links").then(|| root.to_path_buf())
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

pub(crate) fn package_dir(install_dir: &Path, package_name: &str) -> PathBuf {
    let mut package_dir = install_dir.join("node_modules");
    for segment in package_name.split('/') {
        package_dir.push(segment);
    }
    package_dir
}

/// Hard-link `src` to `dest`, replacing any existing file. Marks the
/// result executable on Unix (a copy/link can lose the bit).
fn force_link(src: &Path, dest: &Path) -> std::io::Result<()> {
    match fs::remove_file(dest) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    fs::hard_link(src, dest)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dest, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
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
    let Ok(mut pkg) = serde_json::from_str::<Value>(&text) else {
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
