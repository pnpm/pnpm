use crate::{
    cargo_deps, cli_args::add::AddArgs, ecosystem_install::InstallContext,
    package_specifier::EcosystemPackageSpecifier, python,
};
use pnpm_install_coordinator::InstallPlan;
use std::path::PathBuf;

pub(crate) async fn plan<Reporter: pnpm_reporter::Reporter + 'static>(
    context: InstallContext,
    root: PathBuf,
    packages: Vec<EcosystemPackageSpecifier>,
    args: &AddArgs,
    has_node_packages: bool,
) -> miette::Result<InstallPlan<'static>> {
    if context.config.recursive {
        return Err(miette::miette!(
            "crate: and pypi: dependencies cannot yet be added through a recursive or filtered selection"
        ));
    }
    if args.save_catalog || args.save_catalog_name.is_some() {
        return Err(miette::miette!("ecosystem dependencies cannot be saved to an npm catalog"));
    }
    let mut crates = Vec::new();
    let mut requirements = Vec::new();
    for package in packages {
        match package {
            EcosystemPackageSpecifier::Cargo(package) => crates.push(package),
            EcosystemPackageSpecifier::Python(requirement) => requirements.push(requirement),
        }
    }
    let mut tasks = Vec::new();
    let mut cargo_transaction_root = None;
    if !crates.is_empty() {
        let (cargo_root, task) = cargo_deps::add::plan::<Reporter>(
            context.clone(),
            root.join("Cargo.toml"),
            cargo_deps::add::AddOptions {
                packages: crates,
                dependency_kind: args
                    .dependency_options
                    .cargo_dependency_kind(has_node_packages)?,
                save_exact: args.save_exact,
                save_prefix: args.save_prefix.clone(),
            },
        )
        .await?;
        cargo_transaction_root = Some(cargo_root);
        tasks.push(task);
    }
    if !requirements.is_empty() {
        tasks.push(python::plan_add::<Reporter>(
            context.clone(),
            &root,
            requirements,
            args.dependency_options.python_development()?,
            args.save_exact,
            args.save_prefix.clone(),
        )?);
    }
    let mut plan = InstallPlan::new(
        context.config.workspace_dir.clone().or(cargo_transaction_root).unwrap_or(root),
    );
    for task in tasks {
        plan = plan.with_task(task);
    }
    Ok(plan)
}
