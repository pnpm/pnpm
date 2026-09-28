//! The lockfile importers `audit` and `audit signatures` cover.

use super::{
    AuditError, EnvLockfile, HashMap, Include, Lockfile, State, lockfile_to_audit_request,
    pick_registry_for_package, signatures,
};
use crate::cli_args::recursive::{
    no_projects_matched_message, notice_workspace_dir, selected_workspace_importer_ids,
    selectors_narrow_the_run,
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
        let workspace_dir = notice_workspace_dir(state.config, state.project_dir());
        println!("{}", no_projects_matched_message(workspace_dir));
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
    let env_lockfile = EnvLockfile::read(lockfile_dir)
        .map_err(|err| miette::Report::new(err).wrap_err("load the env lockfile"))?;
    let audit_request = lockfile_to_audit_request(lockfile, env_lockfile.as_ref(), include);
    let registries: HashMap<String, String> = state.config
        .resolved_registries()
        .into_iter()
        .collect();
    Ok(Some(
        audit_request.request
            .iter()
            .flat_map(|(name, versions)| {
                let registry = pick_registry_for_package(&registries, name, None);
                versions
                    .iter()
                    .map(move |version| signatures::SignaturePackage {
                        name: name.clone(),
                        registry: registry.clone(),
                        version: version.clone(),
                    })
            })
            .collect(),
    ))
}
