use crate::{
    cargo_deps,
    cli_args::{add::AddArgs, pipelines::WorkspaceScope},
    ecosystem_install::{
        EcosystemPlan, EcosystemWorkspaceInventory, InstallContext, PythonProjects, python,
    },
    package_specifier::EcosystemPackageSpecifier,
};
use pnpm_install_coordinator::{InstallPlan, InstallTask};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(crate) async fn plan<Reporter: pnpm_reporter::Reporter + 'static>(
    context: InstallContext,
    root: PathBuf,
    packages: Vec<EcosystemPackageSpecifier>,
    args: &AddArgs,
    has_node_packages: bool,
    scope: Option<&WorkspaceScope>,
) -> miette::Result<EcosystemPlan> {
    let (crates, requirements) = partition_packages(packages);
    validate_add_options(
        &context,
        args,
        AddedPackages { crates: !crates.is_empty(), node: has_node_packages },
    )?;
    let mut tasks = Vec::new();
    let mut python = PythonProjects::default();
    let mut cargo_transaction_root = None;
    if !crates.is_empty() {
        let (cargo_root, task) =
            cargo_add_task::<Reporter>(context.clone(), &root, crates, (args, has_node_packages))
                .await?;
        cargo_transaction_root = Some(cargo_root);
        tasks.push(task);
    }
    if !requirements.is_empty() {
        let (task, projects) =
            python_add_task::<Reporter>(context.clone(), &root, requirements, (args, scope)).await?;
        python = projects;
        tasks.push(task);
    }
    let mut plan = InstallPlan::new(
        context.config.workspace_dir
            .clone()
            .or(cargo_transaction_root)
            .unwrap_or(root),
    );
    for task in tasks {
        plan = plan.with_task(task);
    }
    Ok(EcosystemPlan { plan, python })
}

/// Which kinds of package an ecosystem add carries beside its `pypi:`
/// requirements. Neither kind can be added to a selection yet.
#[derive(Clone, Copy)]
struct AddedPackages {
    crates: bool,
    node: bool,
}

async fn cargo_add_task<Reporter: pnpm_reporter::Reporter + 'static>(
    context: InstallContext,
    root: &Path,
    packages: Vec<crate::package_specifier::RegistryPackageSpecifier>,
    (args, has_node_packages): (&AddArgs, bool),
) -> miette::Result<(PathBuf, InstallTask<'static>)> {
    cargo_deps::add::plan::<Reporter>(
        context,
        root.join("Cargo.toml"),
        cargo_deps::add::AddOptions {
            packages,
            dependency_kind: args.dependency_options.cargo_dependency_kind(has_node_packages)?,
            save_exact: args.save.exact,
            save_prefix: args.save.prefix.clone(),
        },
    )
    .await
}

/// The Python half of the add, and the projects it was resolved against.
///
/// Without a `--filter` selection the add acts on the project the command
/// was run in, the way the npm add does, and reads that project alone.
async fn python_add_task<Reporter: pnpm_reporter::Reporter + 'static>(
    context: InstallContext,
    root: &Path,
    requirements: Vec<String>,
    (args, scope): (&AddArgs, Option<&WorkspaceScope>),
) -> miette::Result<(InstallTask<'static>, PythonProjects)> {
    let config = context.config;
    // Before the discovery below parses a manifest: an add pnpm refuses
    // must not fail on what it was going to read.
    let options = python_add_options(args, requirements)?;
    options.validate(config)?;
    let (discovery, selected) = if let Some(scope) = scope {
        let workspace_root = config.workspace_dir.clone().unwrap_or_else(|| root.to_path_buf());
        let inventory = EcosystemWorkspaceInventory::new(workspace_root, config);
        let discovery = python::discover(config, &inventory).await?;
        let selected = python::selected_projects(config, root, &discovery, Some(scope))?;
        (discovery, selected)
    } else {
        let project = pnpm_python_installer::writable_project(root)?;
        let discovery =
            pnpm_python_installer::discover(config, vec![project.join("pyproject.toml")]).await?;
        (discovery, BTreeSet::from([project]))
    };
    let projects =
        PythonProjects { discovered: discovery.project_roots().count(), selected: selected.len() };
    let task =
        pnpm_python_installer::plan_add::<Reporter>(context.into(), discovery, selected, options)?;
    Ok((task, projects))
}

fn validate_add_options(
    context: &InstallContext,
    args: &AddArgs,
    added: AddedPackages,
) -> miette::Result<()> {
    if context.config.recursive && added.crates {
        return Err(miette::miette!(
            "crate: dependencies cannot yet be added through a recursive or filtered selection"
        ));
    }
    if context.config.recursive && added.node {
        return Err(miette::miette!(
            "an npm dependency cannot yet be added alongside a pypi: dependency through a \
             recursive or filtered selection"
        ));
    }
    if args.save.catalog || args.save.catalog_name.is_some() {
        return Err(miette::miette!("ecosystem dependencies cannot be saved to an npm catalog"));
    }
    Ok(())
}

fn partition_packages(
    packages: Vec<EcosystemPackageSpecifier>,
) -> (Vec<crate::package_specifier::RegistryPackageSpecifier>, Vec<String>) {
    let mut crates = Vec::new();
    let mut requirements = Vec::new();
    for package in packages {
        match package {
            EcosystemPackageSpecifier::Cargo(package) => crates.push(package),
            EcosystemPackageSpecifier::Python(requirement) => requirements.push(requirement),
        }
    }
    (crates, requirements)
}

fn python_add_options(
    args: &AddArgs,
    requirements: Vec<String>,
) -> miette::Result<pnpm_python_installer::AddOptions> {
    Ok(pnpm_python_installer::AddOptions {
        requirements,
        development: args.dependency_options.python_development()?,
        exact: args.save.exact,
        prefix: args.save.prefix.clone(),
    })
}
