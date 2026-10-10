//! Resolve any config dependencies missing from the env lockfile, then
//! install them all.
//!
//! Handles three input shapes:
//! 1. old object form `{ tarball?, integrity }` — migrated inline into
//!    the lockfile when it carries a tarball URL, otherwise resolved,
//! 2. old string form `<version>+<integrity>` — resolved against the
//!    registry for its tarball URL, keeping the inline integrity,
//! 3. new clean specifier (`1.2.0` / `^1.0.0`) — resolved against the
//!    registry when it isn't already pinned in the lockfile.

use crate::{
    ConfigDepError, install_config_deps::install_config_deps, options::ConfigDepsInstallOptions,
    prune::prune_env_lockfile, resolve_optional_subdeps::resolve_optional_subdeps,
    verify_config_dep_resolutions::verify_config_dep_resolutions,
    verify_env_lockfile::write_verified_env_lockfile,
};
use pnpm_lockfile::{
    EnvLockfile, LockfileFormOptions, LockfileResolution, PackageKey, PackageMetadata,
    SnapshotEntry, SpecifierAndResolution, TarballResolution,
};
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::{
    ResolutionPolicyOptions, ResolveOptions, ResolveResult, Resolver, UpdateBehavior,
    VersionSelectorType, WantedDependency,
};
use pnpm_workspace_state::ConfigDependency;
use ssri::Integrity;
use std::collections::BTreeMap;

mod plan;
use plan::{ToResolve, plan_config_deps};

/// Config deps keep the npm tarball layout: `registries` is a workspace
/// setting, and config deps are resolved before workspace settings apply. The
/// writer and the reader here agree because both use this same default.
fn npm_lockfile_form(registry: &str) -> LockfileFormOptions<'_> {
    LockfileFormOptions { registry, server_type: None, include_tarball_url: false }
}

