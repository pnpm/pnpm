use super::RemoveArgs;
use crate::{
    State,
    cargo_deps,
    cargo_manifest,
    ecosystem_install::{
        EcosystemWorkspaceInventory,
        InstallContext,
        python,
    },
    package_specifier::EcosystemPackageSpecifier,
};
use miette::{
    Result,
    bail,
};
use pnpm_install_coordinator::{
    InstallPlan,
    InstallTask,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_python_installer::PythonDependencyGroup;
use pnpm_reporter::Reporter;
use std::{
    collections::BTreeSet,
    path::{
        Path,
        PathBuf,
    },
};

impl RemoveArgs {
    pub(crate) async fn ecosystem_plan<Rep: Reporter + 'static>(
        &self,
        config: &'static pnpm_config::Config,
        root: &Path,
        dirs: &[PathBuf],
        packages: Vec<EcosystemPackageSpecifier>,
    ) -> Result<InstallPlan<'static>> {
        let mut crates = BTreeSet::new();
        let mut python_names = BTreeSet::new();
        for package in packages {
            match package {
                EcosystemPackageSpecifier::Cargo(spec) => {
                    crates.insert(spec.name);
                }
                EcosystemPackageSpecifier::Python(req) => {
                    python_names.insert(pnpm_python_resolver::parse_requirement(&req)?.name);
                }
                EcosystemPackageSpecifier::RustToolchain(_) => unreachable!(),
            }
        }
        let group = self.dependency_options.save_type();
        let context = InstallContext {
            config,
            http_client: State::new_http_client(config)?,
            lockfile_only: self.lockfile_only,
            frozen_lockfile: false,
        };
        let workspace = config.workspace_dir.as_deref().unwrap_or(root);
        let mut plan = InstallPlan::new(workspace.to_owned());
        if !crates.is_empty() {
            plan = plan.with_task(
                cargo_task::<Rep>(context.clone(), workspace, dirs, crates, group).await?,
            );
        }
        if !python_names.is_empty() {
            plan = plan.with_task(
                python_task::<Rep>(context, workspace, dirs, python_names, group).await?,
            );
        }
        Ok(plan)
    }
}

async fn python_task<Rep: Reporter + 'static>(
    context: InstallContext,
    workspace: &Path,
    dirs: &[PathBuf],
    python_names: BTreeSet<pnpm_python_installer::PackageName>,
    group: Option<DependencyGroup>,
) -> Result<InstallTask<'static>> {
    let projects: Vec<_> = dirs
        .iter()
        .filter(|dir| dir.join("pyproject.toml").is_file())
        .cloned()
        .collect();
    if projects.is_empty() {
        bail!("no pyproject.toml in the selected projects");
    }
    let config = context.config;
    let inventory = EcosystemWorkspaceInventory::new(workspace.to_owned(), config);
    let discovery = python::discover_around_projects(config, &inventory, &projects).await?;
    let group = group.map(|group| match group {
        DependencyGroup::Prod => PythonDependencyGroup::Prod,
        DependencyGroup::Dev => PythonDependencyGroup::Dev,
        DependencyGroup::Optional => PythonDependencyGroup::Optional,
        DependencyGroup::Peer => unreachable!(),
    });
    Ok(pnpm_python_installer::plan_remove::<Rep>(
        context.into(),
        discovery,
        projects.into_iter().collect(),
        python_names,
        group,
    ))
}

async fn cargo_task<Rep: Reporter + 'static>(
    context: InstallContext,
    workspace: &Path,
    dirs: &[PathBuf],
    names: BTreeSet<String>,
    group: Option<DependencyGroup>,
) -> Result<InstallTask<'static>> {
    let manifests: Vec<_> = dirs
        .iter()
        .map(|dir| dir.join("Cargo.toml"))
        .filter(|path| path.is_file())
        .collect();
    if manifests.is_empty() {
        bail!("no Cargo.toml in the selected projects");
    }
    let checkout = cargo_deps::checkout(workspace);
    cargo_deps::provision_toolchain::<Rep>(
        context.config,
        &context.http_client,
        &manifests,
        checkout.as_deref(),
    )
    .await?;
    let mut roots = BTreeSet::new();
    for path in &manifests {
        roots.insert(cargo_deps::workspace_root(path, checkout.as_deref()).await?);
    }
    let metadata = manifests
        .iter()
        .cloned()
        .chain(roots.iter().flat_map(|root| cargo_deps::metadata_paths(root)))
        .collect();
    Ok(InstallTask::new(metadata, async move {
        remove_cargo_declarations(&manifests, &names, group)?;
        cargo_deps::prepare::<Rep>(
            context,
            roots.into_iter().collect(),
            checkout,
            cargo_deps::CargoLockfilePolicy::KeepLocked,
        )
        .await
    }))
}

fn remove_cargo_declarations(
    manifests: &[PathBuf],
    names: &BTreeSet<String>,
    group: Option<DependencyGroup>,
) -> Result<()> {
    let mut removed = BTreeSet::new();
    for path in manifests {
        cargo_manifest::remove::remove_dependencies(path, names, group, &mut removed)?;
    }
    if let Some(name) = names.difference(&removed).next() {
        bail!("cannot remove {name}: not declared in the selected Cargo dependencies");
    }
    Ok(())
}
