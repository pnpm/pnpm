//! Install pnpm into the global packages directory for a self-update.
//!
//! The engine is installed into a fresh directory under the global
//! packages dir (visible to `pnpm ls -g`), native target installs have the
//! host's platform binary linked into the wrapper (replicating the wrapper's
//! preinstall, which is skipped because the engine is installed with scripts
//! disabled), and the caller links the bins + hash symlink.

pub(crate) use native_binary::link_exe_platform_binary;
pub(super) use native_binary::{
    exe_platform_pkg_dir_name, exe_platform_pkg_dir_name_next, native_target_name,
};

use super::SelfUpdateError;
use crate::{State, cli_args::add::add_package, executable_link::replace_executable};
use miette::{Context, IntoDiagnostic};

use pnpm_config::{Config, NodeLinker, PackageManagerBootstrap};
use pnpm_global::{clean_orphaned_install_dirs, create_install_dir, find_global_package};
use pnpm_graph_hasher::{format_global_virtual_store_path, host_arch, host_libc, host_platform};
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_package_manifest::{DependencyGroup, parse_manifest};
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// From v12 the unscoped `pnpm` package is itself the native engine
/// (equal content to `@pnpm/exe`), so v12+ installs converge on `pnpm`.
pub(crate) const PNPM_PACKAGE_NAME: &str = "pnpm";
pub(crate) const PNPM_EXE_PACKAGE_NAME: &str = "@pnpm/exe";

const PNPM_EXE_INTRODUCED: (u64, u64, u64) = (6, 17, 1);

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
        None,
    ))
    .await
    .and_then(|()| finalize_engine_install(&install_dir, package, version));
    if let Err(err) = outcome {
        let _ = fs::remove_dir_all(&install_dir);
        return Err(err);
    }
    Ok(InstallPnpmResult { install_dir, package_name, already_existed: false })
}

/// Fail unless the engine installed at `install_dir` can execute — a release can
/// install cleanly and still not run, when its wrapper kept the placeholder bin
/// of a platform package that shipped without a native.
///
/// Only that it runs is asserted; reading `--version` output would tie the check
/// to whatever startup decides to print.
pub(super) fn assert_pnpm_runs(
    install_dir: &Path,
    package_name: &str,
    version: &str,
) -> miette::Result<()> {
    let executable = pnpm_executable_path(install_dir, package_name);
    // pnpm prints its version only after loading config and switching versions,
    // so probing from the caller's directory answers with their pin rather than
    // the release under test.
    let probe_dir = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("create a directory to check the installed pnpm from")?;
    let reason =
        match Command::new(&executable).arg("--version").current_dir(probe_dir.path()).output() {
            Err(err) => err.to_string(),
            Ok(output) if !output.status.success() => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stderr = stderr.trim();
                let code = output
                    .status
                    .code()
                    .map_or_else(|| "a signal".to_string(), |code| format!("code {code}"));
                if stderr.is_empty() {
                    format!("it exited with {code}")
                } else {
                    format!("it exited with {code}: {stderr}")
                }
            }
            Ok(_) => return Ok(()),
        };
    Err(SelfUpdateError::BrokenPnpmInstall {
        version: version.to_string(),
        reason,
        executable: executable.display().to_string(),
    }
    .into())
}

/// The native pnpm executable linked into an installed engine wrapper.
pub(super) fn pnpm_executable_path(install_dir: &Path, package_name: &str) -> PathBuf {
    package_dir(install_dir, package_name).join(if host_platform() == "win32" {
        "pnpm.exe"
    } else {
        "pnpm"
    })
}

