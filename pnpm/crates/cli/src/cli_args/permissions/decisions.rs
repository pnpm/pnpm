use super::{
    ALL_CAPABILITIES, Capabilities, IgnoredBuildsScan, PermissionCapability, emit_global_warning,
    partition_params, short_package_label,
};
use indexmap::IndexSet;
use miette::IntoDiagnostic;
use pnpm_config::SkillsPermission;
use pnpm_modules_yaml::{Host, write_modules_manifest};
use pnpm_package_manager::allow_build_key_from_ignored_build;
use pnpm_reporter::Reporter;
use pnpm_workspace_manifest_writer::{PermissionValue, set_permissions};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// The decision for every package a run decides: the capabilities it asked
/// for, each in the shape `permissions` records.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct PackageDecisions {
    /// The `build` verdict.
    pub(crate) build: Option<bool>,
    /// The `skills` verdict: all, none, or the chosen skills.
    pub(crate) skills: Option<SkillsPermission>,
}

pub(crate) type Decisions = BTreeMap<String, PackageDecisions>;

/// The decisions of one approval run, plus the `skills.dirs` answer that
/// goes with them. Recording nothing keeps the detected directories.
#[derive(Debug)]
pub(crate) struct Approval {
    pub(crate) decisions: Decisions,
    pub(crate) skills_dirs: Vec<String>,
}

pub(super) fn write_decisions(settings_dir: &Path, decisions: &Decisions) -> miette::Result<()> {
    let entries = decisions
        .iter()
        .flat_map(|(pkg, decided)| {
            decided.build
                .map(|allowed| (pkg.as_str(), PermissionValue::Build(allowed)))
                .into_iter()
                .chain(
                    decided.skills
                        .clone()
                        .map(|skills| (pkg.as_str(), PermissionValue::Skills(skills))),
                )
        })
        .collect::<Vec<_>>();
    set_permissions(settings_dir, entries).into_diagnostic()
}

/// The packages approved to build that were awaiting it. A package
/// approved before it was installed has nothing to rebuild.
pub(super) fn pending_builds_approved(
    decisions: &Decisions,
    pending: &BTreeMap<String, Capabilities>,
) -> Vec<String> {
    decisions
        .iter()
        .filter_map(|(pkg, decided)| {
            decided.build
                .filter(|&allowed| {
                    allowed
                        && pending
                            .get(pkg)
                            .is_some_and(|requested| {
                                requested.contains(&PermissionCapability::Build)
                            })
                })
                .map(|_| pkg.clone())
        })
        .collect()
}

pub(crate) fn confirmation(
    approved: &BTreeSet<String>,
    pending: &BTreeMap<String, Capabilities>,
) -> String {
    let packages = approved
        .iter()
        .map(|pkg| format!("{} ({})", short_package_label(pkg), capability_list(&pending[pkg])))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Approve {packages}?")
}

/// The decisions of `<pkg>` / `!<pkg>` arguments. A package that is not
/// awaiting approval is decided for every capability.
pub(super) fn named_decisions<Reporter: self::Reporter>(
    params: &[String],
    pending: &BTreeMap<String, Capabilities>,
) -> miette::Result<Decisions> {
    let pending_names: Vec<String> = pending.keys().cloned().collect();
    let partition = partition_params(params, &pending_names);
    if !partition.unknown.is_empty() {
        emit_global_warning::<Reporter>(&format!(
            "The following packages are not awaiting approval: {}",
            partition.unknown.join(", "),
        ));
    }
    partition.reject_contradictions()?;
    let decide = |names: &[String], value: bool| -> Vec<(String, PackageDecisions)> {
        names
            .iter()
            .map(|pkg| {
                let capabilities: Capabilities = pending
                    .get(pkg)
                    .cloned()
                    .unwrap_or_else(|| ALL_CAPABILITIES.into_iter().collect());
                let decided = PackageDecisions {
                    build: capabilities.contains(&PermissionCapability::Build).then_some(value),
                    skills: capabilities
                        .contains(&PermissionCapability::Skills)
                        .then_some(SkillsPermission::All(value)),
                };
                (pkg.clone(), decided)
            })
            .collect()
    };
    Ok(decide(&partition.approved, true)
        .into_iter()
        .chain(decide(&partition.denied, false))
        .collect())
}

/// The decisions for `--all`: every capability of every pending package is
/// approved.
pub(super) fn decisions_for_all(pending: &BTreeMap<String, Capabilities>) -> Decisions {
    pending
        .iter()
        .map(|(pkg, capabilities)| {
            (
                pkg.clone(),
                PackageDecisions {
                    build: capabilities.contains(&PermissionCapability::Build).then_some(true),
                    skills: capabilities
                        .contains(&PermissionCapability::Skills)
                        .then_some(SkillsPermission::All(true)),
                },
            )
        })
        .collect()
}

/// A decision for every pending capability of the packages named in
/// `approved`; the packages left out are denied, so they are not offered
/// again. An approved package whose skills were reviewed takes the
/// chosen decision; an approved package whose skills could not be
/// discovered leaves them undecided so they remain pending.
pub(super) fn decisions_by_approval(
    approved: &BTreeSet<String>,
    pending: &BTreeMap<String, Capabilities>,
    skills_by_package: &BTreeMap<String, SkillsPermission>,
) -> Decisions {
    pending
        .iter()
        .map(|(pkg, capabilities)| {
            let value = approved.contains(pkg);
            let skills = if value {
                skills_by_package.get(pkg).cloned()
            } else {
                capabilities
                    .contains(&PermissionCapability::Skills)
                    .then_some(SkillsPermission::All(false))
            };
            (
                pkg.clone(),
                PackageDecisions {
                    build: capabilities.contains(&PermissionCapability::Build).then_some(value),
                    skills,
                },
            )
        })
        .collect()
}

/// Drop the decided entries from `.modules.yaml`'s pending build scripts
/// and agent skills.
pub(super) fn clear_decided(scan: IgnoredBuildsScan, decisions: &Decisions) -> miette::Result<()> {
    let Some(mut modules) = scan.modules_manifest else { return Ok(()) };
    let decided = |dep_path: &str, capability: PermissionCapability| {
        decisions
            .get(&allow_build_key_from_ignored_build(dep_path))
            .is_some_and(|decided| match capability {
                PermissionCapability::Build => decided.build.is_some(),
                PermissionCapability::Skills => decided.skills.is_some(),
            })
    };
    if let Some(ignored) = modules.ignored_builds.as_mut() {
        ignored.retain(|dep_path| !decided(dep_path.as_str(), PermissionCapability::Build));
    }
    if let Some(pending) = modules.pending_skills.as_mut() {
        pending.retain(|dep_path| !decided(dep_path.as_str(), PermissionCapability::Skills));
    }
    if modules.ignored_builds.as_ref().is_some_and(IndexSet::is_empty) {
        modules.ignored_builds = None;
    }
    if modules.pending_skills.as_ref().is_some_and(IndexSet::is_empty) {
        modules.pending_skills = None;
    }
    write_modules_manifest::<Host>(&scan.modules_dir, modules).into_diagnostic()
}

pub(super) fn capability_list(capabilities: &Capabilities) -> String {
    capabilities
        .iter()
        .map(|capability| capability.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
