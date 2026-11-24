//! `pnpm audit engines`: the installed packages that don't support the
//! Node.js version or the platform an install checks them against.

use super::{
    AuditGraph,
    HashMap,
    HashSet,
    Include,
    Lockfile,
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
    package_version,
    walk_reachable,
};
use crate::cli_args::sanitize::sanitize_inline;
use pnpm_config::Config;
use pnpm_deps_restorer::{
    InstallabilityHost,
    incompatible_required_packages,
    materialization_plan::{
        detect_installability_host,
        with_locked_runtime_node,
    },
};
use pnpm_lockfile::ProjectSnapshot;
use pnpm_package_is_installable::{
    InstallabilityError,
    UnsupportedEngineError,
    UnsupportedPlatformError,
};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IncompatiblePackage {
    pub(super) name: String,
    pub(super) version: String,
    #[serde(flatten)]
    pub(super) unsupported: Unsupported,
    #[serde(flatten)]
    pub(super) origin: Origin,
    /// Its peer-stripped lockfile key.
    #[serde(skip)]
    pub(super) key: PackageKey,
}

/// What a package wants that the host does not have. `wanted` lists only
/// the unsatisfied entries.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "unsupported", rename_all = "camelCase")]
pub(super) enum Unsupported {
    Engine { wanted: Engines, current: Engines },
    Platform { wanted: PlatformFields, current: PlatformFields },
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct Engines {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) pnpm: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct PlatformFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) os: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) cpu: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) libc: Option<Vec<String>>,
}

/// The installed packages that an install warns about because they don't
/// support the host. `None` when the selectors matched no project.
pub(super) async fn find_incompatible(
    state: &crate::State,
    include: Include,
    lockfile_dir: &Path,
) -> miette::Result<Option<Vec<IncompatiblePackage>>> {
    let Some((lockfile, env_lockfile)) = load_audited_lockfiles(state, lockfile_dir)? else {
        return Ok(None);
    };
    let lockfile = lockfile.as_ref();
    let (Some(snapshots), Some(packages)) = (&lockfile.snapshots, &lockfile.packages) else {
        return Ok(Some(Vec::new()));
    };
    let host = installability_host(state.config, lockfile).await;
    let incompatible = incompatible_required_packages(
        &included_importers(lockfile, include),
        snapshots,
        packages,
        &host,
    )
    .map_err(|error| miette::Report::new(*error))?;
    let mut found = installed_incompatible(lockfile, include, incompatible, packages);
    sort_audited(&mut found);
    Origins::new(lockfile, env_lockfile.as_ref(), include).add_to(&mut found);
    Ok(Some(found))
}

/// The packages of `incompatible` that `include` installs.
fn installed_incompatible(
    lockfile: &Lockfile,
    include: Include,
    incompatible: HashMap<PackageKey, InstallabilityError>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> Vec<IncompatiblePackage> {
    let installed = installed_keys(lockfile, include);
    incompatible
        .into_iter()
        .filter(|(key, _)| installed.contains(key))
        .filter_map(|(key, error)| incompatible_package(key, &error, packages))
        .collect()
}

impl Audited for IncompatiblePackage {
    fn audited(&self) -> AuditedPackage<'_> {
        AuditedPackage { key: &self.key, name: &self.name, version: &self.version }
    }

    fn origin_mut(&mut self) -> &mut Origin {
        &mut self.origin
    }
}

/// The host an install checks the packages against: the configured
/// `nodeVersion`, the Node.js the lockfile pins for the root project, or
/// the system Node.js, and the `supportedArchitectures`.
async fn installability_host(config: &Config, lockfile: &Lockfile) -> InstallabilityHost {
    let detected = detect_installability_host(
        true,
        false,
        config.node_version.clone(),
        config.supported_architectures.as_ref(),
    )
    .await;
    with_locked_runtime_node(detected.as_ref(), config, &lockfile.importers)
        .expect("a needed detection returns a host")
}

