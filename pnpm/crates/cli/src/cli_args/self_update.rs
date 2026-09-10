//! `pacquet self-update` — update pnpm to the latest version (or a given one).
//!
//! The target version is resolved from the trusted package-manager
//! bootstrap registry. When the project pins pnpm via
//! `packageManager` / `devEngines.packageManager`, that pin is updated in
//! place; otherwise the engine is installed into the global packages
//! directory, its native binary linked, its registry signature verified,
//! and its bins linked into the global bin directory.

// `pub(crate)` so `pnpm with` can reuse the engine installer and the
// engine-identity verifier; both commands install the same pnpm engine.
pub(crate) mod install_pnpm;
pub(crate) mod verify_engine;

use crate::config_deps::{self, EnginePolicyViolation};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_cmd_shim::{Host as CmdShimHost, LinkBinsOptions, link_bins_of_packages_with_excludes};
use pnpm_config::{Config, PNPM_VERSION, standalone_install_command};
use pnpm_fs::force_symlink_dir;
use pnpm_global::{
    create_global_cache_key, find_global_package, get_hash_link, read_installed_packages,
};
use pnpm_lockfile::EnvLockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_npm_resolver::{MINIMUM_RELEASE_AGE_VIOLATION_CODE, infer_range_spec_style};
use project_pin::{read_project_pinned_pnpm_version, update_project_pin};
use serde_json::Value;
use std::{collections::HashSet, io::IsTerminal, path::Path};

/// Migration guidance printed once when `self-update` crosses a major
/// boundary. Add an entry per future major that ships breaking changes
/// users need to act on.
fn major_upgrade_hint(target_major: u64) -> Option<&'static str> {
    match target_major {
        11 => Some(
            "pnpm v11 removed or renamed several v10 settings. \
             See https://pnpm.io/11.x/migration for migration instructions.",
        ),
        _ => None,
    }
}

/// Errors specific to `self-update`. The codes carry the shared
/// `ERR_PNPM_` prefix, so a code already starting with `PNPM_` becomes
/// `ERR_PNPM_PNPM_...`.
#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum SelfUpdateError {
    #[display("pnpm cannot update itself when it is executed by Corepack")]
    #[diagnostic(
        code(ERR_PNPM_CANT_SELF_UPDATE_IN_COREPACK),
        help("Install pnpm with the standalone script instead: {install_command}")
    )]
    CantSelfUpdateInCorepack { install_command: &'static str },

    #[display(r#"Cannot find "{specifier}" version of pnpm"#)]
    #[diagnostic(code(ERR_PNPM_CANNOT_RESOLVE_PNPM))]
    CannotResolvePnpm { specifier: String },

    #[display(
        "Refusing to switch to pnpm v{version}: it violates the configured minimumReleaseAge / trustPolicy"
    )]
    #[diagnostic(code(ERR_PNPM_PNPM_RELEASE_POLICY_VIOLATION))]
    ReleasePolicyViolation { version: String },

    #[display("pnpm@{version} {reason}.")]
    #[diagnostic(
        code(ERR_PNPM_NO_MATURE_MATCHING_VERSION),
        help(
            "Wait for the release to mature past the cutoff, or set PNPM_CONFIG_MINIMUM_RELEASE_AGE=0 to update anyway."
        )
    )]
    NoMatureMatchingVersion { version: String, reason: String },

    #[display("Aborted: the immature pnpm version was not approved")]
    #[diagnostic(code(ERR_PNPM_MINIMUM_RELEASE_AGE_DENIED))]
    MinimumReleaseAgeDenied,

    #[diagnostic(code(ERR_PNPM_PNPM_ENGINE_IDENTITY_UNVERIFIABLE))]
    EngineIdentityUnverifiable { message: String },

    #[diagnostic(code(ERR_PNPM_PNPM_ENGINE_IDENTITY_MISMATCH))]
    EngineIdentityMismatch { message: String },

    #[display("Cannot run {label} on this host: it ships no native binary for {target}.")]
    #[diagnostic(
        code(ERR_PNPM_PNPM_ENGINE_NO_NATIVE_BINARY),
        help("Set `pmOnFail` to `ignore` to skip the version switch.")
    )]
    EngineNoNativeBinary { label: String, target: String },

    #[display("Unable to find the global bin directory")]
    #[diagnostic(
        code(ERR_PNPM_NO_GLOBAL_BIN_DIR),
        help(
            r#"Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH."#
        )
    )]
    NoGlobalDir,

    #[display("The pnpm v{version} that was just installed cannot run: {reason}")]
    #[diagnostic(
        code(ERR_PNPM_BROKEN_PNPM_INSTALL),
        help(
            r#"The installation at "{executable}" was discarded and the currently active pnpm was left in place, so pnpm still works. A release that installs but cannot run is a packaging fault — please report it at https://github.com/pnpm/pnpm/issues. To move to a different version meanwhile, pass one to "pnpm self-update"."#
        )
    )]
    BrokenPnpmInstall { version: String, reason: String, executable: String },

    #[display("pnpm v{version} is a broken release and cannot be installed")]
    #[diagnostic(
        code(ERR_PNPM_BROKEN_PNPM_RELEASE),
        help(
            r#"Its "@pnpm/exe" build shipped without a binary and does not run. Even where it does run, pinning it would break everyone on the project who uses "@pnpm/exe", because the pin is shared. Choose another version, or run "pnpm self-update latest"."#
        )
    )]
    BrokenPnpmRelease { version: String },
}

