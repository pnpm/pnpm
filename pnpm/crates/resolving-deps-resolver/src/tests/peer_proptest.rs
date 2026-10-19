//! Property tests over generated workspaces with peer dependencies
//! (pnpm/tasks#143). Each case is a few importers over a handful of
//! packages published at two majors, whose manifests mix dependencies,
//! required and optional peers, peers that are also dependencies, and
//! peer cycles.

mod counterexamples;
mod model;

use std::{collections::BTreeMap, sync::Arc};

use model::{IMPORTER_IDS, Outcome, Workspace, importer_dependencies, lockfile_of, outcome_of};
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

fn importer_opts(id: &str) -> ResolveImporterOptions {
    ResolveImporterOptions {
        base_opts: ResolveOptions {
            project: pnpm_resolving_resolver_base::ResolverProjectOptions {
                project_dir: std::path::Path::new("/repo").join(id),
                ..Default::default()
            },
            ..ResolveOptions::default()
        },
        peers_suffix_max_length: 1000,
        peers: crate::ImporterPeerOptions {
            auto_install_peers: true,
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

fn workspace_opts(reuse: Reuse<'_>) -> WorkspaceResolveOptions {
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
            peers_suffix_max_length: 1000,
            auto_install_peers: true,
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
            workspace_opts(reuse),
            |importer| importer_opts(&importer.id),
        ))
        .map_err(|error| format!("{error:?}"))
}

fn in_order(workspace: &Workspace) -> Vec<usize> {
    (0..workspace.importers.len()).collect()
}

fn outcome(workspace: &Workspace, order: &[usize], reuse: Reuse<'_>) -> Outcome {
    let result = resolve(workspace, order, reuse).expect("generated workspaces resolve");
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
    #[ignore = "fails on counterexamples::install_over_own_lockfile_keeps_an_optional_peer_on_a_cycle (pnpm/tasks#143)"]
    fn install_over_own_lockfile_matches_fresh(workspace in model::workspace()) {
        let order = in_order(&workspace);
        let fresh = resolve(&workspace, &order, Reuse::Fresh).expect("generated workspaces resolve");
        let lockfile = lockfile_of(&fresh, &workspace);
        let reused = outcome(&workspace, &order, Reuse::Install(&lockfile));
        prop_assert_eq!(outcome_of(&fresh, &workspace), reused);
    }

    /// The order the importers are listed in does not change the result.
    #[test]
    fn importer_order_does_not_change_the_result(workspace in model::workspace()) {
        let order = in_order(&workspace);
        let expected = outcome(&workspace, &order, Reuse::Fresh);
        let reversed: Vec<usize> = order.iter().rev().copied().collect();
        prop_assert_eq!(&expected, &outcome(&workspace, &reversed, Reuse::Fresh));
        let mut rotated = order;
        rotated.rotate_left(1);
        prop_assert_eq!(&expected, &outcome(&workspace, &rotated, Reuse::Fresh));
    }

    /// A second dedupe over the lockfile the first one wrote changes nothing.
    #[test]
    fn dedupe_is_idempotent(workspace in model::workspace()) {
        let order = in_order(&workspace);
        let fresh = resolve(&workspace, &order, Reuse::Fresh).expect("generated workspaces resolve");
        let first = resolve(&workspace, &order, Reuse::Dedupe(&lockfile_of(&fresh, &workspace)))
            .expect("dedupe resolves");
        let second = outcome(&workspace, &order, Reuse::Dedupe(&lockfile_of(&first, &workspace)));
        prop_assert_eq!(outcome_of(&first, &workspace), second);
    }
}