/// Resolve + install the config dependencies declared in
/// `pnpm-workspace.yaml` (`config_deps`).
pub async fn resolve_and_install_config_deps<Reporter: self::Reporter>(
    config_deps: &BTreeMap<String, ConfigDependency>,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<(), ConfigDepError> {
    let updates = ConfigDepUpdates::default();
    resolve_and_install_config_deps_updating::<Reporter>(config_deps, &updates, resolver, opts)
        .await?;
    Ok(())
}

/// The config dependencies `add --config` and `update` resolve again, and
/// how the specifier each is saved with is chosen. That follows the rules a
/// project dependency's manifest specifier follows.
#[derive(Debug, Default)]
pub struct ConfigDepUpdates {
    /// Each config dependency to resolve again, with the specifier
    /// `pnpm-workspace.yaml` declared for it before the command, if any.
    pub prev_specifiers: BTreeMap<String, Option<String>>,
    /// [`UpdateBehavior::Off`] for `add --config`. An update reaches past
    /// the declared range to the `latest` tag with
    /// [`UpdateBehavior::Latest`].
    pub behavior: UpdateBehavior,
    /// The operator a saved specifier gets when the declared one has none.
    pub range_spec_style: RangeSpecStyle,
}

impl ConfigDepUpdates {
    /// Whether `specifier` is saved as declared. An update keeps a dist-tag
    /// declaration tracking the tag, as it keeps one in `package.json`.
    fn keeps_specifier(&self, specifier: &str) -> bool {
        self.behavior != UpdateBehavior::Off
            && get_version_selector_type(specifier) == Some(VersionSelectorType::Tag)
    }
}

/// [`resolve_and_install_config_deps`], but the config dependencies in
/// `updates` are resolved again even when the env lockfile already holds
/// their specifier. Returns the specifier each of them is recorded with,
/// which `pnpm-workspace.yaml` must declare from now on.
pub async fn resolve_and_install_config_deps_updating<Reporter: self::Reporter>(
    config_deps: &BTreeMap<String, ConfigDependency>,
    updates: &ConfigDepUpdates,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<BTreeMap<String, String>, ConfigDepError> {
    let mut env_lockfile = EnvLockfile::read(opts.root_dir)
        .map_err(ConfigDepError::ReadLockfile)?
        .unwrap_or_else(EnvLockfile::create);
    let (to_resolve, lockfile_changed) =
        plan_config_deps(&mut env_lockfile, config_deps, updates, opts)?;

    if opts.frozen_lockfile && (lockfile_changed || !to_resolve.is_empty()) {
        return Err(ConfigDepError::FrozenLockfileOutdated {
            message: r#"Cannot update configDependencies with "frozen-lockfile" because the lockfile is not up to date"#.to_string(),
        });
    }

    if to_resolve.is_empty() && !lockfile_changed {
        verify_config_dep_resolutions(&env_lockfile, config_deps, opts).await?;
        install_config_deps::<Reporter>(&env_lockfile, opts).await?;
        return Ok(BTreeMap::new());
    }

    let saved_specifiers =
        resolve_config_deps(&mut env_lockfile, &to_resolve, updates, resolver, opts).await?;
    // Removal, migration and resolution can each orphan packages and
    // snapshots; drop them before writing.
    prune_env_lockfile(&mut env_lockfile);
    verify_config_dep_resolutions(&env_lockfile, config_deps, opts).await?;
    write_verified_env_lockfile(&env_lockfile, opts.root_dir)?;
    install_config_deps::<Reporter>(&env_lockfile, opts).await?;
    Ok(saved_specifiers)
}

/// Resolve `to_resolve` into the env lockfile. Returns the specifier each
/// one in `updates` is recorded with.
async fn resolve_config_deps(
    env_lockfile: &mut EnvLockfile,
    to_resolve: &[ToResolve],
    updates: &ConfigDepUpdates,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<BTreeMap<String, String>, ConfigDepError> {
    let mut saved_specifiers = BTreeMap::new();
    for (name, specifier, pinned_integrity) in to_resolve {
        let request = ResolveRequest {
            name,
            specifier,
            pinned_integrity: pinned_integrity.as_ref(),
            updates: updates.prev_specifiers.contains_key(name).then_some(updates),
        };
        let saved = resolve_one(env_lockfile, resolver, opts, &request).await?;
        if request.updates.is_some() {
            saved_specifiers.insert(name.clone(), saved);
        }
    }
    Ok(saved_specifiers)
}

/// One config dependency to resolve.
#[derive(Clone, Copy)]
struct ResolveRequest<'a> {
    name: &'a str,
    specifier: &'a str,
    pinned_integrity: Option<&'a Integrity>,
    /// Set when `add --config` or `update` resolves it again.
    updates: Option<&'a ConfigDepUpdates>,
}

impl ResolveRequest<'_> {
    fn wanted_dependency(&self) -> WantedDependency {
        WantedDependency {
            alias: Some(self.name.to_string()),
            bare_specifier: Some(self.specifier.to_string()),
            prev_specifier: self.updates.and_then(|updates| {
                updates.prev_specifiers
                    .get(self.name)
                    .cloned()
                    .flatten()
            }),
            ..WantedDependency::default()
        }
    }

    /// Resolve the request, rejecting a resolution that broke a policy or
    /// carries no integrity. Returns the result and the version it picked.
    async fn resolve(
        &self,
        resolver: &dyn Resolver,
        opts: &ConfigDepsInstallOptions<'_>,
    ) -> Result<(ResolveResult, String), ConfigDepError> {
        let Self {
            name,
            specifier,
            pinned_integrity,
            updates,
        } = *self;
        let resolve_opts = config_dep_resolve_options(opts, pinned_integrity.is_some(), updates);
        let no_integrity = || missing_config_integrity(name, specifier);
        let result = resolver
            .resolve(&self.wanted_dependency(), &resolve_opts)
            .await
            .map_err(|error| ConfigDepError::Resolve {
                spec: format!("{name}@{specifier}"),
                error,
            })?
            .ok_or_else(no_integrity)?;
        assert_no_policy_violation(&result)?;
        if !crate::resolve_optional_subdeps::resolution_has_integrity(&result.resolution) {
            return Err(no_integrity());
        }
        let version = result.package.name_ver
            .as_ref()
            .ok_or_else(no_integrity)?
            .suffix
            .to_string();
        Ok((result, version))
    }

    /// The specifier the resolution is recorded with. A resolver that does
    /// not compute one leaves the declared specifier.
    fn saved_specifier(&self, result: &ResolveResult) -> String {
        match (self.updates, &result.normalized_bare_specifier) {
            (Some(updates), Some(normalized)) if !updates.keeps_specifier(self.specifier) => {
                normalized.clone()
            }
            _ => self.specifier.to_string(),
        }
    }
}

/// Resolve a single config dependency and record it (plus one level of
/// optional subdeps) into the env lockfile. Returns the specifier it is
/// recorded with.
async fn resolve_one(
    env_lockfile: &mut EnvLockfile,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
    request: &ResolveRequest<'_>,
) -> Result<String, ConfigDepError> {
    let (result, version) = request.resolve(resolver, opts).await?;
    let ResolveRequest { name, pinned_integrity, .. } = *request;
    let key = pkg_key(name, &version)?;
    let saved_specifier = request.saved_specifier(&result);
    record_config_dependency(
        env_lockfile,
        &key,
        (name, &saved_specifier),
        &version,
        result.resolution,
        pinned_integrity,
        opts.verification.pick_registry(name),
    )?;

    // A pinned dependency covers only itself, so its optional subdeps stay out
    // of the lockfile until it is declared as a clean specifier.
    let optional_subdeps = match (pinned_integrity, result.package.manifest.as_deref()) {
        (None, Some(manifest)) => {
            resolve_optional_subdeps(name, manifest, resolver, opts, env_lockfile).await?
        }
        _ => None,
    };
    env_lockfile.snapshots.insert(
        key,
        SnapshotEntry { optional_dependencies: optional_subdeps, ..SnapshotEntry::default() },
    );
    Ok(saved_specifier)
}

fn missing_config_integrity(name: &str, specifier: &str) -> ConfigDepError {
    ConfigDepError::BadConfigDep {
        message: format!(
            "Cannot resolve {name}@{specifier} as a configuration dependency because it has no integrity",
        ),
    }
}

fn record_config_dependency(
    env_lockfile: &mut EnvLockfile,
    key: &PackageKey,
    declaration: (&str, &str),
    version: &str,
    mut resolution: LockfileResolution,
    pinned_integrity: Option<&Integrity>,
    registry: &str,
) -> Result<(), ConfigDepError> {
    let (name, specifier) = declaration;
    env_lockfile
        .root_importer_mut()
        .config_dependencies
        .insert(
            name.to_string(),
            SpecifierAndResolution {
                specifier: specifier.to_string(),
                version: version.to_string(),
            },
        );
    pin_integrity(&mut resolution, pinned_integrity);
    env_lockfile.packages.insert(
        key.clone(),
        registry_package_metadata(
            resolution
                .to_lockfile_form(name, version, npm_lockfile_form(registry))
                .map_err(ConfigDepError::LockfileForm)?,
        ),
    );

    Ok(())
}

/// A migrated dependency keeps the integrity pinned in pnpm-workspace.yaml,
/// so the registry hands over the tarball URL without loosening the pin.
fn pin_integrity(resolution: &mut LockfileResolution, pinned: Option<&Integrity>) {
    if let (Some(pinned), LockfileResolution::Tarball(tarball)) = (pinned, resolution) {
        tarball.integrity = Some(pinned.clone());
    }
}

pub(crate) fn resolve_options(opts: &ConfigDepsInstallOptions<'_>) -> ResolveOptions {
    ResolveOptions {
        project: pnpm_resolving_resolver_base::ResolverProjectOptions {
            project_dir: opts.root_dir.to_path_buf(),
            lockfile_dir: opts.root_dir.to_path_buf(),
            ..Default::default()
        },
        policy: opts.verification.resolution_policy.clone(),
        ..ResolveOptions::default()
    }
}

/// A `version+integrity` pin only needs the tarball URL, and verification
/// skips pins, so a pin resolves without
/// [`ConfigDependencyVerification::resolution_policy`](crate::ConfigDependencyVerification::resolution_policy).
fn config_dep_resolve_options(
    opts: &ConfigDepsInstallOptions<'_>,
    pinned: bool,
    updates: Option<&ConfigDepUpdates>,
) -> ResolveOptions {
    let mut resolve_opts = resolve_options(opts);
    if pinned {
        resolve_opts.policy = ResolutionPolicyOptions::default();
    }
    if let Some(updates) = updates {
        resolve_opts.refresh.update = updates.behavior;
        resolve_opts.specifier.calc_specifier = true;
        resolve_opts.specifier.range_spec_style = Some(updates.range_spec_style);
    }
    resolve_opts
}

/// Reject a resolution that broke a policy in
/// [`ConfigDependencyVerification::resolution_policy`](crate::ConfigDependencyVerification::resolution_policy).
/// The resolution verifiers would reject it on the next clean install.
pub(crate) fn assert_no_policy_violation(result: &ResolveResult) -> Result<(), ConfigDepError> {
    match &result.policy_violation {
        None => Ok(()),
        Some(violation) => Err(ConfigDepError::BadConfigDep {
            message: format!(
                r#"Configuration dependency "{}@{}" {}"#,
                violation.name, violation.version, violation.reason,
            ),
        }),
    }
}

/// Insert the lockfile entries for an old-format config dependency
/// being migrated inline (object or `version+integrity` string form).
fn migrate_into_lockfile(
    env_lockfile: &mut EnvLockfile,
    name: &str,
    version: &str,
    integrity: Integrity,
    tarball: String,
    registry: &str,
) -> Result<(), ConfigDepError> {
    let key = pkg_key(name, version)?;
    env_lockfile
        .root_importer_mut()
        .config_dependencies
        .insert(
            name.to_string(),
            SpecifierAndResolution { specifier: version.to_string(), version: version.to_string() },
        );
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity),
        revision: None,
        git_hosted: None,
        path: None,
    })
    .to_lockfile_form(name, version, npm_lockfile_form(registry))
    .map_err(ConfigDepError::LockfileForm)?;
    env_lockfile.packages.insert(key.clone(), registry_package_metadata(resolution));
    env_lockfile.snapshots.insert(key, SnapshotEntry::default());
    Ok(())
}

/// A `packages:` entry carrying only a resolution — the shape a config
/// dependency (with no peer/engine metadata of its own) takes.
fn registry_package_metadata(resolution: LockfileResolution) -> PackageMetadata {
    PackageMetadata {
        resolution,
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn has_config_dep(env_lockfile: &EnvLockfile, name: &str) -> bool {
    config_dep(env_lockfile, name).is_some()
}

fn config_dep<'a>(env_lockfile: &'a EnvLockfile, name: &str) -> Option<&'a SpecifierAndResolution> {
    env_lockfile.importers.get(EnvLockfile::ROOT_IMPORTER_KEY)?.config_dependencies.get(name)
}

fn pkg_key(name: &str, version: &str) -> Result<PackageKey, ConfigDepError> {
    format!("{name}@{version}")
        .parse()
        .map_err(|_| ConfigDepError::BadConfigDep {
            message: format!("Config dependency {name}@{version} has an unparsable lockfile key"),
        })
}
