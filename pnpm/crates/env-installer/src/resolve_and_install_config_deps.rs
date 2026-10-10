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
    ConfigDepError,
    install_config_deps::install_config_deps,
    options::ConfigDepsInstallOptions,
    parse_integrity::parse_integrity,
    prune::prune_env_lockfile,
    resolve_optional_subdeps::resolve_optional_subdeps,
    verify_config_dep_resolutions::verify_config_dep_resolutions,
    verify_env_lockfile::{assert_valid_migrated_config_dep, write_verified_env_lockfile},
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
use pnpm_workspace_state::{ConfigDependency, ConfigDependencyDetail};
use ssri::Integrity;
use std::collections::BTreeMap;

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

    let mut to_resolve: Vec<(String, String, Option<Integrity>)> = Vec::new();
    let mut lockfile_changed = drop_removed_config_deps(&mut env_lockfile, config_deps);

    for (name, value) in config_deps {
        let update = updates.prev_specifiers.contains_key(name);
        match plan_config_dep(&mut env_lockfile, opts, name, value, update)? {
            ConfigDepPlan::Satisfied => {}
            ConfigDepPlan::Migrated => lockfile_changed = true,
            ConfigDepPlan::Resolve { specifier, integrity } => {
                to_resolve.push((name.clone(), specifier, integrity));
            }
        }
    }

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

    let mut saved_specifiers = BTreeMap::new();
    for (name, specifier, pinned_integrity) in &to_resolve {
        let request = ResolveRequest {
            name,
            specifier,
            pinned_integrity: pinned_integrity.as_ref(),
            updates: updates.prev_specifiers.contains_key(name).then_some(updates),
        };
        let saved = resolve_one(&mut env_lockfile, resolver, opts, &request).await?;
        if request.updates.is_some() {
            saved_specifiers.insert(name.clone(), saved);
        }
    }

    // Removal, migration and resolution can each orphan packages and
    // snapshots; drop them before writing.
    prune_env_lockfile(&mut env_lockfile);
    verify_config_dep_resolutions(&env_lockfile, config_deps, opts).await?;
    write_verified_env_lockfile(&env_lockfile, opts.root_dir)?;
    install_config_deps::<Reporter>(&env_lockfile, opts).await?;
    Ok(saved_specifiers)
}

/// Drop env-lockfile entries for config deps that were removed from
/// `pnpm-workspace.yaml`, so they stop being installed and get pruned from
/// `.pnpm-config`. Reports whether anything was dropped.
fn drop_removed_config_deps(
    env_lockfile: &mut EnvLockfile,
    config_deps: &BTreeMap<String, ConfigDependency>,
) -> bool {
    let importer = env_lockfile.root_importer_mut();
    let before = importer.config_dependencies.len();
    importer.config_dependencies.retain(|name, _| config_deps.contains_key(name));
    importer.config_dependencies.len() != before
}

/// What one declared config dependency still needs before it can be
/// installed.
enum ConfigDepPlan {
    /// The env lockfile already describes it.
    Satisfied,
    /// It was migrated into the env lockfile from the declaration's own
    /// integrity and tarball, so the lockfile needs writing.
    Migrated,
    /// It has to be resolved against the registry.
    Resolve { specifier: String, integrity: Option<Integrity> },
}

fn plan_config_dep(
    env_lockfile: &mut EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
    name: &str,
    value: &ConfigDependency,
    update: bool,
) -> Result<ConfigDepPlan, ConfigDepError> {
    match value {
        ConfigDependency::Detailed(detail) => plan_detailed(env_lockfile, opts, name, detail),
        ConfigDependency::VersionWithIntegrity(value) if value.contains('+') => {
            plan_pinned(env_lockfile, name, value)
        }
        ConfigDependency::VersionWithIntegrity(specifier) if update => {
            Ok(ConfigDepPlan::Resolve { specifier: specifier.clone(), integrity: None })
        }
        ConfigDependency::VersionWithIntegrity(specifier) => {
            plan_specifier(env_lockfile, name, specifier)
        }
    }
}

/// A declaration carrying its own tarball URL is recorded without a
/// resolution round trip.
fn plan_detailed(
    env_lockfile: &mut EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
    name: &str,
    detail: &ConfigDependencyDetail,
) -> Result<ConfigDepPlan, ConfigDepError> {
    if has_config_dep(env_lockfile, name) {
        return Ok(ConfigDepPlan::Satisfied);
    }
    let (version, integrity) = parse_integrity(name, &detail.integrity)?;
    assert_valid_migrated_config_dep(name, &version)?;
    let Some(tarball) = detail.tarball.clone() else {
        return Ok(ConfigDepPlan::Resolve { specifier: version, integrity: Some(integrity) });
    };
    let registry = opts.verification.pick_registry(name);
    migrate_into_lockfile(env_lockfile, name, &version, integrity, tarball, registry)?;
    Ok(ConfigDepPlan::Migrated)
}

/// A `<version>+<integrity>` declaration pins the integrity but still needs
/// the tarball URL a resolution provides.
fn plan_pinned(
    env_lockfile: &EnvLockfile,
    name: &str,
    value: &str,
) -> Result<ConfigDepPlan, ConfigDepError> {
    if has_config_dep(env_lockfile, name) {
        return Ok(ConfigDepPlan::Satisfied);
    }
    let (version, integrity) = parse_integrity(name, value)?;
    assert_valid_migrated_config_dep(name, &version)?;
    Ok(ConfigDepPlan::Resolve { specifier: version, integrity: Some(integrity) })
}

/// A bare specifier is satisfied only when the lockfile already resolved
/// that exact specifier and still holds the package it resolved to.
fn plan_specifier(
    env_lockfile: &EnvLockfile,
    name: &str,
    specifier: &str,
) -> Result<ConfigDepPlan, ConfigDepError> {
    if let Some(existing) = config_dep(env_lockfile, name)
        && existing.specifier == *specifier
        && env_lockfile.packages.contains_key(&pkg_key(name, &existing.version)?)
    {
        return Ok(ConfigDepPlan::Satisfied);
    }
    Ok(ConfigDepPlan::Resolve { specifier: specifier.to_string(), integrity: None })
}

/// One config dependency to resolve.
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
    let &ResolveRequest {
        name,
        specifier,
        pinned_integrity,
        updates,
    } = request;
    let wanted = request.wanted_dependency();
    let resolve_opts = config_dep_resolve_options(opts, pinned_integrity.is_some(), updates);
    let no_integrity = || missing_config_integrity(name, specifier);
    let result = resolver
        .resolve(&wanted, &resolve_opts)
        .await
        .map_err(|error| ConfigDepError::Resolve { spec: format!("{name}@{specifier}"), error })?
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
    let registry = opts.verification.pick_registry(name);
    let key = pkg_key(name, &version)?;
    // A resolver that does not compute a specifier leaves the declared one.
    let saved_specifier = match (updates, &result.normalized_bare_specifier) {
        (Some(updates), Some(normalized)) if !updates.keeps_specifier(specifier) => {
            normalized.clone()
        }
        _ => specifier.to_string(),
    };

    record_config_dependency(
        env_lockfile,
        &key,
        (name, &saved_specifier),
        &version,
        result.resolution,
        pinned_integrity,
        registry,
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
