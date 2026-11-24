//! `pnpm audit deprecations`: the installed packages their registry marks
//! deprecated.

use super::{
    AuditGraph,
    HashSet,
    Include,
    Lockfile,
    build_audit_path_index,
    importers::load_audited_lockfiles,
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
    ResolveOptions,
    ResolvedPackageInfo,
    WantedDependency,
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::Path,
    sync::Arc,
};

/// How many registry lookups run at once.
const CONCURRENT_LOOKUPS: usize = 16;

/// How many projects a dependency's line names before it counts the rest.
const MAX_LISTED_PROJECTS: usize = 3;

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
    /// The projects that declare it as a direct dependency.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) projects: Vec<String>,
    /// A chain of packages that brings it in, for a package no project
    /// declares.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) via: Vec<String>,
}

/// A registry package version the lockfile records, with the specifier that
/// resolves it on the registry it was installed from.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct InstalledPackage {
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
    let mut installed = BTreeSet::new();
    collect_installed(&AuditGraph::main(lockfile, include.peer_edges), include, &mut installed);
    if let Some(env_lockfile) = env_lockfile.as_ref() {
        collect_installed(&AuditGraph::env(env_lockfile), include, &mut installed);
    }
    let mut deprecated = look_up(state, installed, ignore_registry_errors).await?;
    deprecated.sort_by(|left, right| {
        (&left.name, &left.version).cmp(&(&right.name, &right.version))
    });
    add_paths(&mut deprecated, lockfile, env_lockfile.as_ref(), include);
    Ok(Some(deprecated))
}

/// Resolves every installed package on the registry it was installed from
/// and keeps the deprecated ones.
async fn look_up(
    state: &State,
    installed: BTreeSet<InstalledPackage>,
    ignore_registry_errors: bool,
) -> miette::Result<Vec<DeprecatedPackage>> {
    let policy = PickPolicy::from_config(state.config).map_err(miette::Report::new)?;
    let resolver =
        create_configured_registry_resolver(state.config, Arc::clone(&state.http_client), &policy)
            .map_err(miette::Report::new)?;
    let options = ResolveOptions::default();
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
    out: &mut BTreeSet<InstalledPackage>,
) {
    for key in walk_reachable(graph, include, include.optional_dependencies) {
        if key.suffix.prefix() == pnpm_lockfile::Prefix::Runtime {
            continue;
        }
        let Some(version) = key.suffix.registry_version() else { continue };
        let metadata_key = key.without_peer();
        let is_directory = graph.packages
            .get(&metadata_key)
            .is_some_and(|package| {
                matches!(package.resolution, pnpm_lockfile::LockfileResolution::Directory(_))
            });
        if is_directory {
            continue;
        }
        let specifier = match metadata_key.suffix.registry_qualified() {
            Some((registry_name, version)) => format!("{registry_name}:{version}"),
            None => version.to_string(),
        };
        out.insert(InstalledPackage {
            name: metadata_key.name.to_string(),
            version: version.to_string(),
            specifier,
        });
    }
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
        projects: Vec::new(),
        via: Vec::new(),
    })
}

/// Fills in the projects that declare each package and, for one no project
/// declares, a chain of packages that brings it in. A path reads
/// `<project>>...>name`.
fn add_paths(
    deprecated: &mut [DeprecatedPackage],
    lockfile: &Lockfile,
    env_lockfile: Option<&pnpm_lockfile::EnvLockfile>,
    include: Include,
) {
    let names: HashSet<String> = deprecated
        .iter()
        .map(|package| package.name.clone())
        .collect();
    let index = build_audit_path_index(lockfile, env_lockfile, &names, include);
    for package in deprecated {
        let Some(info) = index
            .get(&package.name)
            .and_then(|versions| versions.get(&package.version))
        else {
            continue;
        };
        let paths: Vec<Vec<&str>> = info.paths
            .iter()
            .map(|path| path.split('>').collect())
            .collect();
        let mut projects: Vec<String> = paths
            .iter()
            .filter(|segments| segments.len() == 2)
            .map(|segments| segments[0].to_string())
            .collect();
        projects.sort();
        projects.dedup();
        if projects.is_empty()
            && let Some(segments) = paths
                .iter()
                .find(|segments| segments.len() > 2)
        {
            package.via = segments[1..segments.len() - 1]
                .iter()
                .map(ToString::to_string)
                .collect();
        }
        package.projects = projects;
    }
}

/// A message on one line: line breaks and other whitespace runs become one
/// space, and control characters go.
fn one_line(message: &str) -> String {
    let words: Vec<&str> = message.split_whitespace().collect();
    sanitize_inline(&words.join(" ")).into_owned()
}

pub(super) fn render_deprecations(deprecated: &[DeprecatedPackage]) -> String {
    if deprecated.is_empty() {
        return "No deprecated packages found".to_string();
    }
    let mut lines = Vec::new();
    for package in deprecated {
        lines.push(format!(
            "{} {}  {}",
            super::bold(&package.name),
            package.version,
            where_from(package),
        ));
        lines.push(format!("  {}", package.deprecated));
        if let Some(version) = &package.non_deprecated_version {
            lines.push(format!("  {} is not deprecated", super::green(version)));
        }
    }
    let count = deprecated.len();
    let noun = if count == 1 { "package" } else { "packages" };
    lines.push(String::new());
    lines.push(super::red(&format!("{count} deprecated {noun}")));
    lines.join("\n")
}

fn where_from(package: &DeprecatedPackage) -> String {
    if !package.via.is_empty() {
        return format!("via {}", package.via.join(" > "));
    }
    let listed = package.projects
        .iter()
        .take(MAX_LISTED_PROJECTS)
        .cloned()
        .collect::<Vec<_>>();
    let rest = package.projects.len() - listed.len();
    if rest == 0 { listed.join(", ") } else { format!("{} and {rest} more", listed.join(", ")) }
}

#[cfg(test)]
mod tests;