#[derive(Debug, Args)]
pub struct SelfUpdateArgs {
    /// The version, range, or dist-tag to update to. Defaults to the
    /// `latest` dist-tag (which refuses to downgrade).
    pub version: Option<String>,
}

/// Act on a policy violation the resolver attached to self-update's pick.
///
/// A `minimumReleaseAge` cutoff exists so a freshly published pnpm cannot
/// reach the machine before anyone has had a chance to notice it is
/// malicious, and pnpm itself is the most valuable thing on the machine to
/// compromise — so under strict mode an immature pick is refused. An
/// interactive run may still confirm it: naming a version on the command line
/// is a deliberate act by the person at the keyboard, unlike a dependency
/// drifting onto a new release. CI and other non-interactive runs always fail
/// closed — a CI runner that allocates a pseudo-TTY has no one at the keyboard
/// either. The cutoff never comes from the project — see
/// [`WorkspaceSettings::clear_self_update_policy`] — so the only policy to
/// confirm here is the user's own.
///
/// A `trustPolicy` violation is not negotiable — it means the release's trust
/// evidence weakened relative to the installed version.
///
/// [`WorkspaceSettings::clear_self_update_policy`]: pnpm_config::WorkspaceSettings::clear_self_update_policy
fn enforce_resolution_policy(
    config: &Config,
    version: &str,
    violation: &EnginePolicyViolation,
) -> miette::Result<()> {
    if violation.code != MINIMUM_RELEASE_AGE_VIOLATION_CODE {
        return Err(SelfUpdateError::ReleasePolicyViolation { version: version.to_string() }.into());
    }
    if config.resolved_minimum_release_age().is_none()
        || !config.resolved_minimum_release_age_strict()
    {
        return Ok(());
    }
    if is_ci::cached() || !std::io::stdin().is_terminal() {
        return Err(SelfUpdateError::NoMatureMatchingVersion {
            version: version.to_string(),
            reason: violation.reason.clone(),
        }
        .into());
    }
    let prompt = format!("pnpm@{version} {reason}.\nUpdate anyway?", reason = violation.reason);
    // An interrupted prompt (Esc / Ctrl-C) counts as a refusal.
    match dialoguer::Confirm::new().with_prompt(prompt).default(false).interact() {
        Ok(true) => Ok(()),
        Ok(false) | Err(_) => Err(SelfUpdateError::MinimumReleaseAgeDenied.into()),
    }
}

/// Refuse to self-update under corepack (which manages its own updates).
/// Checked in the dispatcher *before* project config is loaded, so a broken
/// `.npmrc` / workspace config can't mask the corepack refusal.
pub(crate) fn reject_if_corepack() -> miette::Result<()> {
    if is_executed_by_corepack() {
        return Err(SelfUpdateError::CantSelfUpdateInCorepack {
            install_command: standalone_install_command(),
        }
        .into());
    }
    Ok(())
}

