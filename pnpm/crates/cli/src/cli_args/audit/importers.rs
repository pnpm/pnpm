//! The lockfile importers `audit` and `audit signatures` cover.

use super::{
    AuditError, EnvLockfile, HashMap, Include, Lockfile, State, lockfile_to_audit_request,
    pick_registry_for_package, signatures,
};
use crate::cli_args::recursive::{
    print_no_projects_matched, selected_workspace_importer_ids, selectors_narrow_the_run,
};
use std::borrow::Cow;

/// The lockfile narrowed to the importers of the projects that `--filter`,
/// `--filter-prod`, or `--workspace-root` selected, or the whole lockfile
/// when no selector narrows the run. `None` when the selectors matched no
/// project, after printing pnpm's notice for it. A selected project without
/// an importer entry is an error: auditing the rest would report it clean.
pub(super) fn select_audited_importers<'lockfile>(
    state: &State,
    lockfile: &'lockfile Lockfile,
) -> miette::Result<Option<Cow<'lockfile, Lockfile>>> {
    if !selectors_narrow_the_run(state.config) {
        lockfile.verify_importer_snapshot_links()?;
        return Ok(Some(Cow::Borrowed(lockfile)));
    }
    let selected =
        selected_workspace_importer_ids(state.config, state.project_dir(), state.lockfile_dir())?;
    if selected.is_empty() {
        print_no_projects_matched(state.config, state.project_dir());
        return Ok(None);
    }
    let mut missing: Vec<&str> = selected
        .iter()
        .map(String::as_str)
        .filter(|importer_id| !lockfile.importers.contains_key(*importer_id))
        .collect();
    if !missing.is_empty() {
        missing.sort_unstable();
        return Err(AuditError::MissingImporters { importer_ids: missing.join(", ") }.into());
    }
    let mut narrowed = lockfile.clone();
    narrowed.importers.retain(|importer_id, _| selected.contains(importer_id));
    narrowed.verify_importer_snapshot_links()?;
    Ok(Some(Cow::Owned(narrowed)))
}

/// Every installed package version the lockfile and env lockfile record,
/// with the registry that serves it. `None` when the selectors matched no
/// project.
pub(super) fn signature_packages(
    state: &State,
    include: Include,
    lockfile_dir: &std::path::Path,
) -> miette::Result<Option<Vec<signatures::SignaturePackage>>> {
    let lockfile = state.lockfile
        .get()
        .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
    let Some(lockfile) = lockfile else {
        return Err(AuditError::NoLockfile.into());
    };
    let Some(lockfile) = select_audited_importers(state, lockfile)? else {
        return Ok(None);
    };
    let lockfile = lockfile.as_ref();
    let env_lockfile = read_audit_env_lockfile(lockfile_dir)?;
    let audit_request = lockfile_to_audit_request(lockfile, env_lockfile.as_ref(), include);
    let env_lockfile = env_lockfile.as_ref();
    let registries = resolved_registries(state);
    Ok(Some(
        audit_request.request
            .iter()
            .flat_map(|(name, versions)| {
                let registry = pick_registry_for_package(&registries, name, None);
                versions
                    .iter()
                    .flat_map(move |version| {
                        let name = name.clone();
                        let version = version.clone();
                        let registry = registry.clone();
                        signature_integrities(&format!("{name}@{version}"), lockfile, env_lockfile)
                            .into_iter()
                            .map(move |(integrity, pins_tar)| signatures::SignaturePackage {
                                integrity,
                                pins_tar,
                                name: name.clone(),
                                registry: registry.clone(),
                                version: version.clone(),
                            })
                    })
            })
            .collect(),
    ))
}

fn resolved_registries(state: &State) -> HashMap<String, String> {
    state.config
        .resolved_registries()
        .into_iter()
        .collect()
}

fn read_audit_env_lockfile(lockfile_dir: &std::path::Path) -> miette::Result<Option<EnvLockfile>> {
    EnvLockfile::read(lockfile_dir)
        .map_err(|err| miette::Report::new(err).wrap_err("load the env lockfile"))
}

fn signature_integrities(
    package_id: &str,
    lockfile: &Lockfile,
    env_lockfile: Option<&EnvLockfile>,
) -> Vec<(Option<String>, bool)> {
    let Ok(key) = package_id.parse::<pnpm_lockfile::PackageKey>() else {
        return vec![(None, false)];
    };
    let pins_tar = |package: &pnpm_lockfile::PackageMetadata, target| {
        package.resolution.integrity_target(target) == pnpm_lockfile::IntegrityTarget::Tar
    };
    let mut integrities: Vec<(Option<String>, bool)> = lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&key))
        .map(|package| (package, pins_tar(package, lockfile.integrity_target())))
        .into_iter()
        .chain(
            env_lockfile
                .and_then(|env| env.packages.get(&key))
                .map(|package| {
                    (package, pins_tar(package, pnpm_lockfile::IntegrityTarget::Resource))
                }),
        )
        .map(|(package, pins_tar)| {
            (package.resolution.integrity().map(ToString::to_string), pins_tar)
        })
        .collect();
    if integrities.is_empty() {
        integrities.push((None, false));
    }
    integrities.sort_unstable();
    integrities.dedup();
    integrities
}
