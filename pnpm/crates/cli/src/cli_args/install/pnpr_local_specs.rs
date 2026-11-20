use super::{
    Catalogs,
    DependencyGroup,
    Display,
    Error,
    Lockfile,
    PnprLink,
    ResolveProject,
    pnpr_config_validation::validate_pnpr_config,
};
use pnpm_local_spec::is_host_filesystem_specifier;

/// The install declares a local filesystem dependency (`file:`, `link:`, a bare
/// local path, or an escaping `workspace:` path) that a remote `pnpr` server
/// cannot evaluate against the client's disk.
#[derive(Debug, Display, Error)]
#[display(r#""{spec}""#)]
pub(crate) struct PnprLocalDependencyUnsupported {
    pub(crate) spec: String,
}

impl miette::Diagnostic for PnprLocalDependencyUnsupported {}

pub(super) fn check_pnpr_project_specs_and_config(
    config: &pnpm_config::Config,
    projects: &[ResolveProject],
    catalogs: Option<&Catalogs>,
    link: &PnprLink<'_>,
) -> miette::Result<()> {
    check_no_local_specs_for_pnpr(config, projects, catalogs)?;
    if !link.lockfile.ignore_manifest_check {
        validate_pnpr_config(config, link.lockfile.frozen)?;
    }
    Ok(())
}

pub(super) fn check_pnpr_lockfile_specs_and_config(
    config: &pnpm_config::Config,
    lockfile: Option<&Lockfile>,
    link: &PnprLink<'_>,
) -> miette::Result<()> {
    if link.lockfile.ignore_manifest_check {
        check_no_local_lockfile_specs_for_pnpr(config, lockfile)?;
        validate_pnpr_config(config, link.lockfile.frozen)?;
    }
    Ok(())
}

pub(super) fn check_no_local_specs_for_pnpr(
    config: &pnpm_config::Config,
    projects: &[ResolveProject],
    catalogs: Option<&Catalogs>,
) -> miette::Result<()> {
    if let Some(spec) = first_local_spec_for_pnpr(config, projects, catalogs) {
        return Err(miette::Report::new(PnprLocalDependencyUnsupported { spec: spec.to_owned() }));
    }
    Ok(())
}

pub(super) fn check_no_local_lockfile_specs_for_pnpr(
    config: &pnpm_config::Config,
    lockfile: Option<&Lockfile>,
) -> miette::Result<()> {
    let Some(lockfile) = lockfile else {
        return Ok(());
    };
    let is_declared_prefix = |prefix: &str| config.registries_by_prefix.contains_key(prefix);
    for (importer_id, snapshot) in &lockfile.importers {
        let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
        if let Some((_, dep)) = snapshot
            .dependencies_by_groups(groups)
            .find(|(_, dep)| {
                is_host_filesystem_specifier(&dep.specifier, importer_id, is_declared_prefix)
            })
        {
            return Err(miette::Report::new(PnprLocalDependencyUnsupported {
                spec: dep.specifier.clone(),
            }));
        }
    }
    Ok(())
}

pub(super) fn first_local_spec_for_pnpr<'a>(
    config: &'a pnpm_config::Config,
    projects: &'a [ResolveProject],
    catalogs: Option<&'a Catalogs>,
) -> Option<&'a str> {
    let is_declared_prefix = |prefix: &str| config.registries_by_prefix.contains_key(prefix);
    first_local_project_spec(projects, &is_declared_prefix)
        .or_else(|| first_local_catalog_spec(catalogs, &is_declared_prefix))
        .or_else(|| first_local_extension_spec(config, &is_declared_prefix))
        .or_else(|| first_local_override_spec(config, &is_declared_prefix))
}

fn first_local_project_spec<'a>(
    projects: &'a [ResolveProject],
    is_declared_prefix: &impl Fn(&str) -> bool,
) -> Option<&'a str> {
    for project in projects {
        for dependencies in [
            &project.dependencies,
            &project.dev_dependencies,
            &project.optional_dependencies,
            &project.peer_dependencies,
        ] {
            if let Some(spec) = dependencies
                .values()
                .map(String::as_str)
                .find(|spec| is_host_filesystem_specifier(spec, &project.dir, is_declared_prefix))
            {
                return Some(spec);
            }
        }
    }
    None
}

fn first_local_catalog_spec<'a>(
    catalogs: Option<&'a Catalogs>,
    is_declared_prefix: &impl Fn(&str) -> bool,
) -> Option<&'a str> {
    catalogs?
        .values()
        .flat_map(|catalog| catalog.values())
        .map(String::as_str)
        .find(|spec| is_host_filesystem_specifier(spec, ".", is_declared_prefix))
}

fn first_local_extension_spec<'a>(
    config: &'a pnpm_config::Config,
    is_declared_prefix: &impl Fn(&str) -> bool,
) -> Option<&'a str> {
    let extensions = config.package_extensions.as_ref()?;
    for extension in extensions.values() {
        for dependencies in [
            extension.dependencies.as_ref(),
            extension.optional_dependencies.as_ref(),
            extension.peer_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(spec) = dependencies
                .values()
                .map(String::as_str)
                .find(|spec| is_host_filesystem_specifier(spec, ".", is_declared_prefix))
            {
                return Some(spec);
            }
        }
    }
    None
}

fn first_local_override_spec<'a>(
    config: &'a pnpm_config::Config,
    is_declared_prefix: &impl Fn(&str) -> bool,
) -> Option<&'a str> {
    config.overrides
        .as_ref()?
        .values()
        .map(String::as_str)
        .find(|spec| is_host_filesystem_specifier(spec, ".", is_declared_prefix))
}
