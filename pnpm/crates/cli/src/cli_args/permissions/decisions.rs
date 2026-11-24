use super::{
    Capabilities,
    IgnoredBuildsScan,
    PermissionCapability,
    disambiguated_labels,
};
use indexmap::IndexSet;
use miette::IntoDiagnostic;
use pnpm_config::SkillsPermission;
use pnpm_modules_yaml::{
    Host,
    write_modules_manifest,
};
use pnpm_package_manager::{
    allow_build_key_from_ignored_build,
    skill_approval_key,
};
use pnpm_package_name::git_repo_allow_build_key_from_dep_path;
use pnpm_workspace_manifest_writer::{
    PermissionValue,
    set_permissions,
};
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
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

pub(super) enum PermissionVerdict {
    Grant,
    Deny,
}

impl PermissionVerdict {
    pub(super) fn for_capabilities(self, capabilities: &Capabilities) -> PackageDecisions {
        let allowed = match self {
            Self::Grant => true,
            Self::Deny => false,
        };
        PackageDecisions {
            build: capabilities.contains(&PermissionCapability::Build).then_some(allowed),
            skills: capabilities
                .contains(&PermissionCapability::Skills)
                .then_some(SkillsPermission::All(allowed)),
        }
    }
}

pub(crate) type Decisions = BTreeMap<String, PackageDecisions>;

pub(crate) fn confirmation(
    approved: &BTreeSet<String>,
    pending: &BTreeMap<String, Capabilities>,
) -> String {
    let labels = disambiguated_labels(pending.keys().map(String::as_str));
    let packages = approved
        .iter()
        .map(|pkg| format!("{} ({})", labels[pkg.as_str()], capability_list(&pending[pkg])))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Approve {packages}?")
}

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
    pending
        .iter()
        .filter(|(_, requested)| requested.contains(&PermissionCapability::Build))
        .filter(|(pkg, _)| {
            matching_decisions(decisions, pkg)
                .filter_map(|decision| decision.build)
                .min()
                .unwrap_or(false)
        })
        .map(|(pkg, _)| pkg.clone())
        .collect()
}

fn matching_decisions<'a>(
    decisions: &'a Decisions,
    key: &str,
) -> impl Iterator<Item = &'a PackageDecisions> {
    let repository =
        git_repo_allow_build_key_from_dep_path(key).and_then(|repo| decisions.get(repo.as_ref()));
    decisions
        .get(key)
        .into_iter()
        .chain(repository)
}

/// The decisions for `--all`: every capability of every pending package is
/// approved.
pub(super) fn decisions_for_all(pending: &BTreeMap<String, Capabilities>) -> Decisions {
    pending
        .iter()
        .map(|(pkg, capabilities)| {
            (pkg.clone(), PermissionVerdict::Grant.for_capabilities(capabilities))
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

fn is_decided(decisions: &Decisions, dep_path: &str, capability: PermissionCapability) -> bool {
    match capability {
        PermissionCapability::Build => {
            matching_decisions(decisions, &allow_build_key_from_ignored_build(dep_path))
                .any(|decided| decided.build.is_some())
        }
        PermissionCapability::Skills => {
            matching_decisions(decisions, &skill_approval_key(dep_path))
                .any(|decided| decided.skills.is_some())
                || decisions
                    .get(&allow_build_key_from_ignored_build(dep_path))
                    .is_some_and(|decided| decided.skills.is_some())
        }
    }
}

/// Drop the decided entries from `.modules.yaml`'s pending build scripts
/// and agent skills.
pub(super) fn clear_decided(scan: IgnoredBuildsScan, decisions: &Decisions) -> miette::Result<()> {
    let Some(mut modules) = scan.modules_manifest else { return Ok(()) };
    if let Some(ignored) = modules.ignored_builds.as_mut() {
        ignored.retain(|dep_path| {
            !is_decided(decisions, dep_path.as_str(), PermissionCapability::Build)
        });
    }
    if let Some(pending) = modules.pending_skills.as_mut() {
        pending.retain(|dep_path| {
            !is_decided(decisions, dep_path.as_str(), PermissionCapability::Skills)
        });
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
