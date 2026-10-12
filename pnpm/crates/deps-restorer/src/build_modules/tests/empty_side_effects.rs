use super::{
    super::{BuildModules, RebuildOptions},
    TEST_LOGGED_METHODS, create_failing_postinstall_fixture, key, policy_from_specs,
    root_importers,
};
use crate::{SideEffectsBySnapshot, SkippedSnapshots, VirtualStoreLayout};
use pnpm_config::PackageImportMethod;
use pnpm_executor::ScriptsPrependNodePath;
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_reporter::SilentReporter;
use std::{collections::HashMap, sync::Arc};
use tempfile::tempdir;

const ENGINE: &str = "darwin;arm64;node20";

fn registry_package() -> PackageMetadata {
    PackageMetadata {
        resolution: pnpm_lockfile::LockfileResolution::Registry(pnpm_lockfile::RegistryResolution {
            integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .parse()
                .expect("parse integrity"),
            revision: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

/// Runs the build of `@pnpm.e2e/failing-postinstall`, whose script exits
/// with 1, while the side-effects cache holds an empty row for it: a build
/// that ran before and changed nothing inside the package. The script
/// running is observable as an error.
fn build_with_an_empty_row(
    rebuild: Option<&RebuildOptions>,
    needs_build_marker: bool,
) -> Result<(), String> {
    let pkg_key = key("@pnpm.e2e/failing-postinstall", "1.0.0");
    let snapshots = HashMap::from([(pkg_key.clone(), SnapshotEntry::default())]);
    let packages: HashMap<PackageKey, PackageMetadata> =
        HashMap::from([(pkg_key.without_peer(), registry_package())]);
    let importers = root_importers(&[("@pnpm.e2e/failing-postinstall", "1.0.0")]);
    let policy = policy_from_specs([], true);

    let virtual_store_dir = tempdir().expect("create temp dir");
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");
    let pkg_dir = create_failing_postinstall_fixture(virtual_store_dir.path(), &pkg_key);
    if needs_build_marker {
        std::fs::write(pkg_dir.join(crate::NEEDS_BUILD_MARKER), "").expect("write the marker");
    }

    let dep_graph = crate::build_deps_graph(&snapshots, &packages);
    let cache_key = pnpm_graph_hasher::calc_dep_state(
        &dep_graph,
        &mut pnpm_graph_hasher::DepsStateCache::new(),
        &pkg_key,
        &pnpm_graph_hasher::CalcDepStateOptions {
            engine_name: ENGINE,
            patch_file_hash: None,
            include_dep_graph_hash: true,
        },
    );
    let diffs_by_snapshot: SideEffectsBySnapshot = HashMap::from([(
        pkg_key,
        Arc::new(HashMap::from([(
            cache_key,
            pnpm_store_dir::SideEffectsDiff { added: None, deleted: None, remote_origin: None },
        )])),
    )]);

    BuildModules {
        cache: crate::BuildCacheContext {
            // The overlays leave the empty row out, as the store's read path does.
            side_effects: crate::CachedSideEffects {
                overlays: Some(&HashMap::new()),
                rows: Some(&diffs_by_snapshot),
            },
            engine_name: Some(ENGINE),
            read: true,
            write: false,
            publisher: None,
            store_dir: None,
            store_index_writer: None,
            frozen_store: false,
        },
        directories: crate::BuildLayout {
            link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
            layout: &VirtualStoreLayout::legacy(
                virtual_store_dir.path(),
                pnpm_config::default_virtual_store_dir_max_length() as usize,
            ),
            pkg_roots_by_key: None,
            gather_ancestor_bin_paths: false,
            modules_dir: modules_dir.path(),
            lockfile_dir: lockfile_dir.path(),
            import_method: PackageImportMethod::Auto,
            logged_methods: &TEST_LOGGED_METHODS,
        },
        graph: crate::BuildGraphInputs {
            snapshots: Some(&snapshots),
            packages: Some(&packages),
            patches: None,
            requires_build_by_snapshot: None,
            importers: &importers,
            dependency_groups: None,
            build_scope: None,
        },
        scripts: crate::BuildScriptOptions {
            extra_env: &HashMap::new(),
            user_agent: "pnpm/test",
            path: crate::ScriptPath {
                prepend_node_path: ScriptsPrependNodePath::Never,
                extra_bin_paths: &[],
                private_hoisting: false,
            },
            shell: None,
            shell_emulator: false,
            unsafe_perm: true,
            ignore: false,
            patched_engines: crate::PatchedEngineCheck {
                engine_strict: false,
                node_version: None,
                install_state_dir: None,
            },
        },
        allow_build_policy: &policy,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        rebuild,
    }
    .run::<SilentReporter>()
    .map(drop)
    .map_err(|error| error.to_string())
}

fn rebuild_options(skip_if_has_side_effects_cache: bool) -> RebuildOptions {
    RebuildOptions {
        selected_names: None,
        pending_projects: Vec::new(),
        check_lockfile_patches_only: true,
        skip_if_has_side_effects_cache,
    }
}

/// The build ran before and left nothing to restore, so a rebuild that
/// skips cached builds does not run it again, as pnpm v11's does.
#[test]
fn rebuild_skipping_cached_builds_skips_a_build_with_an_empty_row() {
    build_with_an_empty_row(Some(&rebuild_options(true)), false)
        .expect("the rebuild must not run the script of a cached build");
}

#[test]
fn plain_rebuild_runs_a_build_with_an_empty_row() {
    build_with_an_empty_row(Some(&rebuild_options(false)), false)
        .expect_err("a plain rebuild runs every selected script");
}

/// An install still runs the build: its effect may lie outside the package
/// directory, where the empty row cannot restore it (pnpm/pnpm#14717).
#[test]
fn install_runs_a_build_with_an_empty_row() {
    build_with_an_empty_row(None, false)
        .expect_err("an install runs a build the cache cannot restore");
}

/// A slot that carries the build marker has not been built, so even a
/// rebuild that skips cached builds runs it.
#[test]
fn rebuild_skipping_cached_builds_runs_a_slot_that_needs_a_build() {
    build_with_an_empty_row(Some(&rebuild_options(true)), true)
        .expect_err("a slot with the build marker must be built");
}
