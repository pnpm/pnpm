use super::{CreateVirtualStore, CreateVirtualStoreError, CreateVirtualStoreOutput, Snapshots};
use crate::BuildModules;
use pnpm_reporter::Reporter;

pub(super) fn run<Report: Reporter>(
    installer: &CreateVirtualStore<'_>,
    snapshots: &Snapshots,
    context: &crate::InstallContext<'_>,
    fetched: &CreateVirtualStoreOutput,
    captured: &crate::CapturedBuildSideEffects,
) -> Result<crate::BuildModulesOutput, CreateVirtualStoreError> {
    let config = context.config;
    link_bins(installer, snapshots, context, fetched)?;
    let extra_env = config.extra_env_with_node_options();
    let publisher =
        crate::shared_side_effects::shared_side_effects_publisher(config, installer.importers);
    BuildModules {
        cache: cache(installer, context, fetched, captured, publisher.as_ref()),
        directories: directories(context),
        graph: graph(installer, snapshots, fetched),
        scripts: crate::BuildScriptOptions::from_config(config, &extra_env),
        allow_build_policy: context.allow_build_policy,
        child_concurrency: config.child_concurrency,
        skipped: installer.selection.skipped,
        rebuild: None,
    }
    .run::<Report>()
    .map_err(CreateVirtualStoreError::LoadedBuild)
}

fn link_bins(
    installer: &CreateVirtualStore<'_>,
    snapshots: &Snapshots,
    context: &crate::InstallContext<'_>,
    fetched: &CreateVirtualStoreOutput,
) -> Result<(), CreateVirtualStoreError> {
    crate::LinkVirtualStoreBins {
        layout: context.linker.layout,
        snapshots: Some(snapshots),
        selected_snapshots: None,
        packages: installer.entries.packages,
        package_manifests: &fetched.package_manifests,
        package_bins: Some(&fetched.package_bins),
        skipped: installer.selection.skipped,
        link_options: context.linker.bin_options,
    }
    .run()
    .map_err(CreateVirtualStoreError::LoadedBins)
}

fn cache<'a>(
    installer: &'a CreateVirtualStore<'_>,
    context: &'a crate::InstallContext<'_>,
    fetched: &'a CreateVirtualStoreOutput,
    captured: &'a crate::CapturedBuildSideEffects,
    publisher: Option<&'a crate::shared_side_effects::SharedSideEffectsPublisher>,
) -> crate::BuildCacheContext<'a> {
    let config = context.config;
    crate::BuildCacheContext {
        captured: Some(captured),
        side_effects: crate::CachedSideEffects {
            overlays: Some(&fetched.side_effects.maps),
            rows: Some(&fetched.side_effects.diffs),
        },
        engine_name: context.linker.layout.engine_name(),
        read: true,
        write: true,
        publisher,
        store: Some(crate::BuildCacheStore {
            directory: &config.store_dir,
            writer: installer.fetching.store_index_writer,
        }),
        frozen_store: false,
    }
}

fn directories<'a>(context: &'a crate::InstallContext<'_>) -> crate::BuildLayout<'a> {
    let config = context.config;
    crate::BuildLayout {
        link_options: context.linker.bin_options,
        layout: context.linker.layout,
        pkg_roots_by_key: None,
        gather_ancestor_bin_paths: false,
        modules_dir: &config.modules_dir,
        lockfile_dir: context.workspace_root,
        import_method: config.package_import_method,
        logged_methods: context.caches.logged_methods,
    }
}

fn graph<'a>(
    installer: &'a CreateVirtualStore<'_>,
    snapshots: &'a Snapshots,
    fetched: &'a CreateVirtualStoreOutput,
) -> crate::BuildGraphInputs<'a> {
    crate::BuildGraphInputs {
        snapshots: Some(snapshots),
        packages: installer.entries.packages,
        patches: None,
        requires_build_by_snapshot: Some(&fetched.requires_build_by_snapshot),
        importers: installer.importers,
        dependency_groups: None,
        build_scope: None,
    }
}
