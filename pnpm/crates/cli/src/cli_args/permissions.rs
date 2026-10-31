use crate::{
    State,
    cli_args::{
        approve_builds::{
            ApproveBuildsArgs, config_with_install_approvals, confirm, partition_params,
            prompt_for_choices,
        },
        ignored_builds::{IgnoredBuildsScan, get_automatically_ignored_builds},
        rebuild::{RebuildSelection, run_rebuild},
    },
    confirm_prompt::stdin_is_terminal,
};
use clap::{Args, Subcommand};
use indexmap::IndexSet;
use miette::{Context, IntoDiagnostic, Report};
use pnpm_config::{Config, PermissionCapability, SkillsPermission, is_ci};
use pnpm_modules_yaml::{Host, write_modules_manifest};
use pnpm_package_manager::{
    allow_build_key_from_ignored_build, resync_installed_agent_skills, short_package_label,
    skill_approval_options,
};
use pnpm_reporter::{Reporter, emit_global_warning};
use pnpm_workspace_manifest_writer::{PermissionValue, set_permissions, set_skills_dirs};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

/// Review what dependencies may do: run build scripts (`build`) and provide
/// agent skills (`skills`). With no subcommand, lists the decisions and what
/// awaits approval.
#[derive(Debug, Args)]
pub struct PermissionsArgs {
    #[clap(subcommand)]
    pub command: Option<PermissionsCommand>,
}

#[derive(Debug, Subcommand)]
pub enum PermissionsCommand {
    /// Approve or deny what the dependencies awaiting approval may do.
    Approve(ApproveArgs),
    /// List the decisions in pnpm-workspace.yaml and what awaits approval.
    #[clap(visible_alias = "ls")]
    List,
}

/// Approve or deny what dependencies may do: run build scripts and provide
/// agent skills.
#[derive(Debug, Args)]
pub struct ApproveArgs {
    /// Packages to approve (`<pkg>`) or deny (`!<pkg>`). A package is
    /// approved or denied for everything it requests. With no packages, the
    /// packages awaiting approval are chosen interactively, and for a
    /// package whose skills are approved, which of its skills.
    pub packages: Vec<String>,

    /// Approve everything awaiting approval without interactive prompts.
    #[clap(long)]
    pub all: bool,
}

type Capabilities = BTreeSet<PermissionCapability>;

/// Every capability, for a package named before it requested any.
const ALL_CAPABILITIES: [PermissionCapability; 2] =
    [PermissionCapability::Build, PermissionCapability::Skills];

/// What awaits approval, by the key a package is approved under.
pub(crate) struct PendingPermissions {
    pub(crate) by_package: BTreeMap<String, Capabilities>,
    scan: IgnoredBuildsScan,
}

/// Read the pending build scripts and agent skills from `.modules.yaml`.
pub(crate) fn pending_permissions(config: &Config) -> miette::Result<PendingPermissions> {
    let scan = get_automatically_ignored_builds(config)?;
    let mut by_package: BTreeMap<String, Capabilities> = BTreeMap::new();
    for name in scan.names.iter().flatten() {
        by_package
            .entry(name.clone())
            .or_default()
            .insert(PermissionCapability::Build);
    }
    let pending_skills = scan.modules_manifest
        .iter()
        .flat_map(|modules| modules.pending_skills.iter().flatten());
    for dep_path in pending_skills {
        by_package
            .entry(allow_build_key_from_ignored_build(dep_path.as_str()))
            .or_default()
            .insert(PermissionCapability::Skills);
    }
    Ok(PendingPermissions { by_package, scan })
}

/// The decision for every package a run decides: the capabilities it asked
/// for, each in the shape `permissions` records.
#[derive(Debug, PartialEq, Default)]
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

