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
};
use clap::{Args, Subcommand};
use indexmap::IndexSet;
use miette::{Context, IntoDiagnostic};
use pnpm_config::{Config, PermissionCapability};
use pnpm_modules_yaml::{Host, write_modules_manifest};
use pnpm_package_manager::{allow_build_key_from_ignored_build, resync_installed_agent_skills};
use pnpm_reporter::{Reporter, emit_global_warning};
use pnpm_workspace_manifest_writer::set_permissions;
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
    /// packages awaiting approval are chosen interactively.
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

/// The decision for each package and capability.
pub(crate) type Decisions = BTreeMap<(String, PermissionCapability), bool>;

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
            println!("There are no packages awaiting approval");
            return Ok(());
        }
        let Some(decisions) = self.decide::<Reporter>(&pending.by_package)? else {
            return Ok(());
        };
        let settings_dir = config.workspace_dir.clone().unwrap_or_else(|| dir.to_path_buf());
        write_decisions(&settings_dir, &decisions)?;
        clear_decided(pending.scan, &decisions)?;

        let approved_config = config_with_install_approvals(config, &settings_dir)?;
        if decisions
            .keys()
            .any(|(_, capability)| *capability == PermissionCapability::Skills)
        {
            let manifest_dir = manifest_path.parent().unwrap_or(dir);
            resync_installed_agent_skills(approved_config, manifest_dir)?;
        }
        let build_packages = pending_builds_approved(&decisions, &pending.by_package);
        if build_packages.is_empty() {
            return Ok(());
        }
        let rebuild_state = State::init(manifest_path.to_path_buf(), approved_config, true)
            .wrap_err("initialize the rebuild state")?;
        let selection = RebuildSelection { names: Some(build_packages), projects: Vec::new() };
        Box::pin(run_rebuild::<Reporter>(&rebuild_state, selection, None)).await
    }

    fn decide<Reporter: self::Reporter>(
        &self,
        pending: &BTreeMap<String, Capabilities>,
    ) -> miette::Result<Option<Decisions>> {
        if !self.packages.is_empty() {
            return named_decisions::<Reporter>(&self.packages, pending).map(Some);
        }
        let approved: BTreeSet<&String> = if self.all {
            pending.keys().collect()
        } else {
            let choices: Vec<(String, String)> = pending
                .iter()
                .map(|(pkg, capabilities)| {
                    (pkg.clone(), format!("{pkg}  {}", capability_list(capabilities)))
                })
                .collect();
            let Some(selected) = prompt_for_choices("Choose which packages to approve", &choices)?
            else {
                return Ok(None);
            };
            let approved: BTreeSet<&String> = pending
                .keys()
                .filter(|pkg| selected.contains(pkg))
                .collect();
            if !approved.is_empty() && !confirm(&confirmation(&approved, pending))? {
                return Ok(None);
            }
            approved
        };
        Ok(Some(
            pending
                .iter()
                .flat_map(|(pkg, capabilities)| {
                    let value = approved.contains(pkg);
                    capabilities
                        .iter()
                        .map(move |capability| ((pkg.clone(), *capability), value))
                })
                .collect(),
        ))
    }
}

fn write_decisions(settings_dir: &Path, decisions: &Decisions) -> miette::Result<()> {
    let entries = decisions
        .iter()
        .map(|((pkg, capability), &value)| (pkg.as_str(), *capability, value));
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
        .filter(|((pkg, capability), value)| {
            **value
                && *capability == PermissionCapability::Build
                && pending
                    .get(pkg)
                    .is_some_and(|requested| requested.contains(capability))
        })
        .map(|((pkg, _), _)| pkg.clone())
        .collect()
}

fn confirmation(approved: &BTreeSet<&String>, pending: &BTreeMap<String, Capabilities>) -> String {
    let packages = approved
        .iter()
        .map(|pkg| format!("{pkg} ({})", capability_list(&pending[*pkg])))
        .collect::<Vec<_>>()
        .join(", ");
    format!("The next packages will now be approved: {packages}.\nDo you approve?")
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
    let decide = |names: &[String], value: bool| -> Vec<((String, PermissionCapability), bool)> {
        names
            .iter()
            .flat_map(|pkg| {
                let capabilities: Capabilities = pending
                    .get(pkg)
                    .cloned()
                    .unwrap_or_else(|| ALL_CAPABILITIES.into_iter().collect());
                capabilities
                    .into_iter()
                    .map(move |capability| ((pkg.clone(), capability), value))
            })
            .collect()
    };
    Ok(decide(&partition.approved, true)
        .into_iter()
        .chain(decide(&partition.denied, false))
        .collect())
}

/// Drop the decided entries from `.modules.yaml`'s pending build scripts
/// and agent skills.
fn clear_decided(scan: IgnoredBuildsScan, decisions: &Decisions) -> miette::Result<()> {
    let Some(mut modules) = scan.modules_manifest else { return Ok(()) };
    let decided = |dep_path: &str, capability: PermissionCapability| {
        decisions.contains_key(&(allow_build_key_from_ignored_build(dep_path), capability))
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

/// The decisions in effect, and what awaits approval, one package per line.
pub(crate) fn render_permissions(config: &Config) -> miette::Result<String> {
    let mut granted: BTreeMap<&str, Capabilities> = BTreeMap::new();
    let mut denied: BTreeMap<&str, Capabilities> = BTreeMap::new();
    let decided = [
        (PermissionCapability::Build, &config.allow_builds),
        (PermissionCapability::Skills, &config.allow_skills),
    ];
    for (capability, decisions) in decided {
        for (pkg, &allowed) in decisions {
            let section = if allowed { &mut granted } else { &mut denied };
            section
                .entry(pkg.as_str())
                .or_default()
                .insert(capability);
        }
    }
    let pending = pending_permissions(config)?.by_package;
    let pending: BTreeMap<&str, Capabilities> = pending
        .iter()
        .map(|(pkg, capabilities)| (pkg.as_str(), capabilities.clone()))
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

fn render_section(output: &mut String, title: &str, section: &BTreeMap<&str, Capabilities>) {
    if section.is_empty() {
        return;
    }
    if !output.is_empty() {
        output.push('\n');
    }
    let width = section
        .keys()
        .map(|pkg| pkg.len())
        .max()
        .unwrap_or_default();
    output.push_str(title);
    output.push_str(":\n");
    for (pkg, capabilities) in section {
        writeln!(output, "  {pkg:width$}  {}", capability_list(capabilities)).expect(
            "write to a String",
        );
    }
}

#[cfg(test)]
mod tests;