/// The importers with only the dependency groups `include` selects.
fn included_importers(lockfile: &Lockfile, include: Include) -> HashMap<String, ProjectSnapshot> {
    lockfile.importers
        .iter()
        .map(|(id, importer)| {
            let mut importer = importer.clone();
            if !include.dependencies {
                importer.dependencies = None;
            }
            if !include.dev_dependencies {
                importer.dev_dependencies = None;
            }
            if !include.optional_dependencies {
                importer.optional_dependencies = None;
            }
            (id.clone(), importer)
        })
        .collect()
}

/// The peer-stripped keys of the packages `include` installs.
fn installed_keys(lockfile: &Lockfile, include: Include) -> HashSet<PackageKey> {
    let graph = AuditGraph::main(lockfile, include.peer_edges);
    walk_reachable(&graph, include, include.optional_dependencies)
        .into_iter()
        .map(|key| key.without_peer())
        .collect()
}

fn incompatible_package(
    key: PackageKey,
    error: &InstallabilityError,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> Option<IncompatiblePackage> {
    let unsupported = match error {
        InstallabilityError::Engine(error) => engine(error),
        InstallabilityError::Platform(error) => platform(error),
        InstallabilityError::InvalidNodeVersion(_) => return None,
    };
    Some(IncompatiblePackage {
        name: key.name.to_string(),
        version: package_version(&key, Some(packages))
            .unwrap_or_else(|| key.suffix.to_string()),
        unsupported,
        origin: Origin::default(),
        key,
    })
}

fn engine(error: &UnsupportedEngineError) -> Unsupported {
    Unsupported::Engine {
        wanted: Engines { node: error.wanted.node.clone(), pnpm: error.wanted.pnpm.clone() },
        current: Engines {
            node: error.wanted.node.as_ref().map(|_| error.current.node.clone()),
            pnpm: error.wanted.pnpm.as_ref().and_then(|_| error.current.pnpm.clone()),
        },
    }
}

fn platform(error: &UnsupportedPlatformError) -> Unsupported {
    Unsupported::Platform {
        wanted: PlatformFields {
            os: error.wanted.os.clone(),
            cpu: error.wanted.cpu.clone(),
            libc: error.wanted.libc.clone(),
        },
        current: PlatformFields {
            os: Some(error.current.os.clone()),
            cpu: Some(error.current.cpu.clone()),
            libc: Some(error.current.libc.clone()),
        },
    }
}

pub(super) fn render_incompatible(found: &[IncompatiblePackage]) -> String {
    if found.is_empty() {
        return "No incompatible packages found".to_string();
    }
    let mut lines = Vec::new();
    for package in found {
        lines.push(format!(
            "{} {}  wants {}, not {}  {}",
            super::bold(&sanitize_inline(&package.name)),
            sanitize_inline(&package.version),
            sanitize_inline(&wanted_text(&package.unsupported)),
            current_text(&package.unsupported),
            package.origin.render(),
        ));
    }
    let count = found.len();
    let noun = if count == 1 { "package" } else { "packages" };
    lines.push(String::new());
    lines.push(super::red(&format!("{count} incompatible {noun}")));
    lines.join("\n")
}

/// Such as `Node.js >=18 <26` or `os darwin or linux and cpu arm64`.
fn wanted_text(unsupported: &Unsupported) -> String {
    match unsupported {
        Unsupported::Engine { wanted, .. } => engines_text(wanted),
        Unsupported::Platform { wanted, .. } => {
            pnpm_default_reporter::state::platform_requirement(&pnpm_reporter::WantedPlatform {
                os: wanted.os.clone(),
                cpu: wanted.cpu.clone(),
                libc: wanted.libc.clone(),
            })
        }
    }
}

/// Such as `26.13.0` or `linux x64 glibc`.
fn current_text(unsupported: &Unsupported) -> String {
    match unsupported {
        Unsupported::Engine { current, .. } => engines_text(current),
        Unsupported::Platform { current, .. } => [&current.os, &current.cpu, &current.libc]
            .into_iter()
            .flatten()
            .flatten()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn engines_text(engines: &Engines) -> String {
    [("Node.js", &engines.node), ("pnpm", &engines.pnpm)]
        .into_iter()
        .filter_map(|(engine, version)| Some(format!("{engine} {}", version.as_ref()?)))
        .collect::<Vec<_>>()
        .join(" and ")
}

#[cfg(test)]
mod tests;