impl ApproveArgs {
    /// Decide, write the decisions to `pnpm-workspace.yaml`, clear them
    /// from `.modules.yaml`, link the approved agent skills, and rebuild the
    /// packages approved to build.
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir: &Path,
        config: &'static Config,
        manifest_path: &Path,
    ) -> miette::Result<()> {
        ApproveBuildsArgs { packages: self.packages.clone(), all: self.all, global: false }
            .validate()?;
        let pending = pending_permissions(config)?;
        if pending.by_package.is_empty() && self.packages.is_empty() {
            pnpm_diagnostics::println!("There are no packages awaiting approval");
            return Ok(());
        }
        let manifest_dir = manifest_path.parent().unwrap_or(dir);
        let Some(approval) = self.decide::<Reporter>(&pending.by_package, config, manifest_dir)?
        else {
            return Ok(());
        };
        let settings_dir = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        write_decisions(&settings_dir, &approval.decisions)?;
        if !approval.skills_dirs.is_empty() {
            set_skills_dirs(&settings_dir, &approval.skills_dirs).into_diagnostic()?;
        }
        clear_decided(pending.scan, &approval.decisions)?;

        let approved_config = config_with_install_approvals(config, &settings_dir)?;
        if approval.decisions.values().any(|decided| decided.skills.is_some()) {
            resync_installed_agent_skills(approved_config, manifest_dir)?;
        }
        let build_packages = pending_builds_approved(&approval.decisions, &pending.by_package);
        if build_packages.is_empty() {
            return Ok(());
        }
        let rebuild_state = State::init(manifest_path.to_path_buf(), approved_config, true)
            .wrap_err("initialize the rebuild state")?;
        let selection = RebuildSelection { names: Some(build_packages), projects: Vec::new() };
        Box::pin(run_rebuild::<Reporter>(&rebuild_state, selection, None)).await
    }

    /// The decisions of this run. When packages are named, every capability
    /// they request (or ever will) gets the verdicts they asked for. When
    /// `--all` is set, every pending package is approved for everything it
    /// requests. Otherwise the run prompts, and for a package whose skills
    /// are being approved, prompts for the skills.
    fn decide<Reporter: self::Reporter>(
        &self,
        pending: &BTreeMap<String, Capabilities>,
        config: &'static Config,
        manifest_dir: &Path,
    ) -> miette::Result<Option<Approval>> {
        if !self.packages.is_empty() {
            return Ok(Some(Approval {
                decisions: named_decisions::<Reporter>(&self.packages, pending)?,
                skills_dirs: Vec::new(),
            }));
        }
        if self.all {
            return Ok(Some(Approval {
                decisions: decisions_by_approval(
                    &pending.keys().cloned().collect(),
                    pending,
                    &BTreeMap::new(),
                ),
                skills_dirs: Vec::new(),
            }));
        }
        let choices: Vec<(String, String)> = pending
            .iter()
            .map(|(pkg, capabilities)| {
                (
                    pkg.clone(),
                    format!("{}  {}", short_package_label(pkg), capability_list(capabilities)),
                )
            })
            .collect();
        let Some(selected) = prompt_for_choices("Choose which packages to approve", &choices)?
        else {
            return Ok(None);
        };
        let approved: BTreeSet<String> = pending
            .keys()
            .filter(|pkg| selected.contains(pkg))
            .cloned()
            .collect();
        let Some(review) = review_skills(config, manifest_dir, &approved, pending)? else {
            return Ok(None);
        };
        if !approved.is_empty() && !confirm(&confirmation(&approved, pending))? {
            return Ok(None);
        }
        Ok(Some(Approval {
            decisions: decisions_by_approval(&approved, pending, &review.skills_by_package),
            skills_dirs: review.dirs,
        }))
    }
}

/// The interactive skills review of an approval: the first-run
/// `skills.dirs` prompt, and one skill checkbox per approved package.
#[derive(Default)]
struct SkillsReview {
    /// The `skills` decision per package, overriding the all-or-nothing one
    /// the package selection would write.
    skills_by_package: BTreeMap<String, SkillsPermission>,
    /// The `skills.dirs` answer, or empty when the user picked none —
    /// recording nothing keeps the detected directories.
    dirs: Vec<String>,
}

fn review_skills(
    config: &'static Config,
    manifest_dir: &Path,
    approved: &BTreeSet<String>,
    pending: &BTreeMap<String, Capabilities>,
) -> miette::Result<Option<SkillsReview>> {
    let skills_pending: Vec<&String> = approved
        .iter()
        .filter(|pkg| {
            pending
                .get(*pkg)
                .is_some_and(|capabilities| capabilities.contains(&PermissionCapability::Skills))
        })
        .collect();
    if skills_pending.is_empty() {
        return Ok(Some(SkillsReview::default()));
    }

    let options = skill_approval_options(config, manifest_dir)
        .map_err(Report::new)
        .wrap_err("find the skills of the packages awaiting approval")?;

    let mut skills_by_package: BTreeMap<String, SkillsPermission> = BTreeMap::new();
    for pkg in skills_pending {
        let skills = options.skills_by_key
            .get(pkg.as_str())
            .cloned()
            .unwrap_or_default();
        let decision = if skills.is_empty() {
            // Nothing discovered in the installed copy: approving the
            // package covers whatever it ships next.
            SkillsPermission::All(true)
        } else {
            let choices: Vec<(String, String)> = skills
                .iter()
                .map(|skill| (skill.clone(), skill.clone()))
                .collect();
            let Some(selected) = prompt_for_choices(
                &format!("Choose which of {}'s agent skills to link", short_package_label(pkg)),
                &choices,
            )?
            else {
                return Ok(None);
            };
            if selected.is_empty() {
                SkillsPermission::All(false)
            } else {
                SkillsPermission::Chosen(selected)
            }
        };
        skills_by_package.insert(pkg.clone(), decision);
    }

    let mut dirs: Vec<String> = Vec::new();
    if config.skills_dirs.is_none()
        && skills_by_package.values().any(SkillsPermission::links_any)
        && !options.detected_dirs.is_empty()
        && can_prompt()
    {
        let choices: Vec<(String, String)> = options.detected_dirs
            .iter()
            .map(|dir| (dir.clone(), dir.clone()))
            .collect();
        let Some(selected) =
            prompt_for_choices("Choose where the approved agent skills are linked", &choices)?
        else {
            return Ok(None);
        };
        dirs = selected;
    }

    Ok(Some(SkillsReview { skills_by_package, dirs }))
}

