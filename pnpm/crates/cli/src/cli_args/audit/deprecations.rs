//! `pnpm audit deprecations`: the installed packages their registry marks
//! deprecated.

use super::{
    AuditGraph,
    HashMap,
    HashSet,
    Include,
    PackageKey,
    PackageMetadata,
    importers::load_audited_lockfiles,
    origins::{
        Audited,
        AuditedPackage,
        Origin,
        Origins,
        sort_audited,
    },
    walk_reachable,
};
use crate::{
    State,
    cli_args::sanitize::sanitize_inline,
};
use futures_util::{
    StreamExt,
    stream,
};
use pnpm_package_manager::{
    PickPolicy,
    create_configured_registry_resolver,
};
use pnpm_resolving_resolver_base::{
    MetadataRefresh,
    ResolutionRefreshOptions,
    ResolveOptions,
    ResolvedPackageInfo,
    WantedDependency,
};
use serde::Serialize;
use std::{
    path::Path,
    sync::Arc,
};

/// How many registry lookups run at once.
const CONCURRENT_LOOKUPS: usize = 16;

/// How old cached registry metadata may be before the audit asks the
/// registry again. Within it the audit reads what the last install cached.
const METADATA_MAX_AGE: jiff::SignedDuration = jiff::SignedDuration::from_mins(5);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeprecatedPackage {
    pub(super) name: String,
    pub(super) version: String,
    /// The deprecation message the publisher wrote, on one line.
    pub(super) deprecated: String,
    /// The newest version of the package the registry does not mark
    /// deprecated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) non_deprecated_version: Option<String>,
    #[serde(flatten)]
    pub(super) origin: Origin,
    /// Its peer-stripped lockfile key.
    #[serde(skip)]
    pub(super) key: PackageKey,
}

/// A registry package version the lockfile records, with the specifier that
/// resolves it on the registry it was installed from.
struct InstalledPackage {
    key: PackageKey,
    name: String,
    version: String,
    specifier: String,
}

/// The deprecated packages among the installed ones. `None` when the
/// selectors matched no project. A registry error fails the command unless
/// `ignore_registry_errors`, which skips that package.
pub(super) async fn find_deprecated(
    state: &State,
    include: Include,
    lockfile_dir: &Path,
    ignore_registry_errors: bool,
) -> miette::Result<Option<Vec<DeprecatedPackage>>> {
    let Some((lockfile, env_lockfile)) = load_audited_lockfiles(state, lockfile_dir)? else {
        return Ok(None);
    };
    let lockfile = lockfile.as_ref();
    let mut installed = HashMap::new();
    collect_installed(&AuditGraph::main(lockfile, include.peer_edges), include, &mut installed);
    if let Some(env_lockfile) = env_lockfile.as_ref() {
        collect_installed(&AuditGraph::env(env_lockfile), include, &mut installed);
    }
    let mut deprecated = look_up(state, installed.into_values(), ignore_registry_errors).await?;
    sort_audited(&mut deprecated);
    Origins::new(lockfile, env_lockfile.as_ref(), include).add_to(&mut deprecated);
    Ok(Some(deprecated))
}

impl Audited for DeprecatedPackage {
    fn audited(&self) -> AuditedPackage<'_> {
        AuditedPackage { key: &self.key, name: &self.name, version: &self.version }
    }

    fn origin_mut(&mut self) -> &mut Origin {
        &mut self.origin
    }
}

/// Resolves every installed package on the registry it was installed from
/// and keeps the deprecated ones.
async fn look_up(
    state: &State,
    installed: impl Iterator<Item = InstalledPackage>,
    ignore_registry_errors: bool,
) -> miette::Result<Vec<DeprecatedPackage>> {
    let policy = PickPolicy::from_config(state.config).map_err(miette::Report::new)?;
    let resolver =
        create_configured_registry_resolver(state.config, Arc::clone(&state.http_client), &policy)
            .map_err(miette::Report::new)?;
    let options = ResolveOptions {
        refresh: ResolutionRefreshOptions {
            metadata: MetadataRefresh::OlderThan(METADATA_MAX_AGE),
            ..ResolutionRefreshOptions::default()
        },
        ..ResolveOptions::default()
    };
    let lookups: Vec<_> = stream::iter(installed)
        .map(|package| {
            let (resolver, options) = (&resolver, &options);
            async move {
                let wanted = WantedDependency {
                    alias: Some(package.name.clone()),
                    bare_specifier: Some(package.specifier.clone()),
                    ..WantedDependency::default()
                };
                let result = resolver.resolve(&wanted, options).await;
                (package, result)
            }
        })
        .buffer_unordered(CONCURRENT_LOOKUPS)
        .collect()
        .await;
    let mut deprecated = Vec::new();
    for (package, result) in lookups {
        match result {
            Ok(result) => deprecated.extend(deprecation(package, &result.package)),
            Err(error) if ignore_registry_errors => {
                pnpm_diagnostics::eprintln!("{}@{}: {error}", package.name, package.version);
            }
            Err(error) => {
                return Err(miette::miette!("{}@{}: {error}", package.name, package.version));
            }
        }
    }
    Ok(deprecated)
}