impl SelfUpdateArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        config: &'static Config,
        dir: &Path,
    ) -> miette::Result<()> {
        if let Some(message) =
            Box::pin(handler::<Reporter>(self.version.as_deref(), config, dir)).await?
        {
            println!("{message}");
        }
        Ok(())
    }
}

/// The `self-update` flow. Returns the final user-facing message (printed
/// to stdout), or `None` when nothing needs printing.
async fn handler<Reporter: self::Reporter + 'static>(
    params: Option<&str>,
    config: &'static Config,
    dir: &Path,
) -> miette::Result<Option<String>> {
    let prefix = dir.to_string_lossy().into_owned();
    info::<Reporter>(&prefix, "Checking for updates...");

    // `self-update` (no args) defaults to the `latest` dist-tag but
    // refuses to downgrade; `self-update latest` (explicit) bypasses the
    // guard so a downgrade can still be forced.
    let is_implicit_latest = params.is_none();
    let bare_specifier = params.unwrap_or("latest");

    let resolved = Box::pin(config_deps::resolve_engine_version(config, "pnpm", bare_specifier))
        .await?
        .ok_or_else(|| SelfUpdateError::CannotResolvePnpm {
            specifier: bare_specifier.to_string(),
        })?;
    let target_version = resolved.version;
    // Before the pin below is written, not just before the install: the pin is
    // shared, so a release this wrapper survives can still break a teammate's.
    install_pnpm::assert_release_is_installable(&target_version)?;

    if let Some(violation) = resolved.policy_violation {
        enforce_resolution_policy(config, &target_version, &violation)?;
    }

    let manifest_value = super::package_manager::read_manifest_json(&dir.join("package.json"))?;
    let wanted = manifest_value.as_ref().and_then(super::package_manager::wanted_package_manager);

    if let Some(hint) = crossed_major_hint(config, dir, wanted.as_ref(), &target_version) {
        warn::<Reporter>(&prefix, hint);
    }

    // Project-pin branch: the project pins pnpm, so update the pin in
    // place instead of touching the global install.
    if let Some(pm) = &wanted
        && pm.name == "pnpm"
    {
        return Box::pin(update_project_pin(config, dir, pm, &target_version, is_implicit_latest))
            .await;
    }

    if let Some(message) =
        global_switch_declined(config, &target_version, bare_specifier, is_implicit_latest)?
    {
        return Ok(Some(message));
    }

    switch_global_pnpm::<Reporter>(config, &target_version, &prefix, bare_specifier).await
}

/// Resolve the target engine's integrities into the env lockfile and verify
/// its identity before anything is installed.
async fn verify_target_engine<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    target_version: &str,
    prefix: &str,
) -> miette::Result<()> {
    let env_root = config.global_pkg_dir.clone().ok_or(SelfUpdateError::NoGlobalDir)?;
    // Resolve integrities into the env lockfile so the engine identity can
    // be verified before install.
    Box::pin(config_deps::sync_package_manager_dependencies(
        config,
        &env_root,
        target_version,
        target_version,
        false,
        false,
    ))
    .await?;
    let env = EnvLockfile::read(&env_root)
        .map_err(miette::Report::new)
        .wrap_err("read the env lockfile")?
        .ok_or_else(|| SelfUpdateError::EngineIdentityUnverifiable {
            message: format!(
                "Cannot verify the identity of pnpm@{target_version}: its integrity metadata is missing from pnpm-lock.yaml.",
            ),
        })?;
    let label = format!("pnpm@{target_version}");
    let package = install_pnpm::pnpm_package_to_install(target_version);
    let engine = verify_engine::EngineToVerify {
        label: &label,
        package: package.name,
        version: target_version,
        platform_binaries: if package.links_native_binary {
            verify_engine::PlatformBinaries::PnpmExe
        } else {
            verify_engine::PlatformBinaries::None
        },
    };
    if let Some(warning) =
        Box::pin(verify_engine::verify_engine_identity(&env, &engine, config)).await?
    {
        warn::<Reporter>(prefix, &warning);
    }
    Ok(())
}

