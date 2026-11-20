//! The lockfile importers `audit` and `audit signatures` cover.

use super::{
    AuditError, AuditGraph, Config, EnvLockfile, HashMap, Include, Lockfile, PackageKey,
    PackageMetadata, State, signatures, walk_reachable,
};
use crate::cli_args::recursive::{
    print_no_projects_matched, selected_workspace_importer_ids, selectors_narrow_the_run,
};
use std::{borrow::Cow, collections::BTreeSet};

/// The lockfile narrowed to the importers of the projects that `--filter`,
/// `--filter-prod`, or `--workspace-root` selected, or the whole lockfile
/// when no selector narrows the run. `None` when the selectors matched no
/// project, after printing pnpm's notice for it. A selected project without
/// an importer entry is an error: auditing the rest would report it clean.
fn select_audited_importers<'lockfile>(
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

/// Load the main lockfile narrowed to selected workspace importers alongside
/// the optional `pnpm-lock.yaml` environment document. `None` when workspace
/// selectors matched no project.
pub(super) fn load_audited_lockfiles<'lockfile>(
    state: &'lockfile State,
    lockfile_dir: &std::path::Path,
) -> miette::Result<Option<(Cow<'lockfile, Lockfile>, Option<EnvLockfile>)>> {
    let lockfile = state.lockfile
        .get()
        .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
    let Some(lockfile) = lockfile else {
        return Err(AuditError::NoLockfile.into());
    };
    let Some(lockfile) = select_audited_importers(state, lockfile)? else {
        return Ok(None);
    };
    let env_lockfile = read_audit_env_lockfile(lockfile_dir)?;
    Ok(Some((lockfile, env_lockfile)))
}

/// Every installed registry package version the lockfile and env lockfile
/// record, with the registry that serves it. `None` when the selectors matched
/// no project.
pub(super) fn signature_packages(
    state: &State,
    include: Include,
    lockfile_dir: &std::path::Path,
) -> miette::Result<Option<Vec<signatures::SignaturePackage>>> {
    let Some((lockfile, env_lockfile)) = load_audited_lockfiles(state, lockfile_dir)? else {
        return Ok(None);
    };
    let lockfile = lockfile.as_ref();
    let registries = state.config.registry_context();
    let mut packages = BTreeSet::new();
    let main = AuditGraph::main(lockfile, include.peer_edges);
    collect_graph_signature_packages(
        &main,
        include,
        lockfile.integrity_target(),
        &registries,
        &mut packages,
    )?;
    if let Some(env_lockfile) = env_lockfile.as_ref() {
        let env = AuditGraph::env(env_lockfile);
        collect_graph_signature_packages(
            &env,
            include,
            pnpm_lockfile::IntegrityTarget::Resource,
            &registries,
            &mut packages,
        )?;
    }
    Ok(Some(packages.into_iter().collect()))
}

fn collect_graph_signature_packages(
    graph: &AuditGraph<'_>,
    include: Include,
    integrity_target: pnpm_lockfile::IntegrityTarget,
    registries: &pnpm_lockfile::RegistryContext,
    out: &mut BTreeSet<signatures::SignaturePackage>,
) -> Result<(), AuditError> {
    for key in walk_reachable(graph, include, include.optional_dependencies) {
        if let Some(package) =
            signature_package_for_key(&key, graph.packages, integrity_target, registries)?
        {
            out.insert(package);
        }
    }
    Ok(())
}

fn signature_package_for_key(
    key: &PackageKey,
    packages: &HashMap<PackageKey, PackageMetadata>,
    integrity_target: pnpm_lockfile::IntegrityTarget,
    registries: &pnpm_lockfile::RegistryContext,
) -> Result<Option<signatures::SignaturePackage>, AuditError> {
    if key.suffix.prefix() == pnpm_lockfile::Prefix::Runtime {
        return Ok(None);
    }
    let Some(version) = key.suffix.registry_version() else {
        return Ok(None);
    };
    let metadata_key = key.without_peer();
    let Some(registry) = registries.registry_of_key(&metadata_key) else {
        let (registry_name, _) = metadata_key.suffix
            .registry_qualified()
            .expect("only a registry-qualified key can reference an undeclared prefix");
        return Err(AuditError::MissingNamedRegistry {
            package_key: metadata_key.to_string(),
            registry_name: registry_name.to_string(),
        });
    };
    let (integrity, pins_tar) = match packages.get(&metadata_key) {
        Some(package) => (
            package.resolution.integrity().map(ToString::to_string),
            package.resolution.integrity_target(integrity_target)
                == pnpm_lockfile::IntegrityTarget::Tar,
        ),
        None => (None, false),
    };
    Ok(Some(signatures::SignaturePackage {
        name: metadata_key.name.to_string(),
        version: version.to_string(),
        registry: registry.to_string(),
        package_id: metadata_key.pkg_id(),
        integrity,
        pins_tar,
    }))
}

fn read_audit_env_lockfile(lockfile_dir: &std::path::Path) -> miette::Result<Option<EnvLockfile>> {
    EnvLockfile::read(lockfile_dir)
        .map_err(|err| miette::Report::new(err).wrap_err("load the env lockfile"))
}

/// The registry each installed package name resolved from when every locked
/// registry occurrence of that name agrees on one registry.
pub(super) fn locked_package_registries(
    lockfile: &Lockfile,
    env_lockfile: Option<&EnvLockfile>,
    include: Include,
    config: &Config,
) -> HashMap<String, String> {
    let registries = config.registry_context();
    let mut seen: HashMap<String, Option<&str>> = HashMap::new();
    let main = AuditGraph::main(lockfile, include.peer_edges);
    record_graph_registries(&main, include, &registries, &mut seen);
    if let Some(env_lockfile) = env_lockfile {
        let env = AuditGraph::env(env_lockfile);
        record_graph_registries(&env, include, &registries, &mut seen);
    }
    seen.into_iter()
        .filter_map(|(name, registry)| Some((name, registry?.to_string())))
        .collect()
}

fn record_graph_registries<'ctx>(
    graph: &AuditGraph<'_>,
    include: Include,
    registries: &'ctx pnpm_lockfile::RegistryContext,
    seen: &mut HashMap<String, Option<&'ctx str>>,
) {
    for key in walk_reachable(graph, include, include.optional_dependencies) {
        if key.suffix.prefix() == pnpm_lockfile::Prefix::Runtime
            || key.suffix.registry_version().is_none()
        {
            continue;
        }
        let Some(registry) = registries.registry_of_key(&key.without_peer()) else {
            continue;
        };
        seen.entry(key.name.to_string())
            .and_modify(|current| merge_package_registry(current, registry))
            .or_insert(Some(registry));
    }
}

fn merge_package_registry<'ctx>(current: &mut Option<&'ctx str>, registry: &'ctx str) {
    if let Some(existing) = *current
        && !pnpm_lockfile::same_registry(existing, registry)
    {
        *current = None;
    }
}
