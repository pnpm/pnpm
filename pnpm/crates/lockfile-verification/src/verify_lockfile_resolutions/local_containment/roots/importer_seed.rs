use std::collections::{
    HashMap,
    HashSet,
};

use pnpm_lockfile::{
    ImporterDepVersion,
    Lockfile,
    PackageMetadata,
    PkgName,
    PkgNameVerPeer,
    ProjectSnapshot,
    ResolvedDependencySpec,
};
use pnpm_resolving_resolver_base::ResolutionPolicyViolation;

use super::{
    super::{
        super::{
            LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
            local_git_shape::is_local_git_package,
        },
        LocalResolutionPolicy,
        is_deploy_entry,
        is_workspace_directory_entry,
        link_violation_with_code,
        package_entries::package_violation,
        policy::{
            extract_local_package_target,
            importer_specifier_declares_local,
            policy_authorizes_importer_dep,
        },
    },
    ImporterScope,
    anchor_importer_link,
    expand_local_snapshot_closure,
    is_snapshot_backed_by_local_resolution,
    reach::Reached,
};

pub(super) struct ImporterSeedCtx<'a> {
    pub(super) lockfile: &'a Lockfile,
    pub(super) policy: &'a LocalResolutionPolicy,
    pub(super) workspace_dirs: &'a HashSet<String>,
    /// Importers a project's `link:` dependency may target whatever its
    /// specifier, as `linkWorkspacePackages`, a `catalog:` entry, or a
    /// dedicated lockfile links a range to another project. Unlike
    /// `workspace_dirs`, this includes projects absent from the current run.
    pub(super) lockfile_importer_dirs: &'a HashSet<String>,
    pub(super) importer_id: &'a str,
}

/// What an importer's own dependencies contribute, before its snapshot
/// closure is walked.
#[derive(Default)]
pub(super) struct ImporterSeedBuffers {
    pub(super) all_seed: Vec<PkgNameVerPeer>,
    local_seed: Vec<PkgNameVerPeer>,
    local_git_seed: Vec<PkgNameVerPeer>,
    local_packages: HashSet<PkgNameVerPeer>,
    local_git_packages: HashSet<PkgNameVerPeer>,
    links_by_alias: HashSet<(String, String)>,
    violations: Vec<ResolutionPolicyViolation>,
}

pub(super) fn seed_importer(
    ctx: &ImporterSeedCtx<'_>,
    importer: &ProjectSnapshot,
) -> ImporterSeedBuffers {
    let mut bufs = ImporterSeedBuffers::default();
    let all_deps = importer.dependencies
        .iter()
        .flatten()
        .chain(importer.dev_dependencies.iter().flatten())
        .chain(importer.optional_dependencies.iter().flatten());
    for (alias, dep_spec) in all_deps {
        seed_single_importer_dep(ctx, alias, dep_spec, &mut bufs);
    }
    bufs
}

pub(super) fn finish_importer_scope(
    lockfile: &Lockfile,
    mut bufs: ImporterSeedBuffers,
    relevant: Reached,
) -> ImporterScope {
    expand_local_snapshot_closure(
        lockfile,
        bufs.local_seed,
        bufs.local_git_seed,
        (&mut bufs.local_packages, &mut bufs.local_git_packages, &mut bufs.links_by_alias),
    );
    ImporterScope {
        relevant_snapshots: relevant.snapshots,
        relevant_packages: relevant.packages,
        local_packages: bufs.local_packages,
        local_git_packages: bufs.local_git_packages,
        links_by_alias: bufs.links_by_alias,
        violations: bufs.violations,
    }
}

fn seed_single_importer_dep(
    ctx: &ImporterSeedCtx<'_>,
    alias: &PkgName,
    dep_spec: &ResolvedDependencySpec,
    bufs: &mut ImporterSeedBuffers,
) {
    let alias_str = alias.to_string();
    if let ImporterDepVersion::Link(target) = &dep_spec.version {
        let anchored = anchor_importer_link(ctx.importer_id, target);
        if importer_link_is_trusted(ctx, &alias_str, &dep_spec.specifier, &anchored) {
            bufs.links_by_alias.insert((alias_str, anchored));
        } else {
            bufs.violations.push(link_violation_with_code(
                alias.clone(),
                dep_spec.specifier.clone(),
                anchored,
                LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
                "an importer dependency declares a non-local specifier but links to an unauthorized local path",
            ));
        }
        return;
    }
    let Some(full_key) = dep_spec.version.resolved_key(alias) else {
        return;
    };
    let base_key = full_key.without_peer();
    bufs.all_seed.push(full_key.clone());
    classify_importer_resolved_package(
        ctx,
        (&alias_str, &dep_spec.specifier),
        (full_key, base_key),
        bufs,
    );
}

fn classify_importer_resolved_package(
    ctx: &ImporterSeedCtx<'_>,
    (alias_str, specifier): (&str, &str),
    (full_key, base_key): (PkgNameVerPeer, PkgNameVerPeer),
    bufs: &mut ImporterSeedBuffers,
) {
    let packages: Option<&HashMap<PkgNameVerPeer, PackageMetadata>> = ctx
        .lockfile
        .packages
        .as_ref();
    let meta = packages.and_then(|pkgs| pkgs.get(&base_key));
    let is_local_fs = is_snapshot_backed_by_local_resolution(&base_key, meta);
    let is_local_git = !is_local_fs && is_local_git_package(&base_key, meta);
    if !is_local_fs && !is_local_git {
        return;
    }
    if !importer_pkg_is_trusted(ctx, (alias_str, specifier), &base_key, meta) {
        bufs.violations.push(package_violation(
            &base_key,
            meta,
            LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE,
            "an importer dependency declares a non-local specifier but resolves to an unauthorized local package",
        ));
        return;
    }
    if is_local_fs {
        bufs.local_packages.insert(base_key);
        bufs.local_seed.push(full_key);
    } else {
        bufs.local_git_packages.insert(base_key);
        bufs.local_git_seed.push(full_key);
    }
}

fn importer_link_is_trusted(
    ctx: &ImporterSeedCtx<'_>,
    alias: &str,
    specifier: &str,
    anchored_target: &str,
) -> bool {
    ctx.policy.allow_deploy_file_url_keys
        || importer_specifier_declares_local(ctx.policy, alias, specifier)
        || ctx.workspace_dirs.contains(anchored_target)
        || ctx.lockfile_importer_dirs.contains(anchored_target)
        || policy_authorizes_importer_dep(
            ctx.policy,
            ctx.lockfile,
            alias,
            specifier,
            anchored_target,
        )
}

fn importer_pkg_is_trusted(
    ctx: &ImporterSeedCtx<'_>,
    (alias, specifier): (&str, &str),
    base_key: &PkgNameVerPeer,
    meta: Option<&PackageMetadata>,
) -> bool {
    is_deploy_entry(ctx.policy, base_key, meta)
        || importer_specifier_declares_local(ctx.policy, alias, specifier)
        || is_workspace_directory_entry(ctx.policy, ctx.workspace_dirs, base_key, meta)
        || extract_local_package_target(base_key, meta)
            .is_some_and(|target| {
                policy_authorizes_importer_dep(ctx.policy, ctx.lockfile, alias, specifier, &target)
            })
}