/// The migration hint for the major boundary this update crosses, if it
/// crosses one. The project's pin is the source of truth for the version
/// being left behind; the running binary stands in when nothing pins pnpm.
fn crossed_major_hint(
    config: &Config,
    dir: &Path,
    wanted: Option<&super::package_manager::WantedPackageManager>,
    target_version: &str,
) -> Option<&'static str> {
    let previous_version = match wanted {
        // A range pin (`^10.0.0`) has no recoverable major on its own, so
        // read the resolved version from the env lockfile to drive the
        // hint — otherwise crossing a major would silently skip it.
        Some(pm) if pm.name == "pnpm" => {
            let lockfile_dir = config.workspace_dir.as_deref().unwrap_or(dir);
            read_project_pinned_pnpm_version(lockfile_dir, pm.version.as_deref())
                .filter(|version| version != target_version)
        }
        _ if PNPM_VERSION != target_version => Some(PNPM_VERSION.to_string()),
        _ => None,
    }?;
    let previous_major = coerce_major(&previous_version)?;
    let target = node_semver::Version::parse(target_version).ok()?;
    (target.major > previous_major).then(|| major_upgrade_hint(target.major))?
}

/// The message explaining why the global install is left alone, when this
/// update would not move it forward.
fn global_switch_declined(
    config: &Config,
    target_version: &str,
    bare_specifier: &str,
    is_implicit_latest: bool,
) -> miette::Result<Option<String>> {
    // Version equality with the running binary alone must not skip the
    // update: a removed global install can be recovered by running a local
    // pnpm of the same version (see pnpm/pnpm#12877).
    if target_version == PNPM_VERSION
        && is_installed_globally(config.global_pkg_dir.as_deref(), target_version)?
    {
        return Ok(Some(format!(
            r#"The currently active pnpm v{PNPM_VERSION} is already "{bare_specifier}" and doesn't need an update"#,
        )));
    }
    if is_implicit_latest && version_lt(target_version, PNPM_VERSION) {
        return Ok(Some(format!(
            r#"The currently active pnpm v{PNPM_VERSION} is newer than the "latest" version on the registry (v{target_version}). No update performed. Run "pnpm self-update latest" to downgrade."#,
        )));
    }
    Ok(None)
}

/// Link the installed engine's bins into the global bin directory and
/// record its cache-keyed hash symlink (so `pnpm ls -g` and `store prune`
/// see it).
fn link_into_global_bin(
    config: &Config,
    installed: &install_pnpm::InstallPnpmResult,
    version: &str,
) -> miette::Result<()> {
    let global_bin = config.global_bin.clone().ok_or(SelfUpdateError::NoGlobalDir)?;
    let global_pkg_dir = config.global_pkg_dir.clone().ok_or(SelfUpdateError::NoGlobalDir)?;
    let _global_bin_lock = super::global_bin_lock::acquire_global_bin_lock(&global_bin)?;

    refresh_global_shims(&global_bin, installed, version)?;

    let pkgs = read_installed_packages(&installed.install_dir);
    link_bins_of_packages_with_excludes::<CmdShimHost>(
        &pkgs,
        &global_bin,
        &HashSet::new(),
        &LinkBinsOptions::default(),
    )
    .map_err(miette::Report::new)
    .wrap_err("link the updated pnpm bins")?;

    let aliases = vec![installed.package_name.to_string()];
    let cache_hash = create_global_cache_key(&aliases, &registries_for_cache_key(config));
    let hash_link = get_hash_link(&global_pkg_dir, &cache_hash);
    force_symlink_dir(&installed.install_dir, &hash_link)
        .into_diagnostic()
        .wrap_err("link the global pnpm install directory")?;
    Ok(())
}

fn refresh_global_shims(
    global_bin: &Path,
    installed: &install_pnpm::InstallPnpmResult,
    version: &str,
) -> miette::Result<()> {
    // Named native shims first shipped in pnpm 12.3. An older engine
    // cannot interpret their sidecars, so leave the working shim engine
    // in place when downgrading.
    if !node_semver::Version::parse(version)
        .is_ok_and(|version| (version.major, version.minor) >= (12, 3))
    {
        return Ok(());
    }
    let executable =
        install_pnpm::pnpm_executable_path(&installed.install_dir, installed.package_name);
    crate::shim_dispatch::refresh_native_shims(&executable, global_bin)
        .into_diagnostic()
        .wrap_err("refresh the global shims")
}

