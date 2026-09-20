//! `pnpm:root` events for the direct dependencies a hoisted install
//! changed.
//!
//! Every other linker emits them from
//! [`crate::SymlinkDirectDependencies`] as it creates each
//! `node_modules/<alias>` symlink. The hoisted linker writes the package
//! into that path itself and creates no symlink, so `link_only` filters
//! its direct dependencies out of that pass and nothing else reports
//! them (pnpm/pnpm#15161).
//!
//! The symlink outcome answers "did this install put it there". Here the
//! previous install's `<virtual_store_dir>/lock.yaml` answers it.

use crate::{HoistedLinkerInputs, SkippedSnapshots, symlink_direct_dependencies::fallback_version};
use pnpm_lockfile::{
    ImporterDepVersion, Lockfile, PackageKey, PackageMetadata, PkgName, ProjectSnapshot,
    ResolvedDependencySpec,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::{
    AddedRoot, DependencyType, LogEvent, LogLevel, RemovedRoot, Reporter, RootLog, RootMessage,
};
use std::collections::{HashMap, HashSet};

/// Emit one `pnpm:root` event per direct dependency this install added,
/// replaced or dropped, for each project the run selected.
pub fn report_direct_dependency_changes<Reporter: self::Reporter>(
    inputs: &HoistedLinkerInputs<'_>,
    wanted: &Lockfile,
    skipped: &SkippedSnapshots,
) {
    for (project_dir, _) in inputs.projects.manifests {
        let importer_id = pnpm_workspace::importer_id_from_root_dir(
            inputs.projects.walker_lockfile_dir,
            project_dir,
        );
        report_importer::<Reporter>(
            inputs.prior.current_lockfile,
            wanted,
            inputs.projects.dependency_groups,
            skipped,
            &importer_id,
            &project_dir.to_string_lossy(),
        );
    }
}

/// One importer's share of [`report_direct_dependency_changes`].
fn report_importer<Reporter: self::Reporter>(
    previous: Option<&Lockfile>,
    wanted: &Lockfile,
    dependency_groups: &[DependencyGroup],
    skipped: &SkippedSnapshots,
    importer_id: &str,
    prefix: &str,
) {
    let previous_packages = previous.and_then(|lockfile| lockfile.packages.as_ref());
    let before = direct_dependencies(
        previous.and_then(|lockfile| lockfile.importers.get(importer_id)),
        dependency_groups,
        skipped,
    );
    let after = direct_dependencies(wanted.importers.get(importer_id), dependency_groups, skipped);
    for &(name, group, spec) in &after {
        if let Some(&(_, was_group, was_spec)) = before
            .iter()
            .find(|&&(before_name, _, _)| before_name == name)
        {
            if was_spec.version == spec.version {
                continue;
            }
            emit_removed::<Reporter>(name, was_group, was_spec, previous_packages, prefix);
        }
        emit_added::<Reporter>(name, group, spec, wanted.packages.as_ref(), prefix);
    }
    for &(name, group, spec) in &before {
        if after
            .iter()
            .all(|&(after_name, _, _)| after_name != name)
        {
            emit_removed::<Reporter>(name, group, spec, previous_packages, prefix);
        }
    }
}

/// The importer's direct dependencies, first group wins on a name that
/// appears in several. `link:` dependencies are left out: they are
/// symlinked even under the hoisted linker, so they are already reported.
/// So are the packages in `skipped`, which the install resolved but left
/// uninstalled, an unsupported optional dependency among them.
fn direct_dependencies<'a>(
    snapshot: Option<&'a ProjectSnapshot>,
    dependency_groups: &[DependencyGroup],
    skipped: &SkippedSnapshots,
) -> Vec<(&'a PkgName, DependencyGroup, &'a ResolvedDependencySpec)> {
    let mut seen: HashSet<&PkgName> = HashSet::new();
    dependency_groups
        .iter()
        .copied()
        .filter(|group| !matches!(group, DependencyGroup::Peer))
        .flat_map(|group| {
            snapshot
                .and_then(|snapshot| snapshot.get_map_by_group(group))
                .into_iter()
                .flatten()
                .map(move |(name, spec)| (name, group, spec))
        })
        .filter(|(_, _, spec)| !matches!(spec.version, ImporterDepVersion::Link(_)))
        .filter(|(name, _, spec)| {
            spec.version
                .resolved_key(name)
                .is_none_or(|resolved| !skipped.contains(&resolved))
        })
        .filter(|(name, _, _)| seen.insert(*name))
        .collect()
}

fn emit_added<Reporter: self::Reporter>(
    name: &PkgName,
    group: DependencyGroup,
    spec: &ResolvedDependencySpec,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    prefix: &str,
) {
    let Some(dependency_type) = dependency_type(group) else {
        return;
    };
    Reporter::emit(&LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Added {
            prefix: prefix.to_owned(),
            added: AddedRoot {
                name: name.to_string(),
                real_name: real_name(name, spec),
                version: Some(resolved_version(name, spec, packages)),
                dependency_type: Some(dependency_type),
                id: None,
                latest: None,
                linked_from: None,
            },
        },
    }));
}

fn emit_removed<Reporter: self::Reporter>(
    name: &PkgName,
    group: DependencyGroup,
    spec: &ResolvedDependencySpec,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    prefix: &str,
) {
    let Some(dependency_type) = dependency_type(group) else {
        return;
    };
    Reporter::emit(&LogEvent::Root(RootLog {
        level: LogLevel::Debug,
        message: RootMessage::Removed {
            prefix: prefix.to_owned(),
            removed: RemovedRoot {
                name: name.to_string(),
                version: Some(resolved_version(name, spec, packages)),
                dependency_type: Some(dependency_type),
            },
        },
    }));
}

/// The version behind the range the manifest records. Falls back to what
/// the lockfile's importer entry carries when the package has no metadata
/// row, which is what [`crate::SymlinkDirectDependencies`] reports too.
fn resolved_version(
    name: &PkgName,
    spec: &ResolvedDependencySpec,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> String {
    spec.version
        .resolved_key(name)
        .and_then(|key| packages?.get(&key.without_peer()))
        .and_then(|metadata| metadata.version.clone())
        .unwrap_or_else(|| fallback_version(&spec.version))
}

/// An alias carries the package it resolves to; every other shape is
/// named by its own key.
fn real_name(name: &PkgName, spec: &ResolvedDependencySpec) -> String {
    match &spec.version {
        ImporterDepVersion::Alias(alias) => alias.name.to_string(),
        ImporterDepVersion::Regular(_)
        | ImporterDepVersion::Link(_)
        | ImporterDepVersion::File(_) => name.to_string(),
    }
}

/// Peers are materialized through their host package rather than under
/// `node_modules` directly, so pnpm emits no `pnpm:root` for them.
fn dependency_type(group: DependencyGroup) -> Option<DependencyType> {
    match group {
        DependencyGroup::Prod => Some(DependencyType::Prod),
        DependencyGroup::Dev => Some(DependencyType::Dev),
        DependencyGroup::Optional => Some(DependencyType::Optional),
        DependencyGroup::Peer => None,
    }
}
