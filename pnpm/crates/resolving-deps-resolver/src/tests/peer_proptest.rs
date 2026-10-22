//! Property tests over generated workspaces with peer dependencies
//! (pnpm/tasks#143). Each case is a few importers over a handful of
//! packages published at two majors, whose manifests mix dependencies,
//! required and optional peers, peers that are also dependencies, and
//! peer cycles.

mod counterexamples;
mod model;

use std::{collections::BTreeMap, sync::Arc};

use model::{
    IMPORTER_IDS, Outcome, Settings, Workspace, importer_dependencies, lockfile_of,
    move_dependency, outcome_of,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::{PreferredVersions, ResolveOptions};
use proptest::{prelude::*, test_runner::RngSeed};

use super::StubResolver;
use crate::{
    ResolveImporterOptions, ResolveWorkspaceResult, WorkspaceImporter, WorkspaceResolveOptions,
    resolve_workspace,
};

/// How a run treats the lockfile it starts from.
#[derive(Clone, Copy)]
enum Reuse<'a> {
    Fresh,
    /// `pnpm install`: keep every still-satisfied locked subtree.
    Install(&'a pnpm_lockfile::Lockfile),
    /// `pnpm dedupe`: keep the locked pins but re-walk every edge.
    Dedupe(&'a pnpm_lockfile::Lockfile),
}

fn manifests(workspace: &Workspace) -> Vec<(tempfile::TempDir, PackageManifest)> {
    workspace.importers
        .iter()
        .map(|edges| {
            let tmp = tempfile::tempdir().expect("tempdir");
            let path = tmp.path().join("package.json");
            let json = serde_json::json!({
                "name": "importer",
                "version": "0.0.0",
                "dependencies": importer_dependencies(edges),
            });
            std::fs::write(&path, json.to_string()).expect("write package.json");
            let manifest = PackageManifest::from_path(path).expect("parse package.json");
            (tmp, manifest)
        })
        .collect()
}

fn importer_opts(id: &str, settings: Settings) -> ResolveImporterOptions {
    ResolveImporterOptions {
        base_opts: ResolveOptions {
            project: pnpm_resolving_resolver_base::ResolverProjectOptions {
                project_dir: std::path::Path::new("/repo").join(id),
                ..Default::default()
            },
            ..ResolveOptions::default()
        },
        peers_suffix_max_length: settings.peers_suffix_max_length,
        peers: crate::ImporterPeerOptions {
            auto_install_peers: settings.auto_install_peers,
            auto_install_peers_from_highest_match: false,
            resolve_peers_from_workspace_root: false,
            dedupe_peers: false,
            dedupe_peer_dependents: true,
        },
        links: crate::PeerLinkOptions {
            exclude_links_from_lockfile: false,
            lockfile_dir: None,
            modules_dir: None,
        },
        resolution: crate::ImporterResolutionInputs {
            all_preferred_versions: Arc::new(PreferredVersions::new()),
            override_bare_specifier: None,
            patched_dependencies: None,
            pick_lowest_direct: false,
            subdep_published_by: None,
            catalogs: pnpm_catalogs_types::Catalogs::new(),
            catalogs_dir: None,
            catalog_server: false,
        },
        hooks: crate::ManifestTransformHooks {
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
        },
    }
}

fn workspace_opts(settings: Settings, reuse: Reuse<'_>) -> WorkspaceResolveOptions {
    let (lockfile, subtrees) = match reuse {
        Reuse::Fresh => (None, true),
        Reuse::Install(lockfile) => (Some(lockfile), true),
        Reuse::Dedupe(lockfile) => (Some(lockfile), false),
    };
    WorkspaceResolveOptions {
        registry_context: pnpm_lockfile::RegistryContext::default(),
        share_workspace_resolutions: true,
        allowed_deprecated_versions: BTreeMap::new(),
        peers: crate::WorkspacePeerResolutionOptions {
            dedupe_peers: false,
            dedupe_injected_deps: false,
            dedupe_peer_dependents: true,
            resolve_peers_from_workspace_root: false,
            exclude_links_from_lockfile: false,
            lockfile_dir: std::path::PathBuf::from("/repo"),
            peers_suffix_max_length: settings.peers_suffix_max_length,
            auto_install_peers: settings.auto_install_peers,
        },
        hooks: crate::WorkspaceResolveHooks::default(),
        reuse: crate::WorkspaceLockfileReuse {
            lockfile: lockfile.cloned().map(Arc::new),
            subtrees,
            ..crate::WorkspaceLockfileReuse::default()
        },
        version: crate::WorkspaceVersionResolution::default(),
    }
}

/// Resolve the workspace with its importers listed in `order`.
fn resolve(
    workspace: &Workspace,
    order: &[usize],
    reuse: Reuse<'_>,
) -> Result<ResolveWorkspaceResult, String> {
    resolve_with(workspace, order, Settings::default(), reuse)
}

fn resolve_with(
    workspace: &Workspace,
    order: &[usize],
    settings: Settings,
    reuse: Reuse<'_>,
) -> Result<ResolveWorkspaceResult, String> {
    let resolver: StubResolver = model::stub_resolver(workspace);
    let manifests = manifests(workspace);
    let importers: Vec<WorkspaceImporter<'_>> = order
        .iter()
        .map(|&index| WorkspaceImporter {
            id: IMPORTER_IDS[index].to_string(),
            manifest: &manifests[index].1,
        })
        .collect();
    let runtime = tokio::runtime::Builder::new_current_thread().build().expect("build runtime");
    runtime
        .block_on(resolve_workspace(
            &resolver,
            &importers,
            &[DependencyGroup::Prod],
            workspace_opts(settings, reuse),
            |importer| importer_opts(&importer.id, settings),
        ))
        .map_err(|error| format!("{error:?}"))
}

fn in_order(workspace: &Workspace) -> Vec<usize> {
    (0..workspace.importers.len()).collect()
}

fn outcome_with(
    workspace: &Workspace,
    order: &[usize],
    settings: Settings,
    reuse: Reuse<'_>,
) -> Outcome {
    let result =
        resolve_with(workspace, order, settings, reuse).expect("generated workspaces resolve");
    outcome_of(&result, workspace)
}

/// A fixed seed, so every run checks the same cases. Run others locally with
/// `PROPTEST_CASES=5000 PROPTEST_RNG_SEED=<n>`.
fn config() -> ProptestConfig {
    let from_env = ProptestConfig::default();
    let cases = if std::env::var_os("PROPTEST_CASES").is_some() { from_env.cases } else { 64 };
    let rng_seed = match from_env.rng_seed {
        RngSeed::Random => RngSeed::Fixed(143),
        seed @ RngSeed::Fixed(_) => seed,
    };
    ProptestConfig { cases, rng_seed, failure_persistence: None, ..from_env }
}

proptest! {
    #![proptest_config(config())]

    /// Installing over the lockfile a fresh resolution wrote changes nothing.
    #[test]
    fn install_over_own_lockfile_matches_fresh(
        workspace in model::workspace(),
        settings in model::settings(),
    ) {
        let order = in_order(&workspace);
        let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh)
            .expect("generated workspaces resolve");
        let lockfile = lockfile_of(&fresh, &workspace);
        let reused = outcome_with(&workspace, &order, settings, Reuse::Install(&lockfile));
        prop_assert_eq!(outcome_of(&fresh, &workspace), reused);
    }

    /// The order the importers are listed in does not change the result.
    #[test]
    fn importer_order_does_not_change_the_result(
        workspace in model::workspace(),
        settings in model::settings(),
    ) {
        let order = in_order(&workspace);
        let expected = outcome_with(&workspace, &order, settings, Reuse::Fresh);
        let reversed: Vec<usize> = order.iter().rev().copied().collect();
        prop_assert_eq!(&expected, &outcome_with(&workspace, &reversed, settings, Reuse::Fresh));
        let mut rotated = order;
        rotated.rotate_left(1);
        prop_assert_eq!(&expected, &outcome_with(&workspace, &rotated, settings, Reuse::Fresh));
    }

    /// A second dedupe over the lockfile the first one wrote changes nothing.
    #[test]
    fn dedupe_is_idempotent(
        workspace in model::workspace(),
        settings in model::settings(),
    ) {
        let order = in_order(&workspace);
        let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh)
            .expect("generated workspaces resolve");
        let first = resolve_with(
            &workspace,
            &order,
            settings,
            Reuse::Dedupe(&lockfile_of(&fresh, &workspace)),
        )
        .expect("dedupe resolves");
        let second = outcome_with(
            &workspace,
            &order,
            settings,
            Reuse::Dedupe(&lockfile_of(&first, &workspace)),
        );
        prop_assert_eq!(outcome_of(&first, &workspace), second);
    }

    /// Mutating one importer's dependency specifier and deduplicating over the
    /// prior lockfile produces the same graph as resolving the mutated
    /// workspace fresh.
    #[test]
    fn move_plus_dedupe_matches_fresh(
        workspace in model::workspace(),
        settings in model::settings(),
        importer in 0usize..3,
        edge in 0usize..3,
    ) {
        let (initial_ws, moved) = move_dependency(&workspace, importer, edge);
        let order = in_order(&initial_ws);
        let initial = resolve_with(&initial_ws, &order, settings, Reuse::Fresh)
            .expect("initial workspace resolves");
        let lockfile = lockfile_of(&initial, &initial_ws);
        let fresh = outcome_with(&moved, &order, settings, Reuse::Fresh);
        let installed = outcome_with(&moved, &order, settings, Reuse::Install(&lockfile));
        prop_assert_eq!(&fresh, &installed);
        let deduped = outcome_with(&moved, &order, settings, Reuse::Dedupe(&lockfile));
        prop_assert_eq!(fresh, deduped);
    }
}