/// The installed wrapper's recorded version, or `None` when the install is
/// absent or unreadable.
pub(super) fn installed_version(install_dir: &Path, package_name: &str) -> Option<String> {
    let pkg_json = package_dir(install_dir, package_name).join("package.json");
    let text = fs::read_to_string(pkg_json).ok()?;
    let value: Value = parse_manifest(&text).ok()?;
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

/// Whether `version` can be installed at all. False for versions whose
/// `@pnpm/exe` shipped platform packages with no binary, so it cannot run.
/// Matched by version, not package: the pin is shared but the wrapper is not,
/// so a developer on the JS `pnpm` — which does run at these versions — would
/// otherwise pin one and break every teammate on `@pnpm/exe`.
///
/// For callers that pick a version rather than being handed one, and so can
/// choose another instead of failing; [`assert_release_is_installable`] is the
/// failing form.
pub(crate) fn is_release_installable(version: &str) -> bool {
    !matches!(version, "11.12.0" | "11.13.0")
}

/// Fail for a version [`is_release_installable`] rejects.
pub(crate) fn assert_release_is_installable(version: &str) -> miette::Result<()> {
    if is_release_installable(version) {
        return Ok(());
    }
    Err(SelfUpdateError::BrokenPnpmRelease { version: version.to_string() }.into())
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

/// Install a package-manager engine wrapper into a fresh group directory,
/// mirroring the global-add group install but with scripts disabled (the
/// native binary is linked manually afterwards) and no build-approval
/// prompt.
///
/// `shared_engine_packages` selects the layout. `Some(packages)` installs
/// into the shared global virtual store (`<store>/links/...`) — the layout
/// engine provisioning reuses across invocations — with those packages
/// marked buildable so the `ENGINE_NAME` is folded into their GVS hash and
/// each platform resolves to its own slot instead of colliding. `None`
/// keeps the engine self-contained inside `install_dir` (the `self-update`
/// global install). In both cases scripts are disabled and the native
/// binary is linked manually by the caller via
/// [`link_exe_platform_binary`].
pub(crate) async fn run_install<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    install_dir: &Path,
    package_name: &str,
    version: &str,
    supported_architectures: Option<SupportedArchitectures>,
    shared_engine_packages: Option<&[&str]>,
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
    cfg.enable_global_virtual_store = shared_engine_packages.is_some();
    cfg.lockfile = true;
    // Anchored (never `None`, which walks up and can adopt the global
    // packages dir's own settings `pnpm-workspace.yaml` as the workspace
    // root, pnpm/pnpm#13697) — the same guard as `run_group_install` in
    // `cli_args::global`, where the full rationale lives.
    cfg.workspace_dir = Some(install_dir.to_path_buf());
    cfg.supported_architectures = supported_architectures;
    // The engine is installed with scripts disabled — the wrapper's
    // preinstall (which links the platform binary) is replicated by
    // `link_exe_platform_binary`, so running it here is both unnecessary
    // and a code-execution surface during a privileged install.
    cfg.ignore_scripts = true;
    cfg.dangerously_allow_all_builds = false;
    cfg.strict_dep_builds = false;
    cfg.allow_builds.clear();
    if let Some(packages) = shared_engine_packages {
        cfg.global_virtual_store_dir = base_config.store_dir.links();
        for name in packages {
            cfg.allow_builds.insert((*name).to_string(), true);
        }
    }
    // Drop repo-controlled resolution-rewrite settings so a project's
    // `pnpm-workspace.yaml` can't change the engine's installed dependency
    // graph. The top-level engine components are signature-verified, but
    // the installed closure must stay the published one.
    cfg.overrides = None;
    cfg.package_extensions = None;
    cfg.catalogs = None;
    cfg.patched_dependencies = None;
    // The engine closure is pnpm's own, so the project's linker choice must
    // not shape its layout: under `hoisted` the engine materializes inside
    // `install_dir` instead of the global virtual store the caller resolves
    // its slot from (pnpm/pnpm#14595).
    cfg.node_linker = NodeLinker::Isolated;

    let config: &'static Config = Config::leak(cfg);
    let manifest_path = install_dir.join("package.json");
    let state = State::init(manifest_path, config, false)
        .wrap_err("initialize the self-update install state")?;
    add_package::<Reporter, _>(
        state,
        &format!("{package_name}@{version}"),
        RangeSpecStyle::Patch,
        None,
        false,
        config.supported_architectures.clone(),
        [DependencyGroup::Prod],
    )
    .await
}

#[cfg(test)]
mod tests;

/// Apply the trusted package-manager bootstrap registry/network/auth onto
/// `cfg`, so the engine install can't be redirected by repo-controlled
/// project settings. Mirrors the routing in
/// [`crate::config_deps`]'s `for_package_manager` context.
fn apply_package_manager_bootstrap(cfg: &mut Config, bootstrap: &PackageManagerBootstrap) {
    cfg.registry.clone_from(&bootstrap.registry);
    cfg.registries_by_scope.clone_from(&bootstrap.registries);
    cfg.proxy.clone_from(&bootstrap.proxy);
    cfg.tls.clone_from(&bootstrap.tls);
    cfg.tls_by_uri.clone_from(&bootstrap.tls_by_uri);
    cfg.auth_headers = std::sync::Arc::clone(&bootstrap.auth_headers);
}

pub(crate) fn package_dir(install_dir: &Path, package_name: &str) -> PathBuf {
    let mut package_dir = install_dir.join("node_modules");
    for segment in package_name.split('/') {
        package_dir.push(segment);
    }
    package_dir
}

/// Link and execute native engines before making them the global command.
fn finalize_engine_install(
    install_dir: &Path,
    package: PnpmPackageToInstall,
    version: &str,
) -> miette::Result<()> {
    if package.links_native_binary {
        link_exe_platform_binary(install_dir, package.name)?;
        // Before the caller links this dir into the global bin, so a broken
        // release is discarded rather than swapped in.
        assert_pnpm_runs(install_dir, package.name, version)
    } else {
        // The legacy JS engine has no binary of its own to be missing.
        Ok(())
    }
}

mod native_binary;