fn write_decisions(settings_dir: &Path, decisions: &Decisions) -> miette::Result<()> {
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
fn pending_builds_approved(
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

fn confirmation(approved: &BTreeSet<String>, pending: &BTreeMap<String, Capabilities>) -> String {
    let packages = approved
        .iter()
        .map(|pkg| format!("{} ({})", short_package_label(pkg), capability_list(&pending[pkg])))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Approve {packages}?")
}

/// The decisions of `<pkg>` / `!<pkg>` arguments. A package that is not
/// awaiting approval is decided for every capability.
fn named_decisions<Reporter: self::Reporter>(
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

/// A decision for every pending capability of the packages named in
/// `approved`; the packages left out are denied, so they are not offered
/// again. A package the skills review decided keeps its decision — the
/// all-or-nothing one is written only when the review did not ask.
fn decisions_by_approval(
    approved: &BTreeSet<String>,
    pending: &BTreeMap<String, Capabilities>,
    skills_by_package: &BTreeMap<String, SkillsPermission>,
) -> Decisions {
    pending
        .iter()
        .map(|(pkg, capabilities)| {
            let value = approved.contains(pkg);
            (
                pkg.clone(),
                PackageDecisions {
                    build: capabilities.contains(&PermissionCapability::Build).then_some(value),
                    skills: skills_by_package
                        .get(pkg)
                        .cloned()
                        .or_else(|| {
                            capabilities
                                .contains(&PermissionCapability::Skills)
                                .then_some(SkillsPermission::All(value))
                        }),
                },
            )
        })
        .collect()
}

/// Drop the decided entries from `.modules.yaml`'s pending build scripts
/// and agent skills.
fn clear_decided(scan: IgnoredBuildsScan, decisions: &Decisions) -> miette::Result<()> {
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

fn capability_list(capabilities: &Capabilities) -> String {
    capabilities
        .iter()
        .map(|capability| capability.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether the current run can prompt. Under CI, or when the input is not a
/// terminal, a prompt is skipped and answered another time.
fn can_prompt() -> bool {
    !is_ci() && stdin_is_terminal().unwrap_or(false)
}

/// The decisions in effect, and what awaits approval, one package per line.
pub(crate) fn render_permissions(config: &Config) -> miette::Result<String> {
    let mut granted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut denied: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (pkg, &allowed) in &config.allow_builds {
        let section = if allowed { &mut granted } else { &mut denied };
        section
            .entry(short_package_label(pkg))
            .or_default()
            .push("build".to_string());
    }
    for (pkg, decision) in &config.allow_skills {
        let label = match decision {
            SkillsPermission::All(_) => "skills".to_string(),
            SkillsPermission::Chosen(chosen) => format!("skills ({})", chosen.join(", ")),
        };
        let section = if decision.links_any() { &mut granted } else { &mut denied };
        section
            .entry(short_package_label(pkg))
            .or_default()
            .push(label);
    }
    let pending = pending_permissions(config)?.by_package;
    let pending: BTreeMap<String, Vec<String>> = pending
        .iter()
        .map(|(pkg, capabilities)| {
            (
                short_package_label(pkg),
                capabilities
                    .iter()
                    .map(|capability| capability.as_str().to_string())
                    .collect(),
            )
        })
        .collect();
    let mut output = String::new();
    for (title, section) in
        [("Granted", &granted), ("Denied", &denied), ("Awaiting approval", &pending)]
    {
        render_section(&mut output, title, section);
    }
    if output.is_empty() {
        output.push_str("No packages have been approved, denied, or are awaiting approval\n");
    } else if !pending.is_empty() {
        output.push_str("\nRun \"pnpm approve\" to review the packages awaiting approval.\n");
    }
    Ok(output)
}

fn render_section(output: &mut String, title: &str, section: &BTreeMap<String, Vec<String>>) {
    if section.is_empty() {
        return;
    }
    if !output.is_empty() {
        output.push('\n');
    }
    let width = section
        .keys()
        .map(String::len)
        .max()
        .unwrap_or_default();
    output.push_str(title);
    output.push_str(":\n");
    for (pkg, labels) in section {
        writeln!(output, "  {pkg:width$}  {}", labels.join(", ")).expect("write to a String");
    }
}

#[cfg(test)]
mod tests;
