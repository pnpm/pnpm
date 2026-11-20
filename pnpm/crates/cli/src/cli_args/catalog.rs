use crate::{
    cli_args::{
        catalogs::{
            configured_catalogs,
            workspace_manifest_catalogs,
        },
        recursive::discover_workspace_projects,
        reporter::CliReporter,
    },
    config_deps::{
        apply_config_dependency_settings,
        update_config_hook_catalogs,
    },
};
use clap::{
    Args,
    Subcommand,
};
use derive_more::{
    Display,
    Error,
};
use miette::{
    Context,
    Diagnostic,
    IntoDiagnostic,
};
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_types::{
    Catalog,
    Catalogs,
    DEFAULT_CATALOG_NAME,
};
use pnpm_config::Config;
use pnpm_package_manager::stays_out_of_catalogs;
use pnpm_package_manifest::DependencyGroup;
use pnpm_text_sanitize::sanitize_inline;
use pnpm_workspace::Project;
use pnpm_workspace_manifest_writer::{
    UpdateWorkspaceManifestOptions,
    update_workspace_manifest,
};
use serde_json::Value;
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    fmt,
    path::Path,
};

const MIGRATED_GROUPS: [DependencyGroup; 4] =
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional, DependencyGroup::Peer];

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum CatalogError {
    #[display("Catalogs are declared in pnpm-workspace.yaml, but no workspace was found")]
    #[diagnostic(code(ERR_PNPM_CATALOG_NO_WORKSPACE))]
    NoWorkspace,
}

