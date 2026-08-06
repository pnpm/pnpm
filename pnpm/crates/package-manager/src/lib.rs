mod add;
mod build_resolution_verifiers;
mod build_snapshot;
mod catalog_cleanup;
mod catalog_mode;
mod check_custom_resolver_force_resolve;
mod compat_package_extensions;
mod dependencies_graph_to_lockfile;
mod fast_update_catalogs;
mod fast_update_ignored_optional_dependencies;
mod fast_update_importers;
mod fast_update_lockfile;
mod fast_update_overrides;
mod fast_update_settings;
mod install;
mod install_with_fresh_lockfile;
mod link_manifest_link_deps;
mod lockfile_diff;
mod minimum_release_age;
mod optimistic_repeat_install;
mod overrides;
mod package_extender;
mod patch;
mod prefetching_resolver;
mod prune_virtual_store;
mod remove;
mod resolution_observer;
mod resolution_policy;
mod resolve_latest;
mod tarball_prefetch;
mod update;
mod update_project_manifest;
mod update_project_manifest_object;
mod warn_on_stale_convergence_overrides;

pub use add::*;
pub use build_resolution_verifiers::*;
pub use build_snapshot::*;
pub use catalog_mode::*;
pub use dependencies_graph_to_lockfile::*;
pub use install::*;
pub use install_with_fresh_lockfile::*;
pub use link_manifest_link_deps::*;
pub use lockfile_diff::*;
pub use minimum_release_age::MinimumReleaseAgeError;
pub use optimistic_repeat_install::*;
pub use overrides::*;
pub use package_extender::*;
pub use pacquet_deps_restorer::*;
pub use pacquet_patching::{
    PatchCommitError, PkgFilesForDiff, diff_folders, prepare_pkg_files_for_diff,
};
pub use patch::*;
pub use prefetching_resolver::*;
pub use remove::*;
pub use resolution_observer::*;
pub use resolve_latest::ResolveLatestError;
pub use tarball_prefetch::*;
pub use update::*;
pub use update_project_manifest::*;
pub use update_project_manifest_object::*;

/// The dependency groups a project installs directly — `dependencies`,
/// `devDependencies`, `optionalDependencies` — in the order pnpm's
/// `updateProjectManifest` walks them. This is also the install `include`
/// set for runs whose `dependency_groups` carries no user filter intent
/// (`add`, `remove`, `update`): those mutations pick a manifest group to
/// save into separately, and the install itself must keep resolving and
/// materializing every group.
pub(crate) const DIRECT_GROUPS: [pacquet_package_manifest::DependencyGroup; 3] = [
    pacquet_package_manifest::DependencyGroup::Prod,
    pacquet_package_manifest::DependencyGroup::Dev,
    pacquet_package_manifest::DependencyGroup::Optional,
];

pub(crate) fn package_manifest_prefix(
    manifest: &pacquet_package_manifest::PackageManifest,
) -> String {
    manifest.path().parent().unwrap_or_else(|| manifest.path()).to_string_lossy().into_owned()
}

pub(crate) fn emit_initial_package_manifest<Reporter: pacquet_reporter::Reporter>(
    manifest: &pacquet_package_manifest::PackageManifest,
) {
    Reporter::emit(&pacquet_reporter::LogEvent::PackageManifest(
        pacquet_reporter::PackageManifestLog {
            level: pacquet_reporter::LogLevel::Debug,
            message: pacquet_reporter::PackageManifestMessage::Initial {
                prefix: package_manifest_prefix(manifest),
                initial: manifest.value().clone(),
            },
        },
    ));
}

#[cfg(test)]
mod tests;