fn collect_installed(
    graph: &AuditGraph<'_>,
    include: Include,
    out: &mut HashMap<PackageKey, InstalledPackage>,
) {
    for key in walk_reachable(graph, include, include.optional_dependencies) {
        if let Some(package) = installed_package(&key, graph.packages) {
            out.insert(package.key.clone(), package);
        }
    }
}

/// The registry package `key` names, or `None` for a runtime, directory,
/// tarball, or git package.
fn installed_package(
    key: &PackageKey,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> Option<InstalledPackage> {
    if key.suffix.prefix() == pnpm_lockfile::Prefix::Runtime {
        return None;
    }
    let version = key.suffix.registry_version()?;
    let metadata_key = key.without_peer();
    let is_directory = packages
        .get(&metadata_key)
        .is_some_and(|package| {
            matches!(package.resolution, pnpm_lockfile::LockfileResolution::Directory(_))
        });
    if is_directory {
        return None;
    }
    let specifier = match metadata_key.suffix.registry_qualified() {
        Some((registry_name, version)) => format!("{registry_name}:{version}"),
        None => version.to_string(),
    };
    Some(InstalledPackage {
        name: metadata_key.name.to_string(),
        key: metadata_key,
        version: version.to_string(),
        specifier,
    })
}

fn deprecation(package: InstalledPackage, info: &ResolvedPackageInfo) -> Option<DeprecatedPackage> {
    let manifest = info.manifest.as_ref()?;
    let message = manifest.get("deprecated")?.as_str()?;
    if message.is_empty() {
        return None;
    }
    Some(DeprecatedPackage {
        deprecated: one_line(message),
        non_deprecated_version: info.non_deprecated_alternative
            .as_ref()
            .map(|alternative| alternative.version.clone()),
        name: package.name,
        version: package.version,
        origin: Origin::default(),
        key: package.key,
    })
}

/// A message on one line: line breaks and other whitespace runs become one
/// space, and control characters go.
fn one_line(message: &str) -> String {
    let words: Vec<&str> = message.split_whitespace().collect();
    sanitize_inline(&words.join(" ")).into_owned()
}

/// One block per package name and deprecation message: the message, then
/// each deprecated version with where it comes from.
pub(super) fn render_deprecations(deprecated: &[DeprecatedPackage]) -> String {
    if deprecated.is_empty() {
        return "No deprecated packages found".to_string();
    }
    let groups = group_by_deprecation(deprecated);
    let blocks: Vec<String> = groups
        .iter()
        .map(|group| render_group(group))
        .collect();
    let names = groups
        .iter()
        .map(|group| group[0].name.as_str())
        .collect::<HashSet<_>>()
        .len();
    let noun = if names == 1 { "package" } else { "packages" };
    let summary = if names == deprecated.len() {
        format!("{names} deprecated {noun}")
    } else {
        format!("{names} deprecated {noun}, {} versions", deprecated.len())
    };
    format!("{}\n\n{}", blocks.join("\n\n"), super::red(&summary))
}

/// The packages with the same name, message, and non-deprecated version,
/// each group ordered by version.
fn group_by_deprecation(deprecated: &[DeprecatedPackage]) -> Vec<Vec<&DeprecatedPackage>> {
    let mut groups: Vec<Vec<&DeprecatedPackage>> = Vec::new();
    for package in deprecated {
        let group = groups
            .iter_mut()
            .find(|group| {
                let first = group[0];
                first.name == package.name
                    && first.deprecated == package.deprecated
                    && first.non_deprecated_version == package.non_deprecated_version
            });
        match group {
            Some(group) => group.push(package),
            None => groups.push(vec![package]),
        }
    }
    for group in &mut groups {
        group.sort_by_cached_key(|package| node_semver::Version::parse(&package.version).ok());
    }
    groups
}

fn render_group(group: &[&DeprecatedPackage]) -> String {
    let first = group[0];
    let name = super::bold(&sanitize_inline(&first.name));
    let header = match &first.non_deprecated_version {
        Some(version) => format!(
            "{name}  newest non-deprecated version: {}",
            super::green(&sanitize_inline(version)),
        ),
        None => name,
    };
    let mut lines = vec![header, format!("  {}", first.deprecated)];
    let versions: Vec<_> = group
        .iter()
        .map(|package| sanitize_inline(&package.version))
        .collect();
    let width = versions
        .iter()
        .map(|version| version.chars().count())
        .max()
        .unwrap_or_default();
    for (package, version) in group.iter().zip(&versions) {
        lines.push(
            format!("  {version:<width$}  {}", package.origin.render()).trim_end().to_string(),
        );
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