#[derive(Debug, Args)]
pub struct CatalogArgs {
    #[clap(subcommand)]
    pub command: CatalogSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CatalogSubcommand {
    /// Moves the dependency specifiers that all workspace projects agree on
    /// into the default catalog and references them with `catalog:`.
    ///
    /// A dependency declared with different specifiers, or with a specifier
    /// other than the default catalog's entry for it, is listed and left
    /// unchanged. Run `pnpm install` afterwards to update the lockfile.
    Migrate,
}

impl CatalogArgs {
    /// `dir` is the command's working directory. `config` has had the
    /// `updateConfig` hooks applied; `pre_hook_config` is the same
    /// configuration before they ran, which an install starts from.
    pub async fn run(
        self,
        config: &Config,
        pre_hook_config: Config,
        dir: &Path,
    ) -> miette::Result<()> {
        match self.command {
            CatalogSubcommand::Migrate => Box::pin(migrate(config, pre_hook_config, dir)).await,
        }
    }
}

/// The specifiers each dependency is declared with across the workspace,
/// counting only those a catalog can hold.
type DeclaredSpecifiers = BTreeMap<String, BTreeSet<String>>;

#[derive(Debug, Default, PartialEq, Eq)]
struct MigrationPlan {
    /// Dependency name → the specifier that every project declares it
    /// with and that becomes (or already is) its default catalog entry.
    migrated: BTreeMap<String, String>,
    /// The entries to add to the default catalog.
    new_entries: Catalog,
    conflicts: Vec<Conflict>,
    /// Dependencies the projects agree on whose catalog entry an
    /// `updateConfig` hook would drop or change, in the order found.
    changed_by_hooks: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct Conflict {
    name: String,
    declared: BTreeSet<String>,
    catalog_entry: Option<String>,
}

async fn migrate(config: &Config, pre_hook_config: Config, dir: &Path) -> miette::Result<()> {
    let workspace_dir = config.workspace_dir.as_deref().ok_or(CatalogError::NoWorkspace)?;
    let (mut projects, _patterns) = discover_workspace_projects(workspace_dir, config)?;
    let catalogs = configured_catalogs(config)?;
    let mut plan =
        plan_migration(&declared_specifiers(&projects), catalogs.get(DEFAULT_CATALOG_NAME));
    if !plan.new_entries.is_empty() {
        keep_entries_the_hooks_keep(&mut plan, pre_hook_config, dir).await?;
    }

    // The catalog is written before the manifests that reference it, so a
    // failed manifest write leaves an unused entry rather than a dangling
    // `catalog:` reference.
    if !plan.new_entries.is_empty() {
        let updated_catalogs =
            Catalogs::from([(DEFAULT_CATALOG_NAME.to_string(), plan.new_entries.clone())]);
        update_workspace_manifest(
            workspace_dir,
            &UpdateWorkspaceManifestOptions {
                updated_catalogs: Some(&updated_catalogs),
                ..Default::default()
            },
        )
        .into_diagnostic()
        .wrap_err("write the default catalog to pnpm-workspace.yaml")?;
    }
    for project in &mut projects {
        if reference_catalog(project, &plan.migrated) {
            project.manifest
                .save()
                .into_diagnostic()
                .wrap_err_with(|| format!("write {}", project.manifest.path().display()))?;
        }
    }

    pnpm_diagnostics::println!("{}", render_report(&plan));
    Ok(())
}

fn declared_specifiers(projects: &[Project]) -> DeclaredSpecifiers {
    let mut declared = DeclaredSpecifiers::new();
    for project in projects {
        for (name, specifier) in project.manifest.dependencies(MIGRATED_GROUPS) {
            if is_migratable(specifier) {
                declared
                    .entry(name.to_string())
                    .or_default()
                    .insert(specifier.to_string());
            }
        }
    }
    declared
}

/// Whether a declared specifier can move into the default catalog. A
/// `workspace:` specifier stays, because it names a workspace project
/// rather than a version to share.
fn is_migratable(specifier: &str) -> bool {
    parse_catalog_protocol(specifier).is_none()
        && !specifier.starts_with("workspace:")
        && !stays_out_of_catalogs(specifier)
}

fn plan_migration(
    declared: &DeclaredSpecifiers,
    default_catalog: Option<&Catalog>,
) -> MigrationPlan {
    let mut plan = MigrationPlan::default();
    for (name, specifiers) in declared {
        let catalog_entry = default_catalog.and_then(|catalog| catalog.get(name));
        let agreed = match specifiers
            .iter()
            .collect::<Vec<_>>()
            .as_slice()
        {
            [only] => Some(*only),
            _ => None,
        };
        match (agreed, catalog_entry) {
            (Some(specifier), None) => {
                plan.new_entries.insert(name.clone(), specifier.clone());
                plan.migrated.insert(name.clone(), specifier.clone());
            }
            (Some(specifier), Some(entry)) if specifier == entry => {
                plan.migrated.insert(name.clone(), specifier.clone());
            }
            _ => plan.conflicts.push(Conflict {
                name: name.clone(),
                declared: specifiers.clone(),
                catalog_entry: catalog_entry.cloned(),
            }),
        }
    }
    plan
}

/// An `updateConfig` hook's output replaces the workspace catalogs, so a new
/// entry written to `pnpm-workspace.yaml` reaches installs only if the hooks
/// pass it through, and the entries it brings can change what the hooks do
/// with the existing ones. Run the hooks the way an install would, from the
/// configuration before they applied, with the proposed entries added, and
/// hold back every migrated dependency whose entry does not come back as
/// declared. Holding one back changes the hooks' input, so repeat until a
/// run holds back nothing.
async fn keep_entries_the_hooks_keep(
    plan: &mut MigrationPlan,
    mut hook_config: Config,
    dir: &Path,
) -> miette::Result<()> {
    // The first pass installed the config dependencies; an install applies
    // their settings before it runs the hooks.
    apply_config_dependency_settings(&mut hook_config)?;
    let root_dir = hook_config.root_project_manifest_dir(dir).to_path_buf();
    let workspace_catalogs = workspace_manifest_catalogs(&hook_config)?;
    loop {
        let mut seed = workspace_catalogs.clone();
        seed.entry(DEFAULT_CATALOG_NAME.to_string())
            .or_default()
            .extend(plan.new_entries.clone());
        let Some(hook_catalogs) =
            update_config_hook_catalogs::<CliReporter>(&hook_config, &root_dir, &seed).await?
        else {
            return Ok(());
        };
        if !plan.hold_back_entries_changed_by(hook_catalogs.get(DEFAULT_CATALOG_NAME)) {
            return Ok(());
        }
    }
}

impl MigrationPlan {
    /// Hold back each migrated dependency whose entry in `hook_catalog` is
    /// missing or differs from the agreed specifier. Returns whether any was
    /// held back.
    fn hold_back_entries_changed_by(&mut self, hook_catalog: Option<&Catalog>) -> bool {
        let changed: Vec<String> = self.migrated
            .iter()
            .filter(|(name, specifier)| {
                hook_catalog.and_then(|catalog| catalog.get(*name)) != Some(*specifier)
            })
            .map(|(name, _)| name.clone())
            .collect();
        for name in &changed {
            self.new_entries.remove(name);
            self.migrated.remove(name);
        }
        let held_back = !changed.is_empty();
        self.changed_by_hooks.extend(changed);
        held_back
    }
}

/// Replace each of `project`'s specifiers that `migrated` moved into the
/// default catalog with `catalog:`. Returns whether anything changed.
fn reference_catalog(project: &mut Project, migrated: &BTreeMap<String, String>) -> bool {
    let mut changed = false;
    let manifest = project.manifest.value_mut();
    for group in MIGRATED_GROUPS {
        let field: &str = group.into();
        let Some(dependencies) = manifest.get_mut(field).and_then(Value::as_object_mut) else {
            continue;
        };
        for (name, specifier) in dependencies.iter_mut() {
            if migrated
                .get(name)
                .is_some_and(|agreed| specifier.as_str() == Some(agreed))
            {
                *specifier = Value::String("catalog:".to_string());
                changed = true;
            }
        }
    }
    changed
}

fn render_report(plan: &MigrationPlan) -> String {
    let mut report = String::new();
    if !plan.conflicts.is_empty() {
        report.push_str(
            "These dependencies were left unchanged because the projects or the default catalog disagree on their specifier:\n",
        );
        for conflict in &plan.conflicts {
            report.push_str("  ");
            report.push_str(&conflict.to_string());
            report.push('\n');
        }
    }
    report.push_str(&render_changed_by_hooks(&plan.changed_by_hooks));
    report.push_str(&render_summary(plan.migrated.len()));
    report
}

fn render_changed_by_hooks(names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let names = names
        .iter()
        .map(|name| sanitize_inline(name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "These dependencies were left unchanged because an updateConfig hook would not keep their catalog entry as declared: {names}\n",
    )
}

fn render_summary(migrated: usize) -> String {
    match migrated {
        0 => "No dependencies were moved to the default catalog.".to_string(),
        1 => {
            "Moved 1 dependency to the default catalog. Run `pnpm install` to update the lockfile."
                .to_string()
        }
        count => format!(
            "Moved {count} dependencies to the default catalog. Run `pnpm install` to update the lockfile.",
        ),
    }
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The names and specifiers come from the workspace manifests, so
        // control characters are stripped before they reach the terminal.
        let declared = self.declared
            .iter()
            .map(|specifier| sanitize_inline(specifier))
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "{}: {declared}", sanitize_inline(&self.name))?;
        match &self.catalog_entry {
            Some(entry) => write!(f, " (catalog: {})", sanitize_inline(entry)),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