/// Build the registry map (`{ default, ...scoped }`) hashed into the
/// global cache key, from the trusted package-manager bootstrap registries
/// — never the repo-controlled project registries, so a project `.npmrc`
/// can't change the hash-symlink name (which would create duplicate global
/// `pnpm` groups that `find_global_package` resolves non-deterministically).
fn registries_for_cache_key(config: &Config) -> Vec<(String, String)> {
    let bootstrap = &config.package_manager_bootstrap;
    let mut registries = vec![("default".to_string(), bootstrap.registry.clone())];
    registries.extend(bootstrap.registries.iter().map(|(key, value)| (key.clone(), value.clone())));
    registries
}

/// Whether the global packages directory already holds the engine that a
/// switch to `version` would install, at exactly that version.
fn is_installed_globally(global_pkg_dir: Option<&Path>, version: &str) -> miette::Result<bool> {
    let Some(global_pkg_dir) = global_pkg_dir else {
        return Ok(false);
    };
    let package = install_pnpm::pnpm_package_to_install(version);
    let existing = find_global_package(global_pkg_dir, package.name)
        .into_diagnostic()
        .wrap_err("scan global packages")?;
    Ok(existing.is_some_and(|existing| {
        install_pnpm::installed_version(&existing.install_dir, package.name).as_deref()
            == Some(version)
    }))
}

/// `true` when pnpm is running under corepack, which manages its own
/// updates (corepack sets `COREPACK_ROOT`).
fn is_executed_by_corepack() -> bool {
    std::env::var_os("COREPACK_ROOT").is_some()
}

fn coerce_major(version: &str) -> Option<u64> {
    node_semver::Version::parse(version).ok().map(|version| version.major)
}

pub(super) fn version_lt(left: &str, right: &str) -> bool {
    match (node_semver::Version::parse(left), node_semver::Version::parse(right)) {
        (Ok(left), Ok(right)) => left < right,
        _ => false,
    }
}

fn range_satisfies(range: &str, version: &str) -> bool {
    let (Ok(range), Ok(parsed)) =
        (node_semver::Range::parse(range), node_semver::Version::parse(version))
    else {
        return false;
    };
    if range.satisfies(&parsed) {
        return true;
    }
    // node-semver rejects a prerelease (`1.2.3-beta.1`) even when its base
    // version is in range; retry with the base so prerelease self-update
    // targets aren't spuriously treated as out of range.
    if parsed.pre_release.is_empty() {
        return false;
    }
    let base = format!("{}.{}.{}", parsed.major, parsed.minor, parsed.patch);
    matches!(node_semver::Version::parse(&base), Ok(base) if range.satisfies(&base))
}

#[cfg(test)]
mod tests;

fn info<Reporter: self::Reporter>(prefix: &str, message: &str) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: message.to_string(),
        prefix: prefix.to_string(),
    }));
}

fn warn<Reporter: self::Reporter>(prefix: &str, message: &str) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: message.to_string(),
        prefix: prefix.to_string(),
    }));
}

async fn switch_global_pnpm<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    target_version: &str,
    prefix: &str,
    bare_specifier: &str,
) -> miette::Result<Option<String>> {
    info::<Reporter>(
        prefix,
        &format!("Switching pnpm from v{PNPM_VERSION} to v{target_version}..."),
    );

    verify_target_engine::<Reporter>(config, target_version, prefix).await?;

    let result = Box::pin(install_pnpm::install_pnpm::<Reporter>(
        config,
        target_version,
        config.supported_architectures.clone(),
    ))
    .await?;

    link_into_global_bin(config, &result, target_version)?;

    if result.already_existed {
        return Ok(Some(format!(
            "The {bare_specifier} version, v{target_version}, is already present on the system. It was activated by linking it from {}.",
            result.install_dir.display(),
        )));
    }
    Ok(Some(format!("Successfully updated pnpm to v{target_version}")))
}

mod project_pin;
