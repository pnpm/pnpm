use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;

/// Apply the `runtimeOnFail` setting to a manifest read from disk, so it
/// declares the runtime dependencies the install acts on.
pub fn apply_runtime_on_fail(config: &Config, manifest: &mut PackageManifest) {
    if let Some(runtime_on_fail) = config.runtime_on_fail {
        manifest.apply_runtime_on_fail(runtime_on_fail.as_str());
    }
}

/// [`apply_runtime_on_fail`] on the manifest of each of `projects`.
pub fn apply_runtime_on_fail_to_projects(
    config: &Config,
    projects: &mut [pnpm_workspace::Project],
) {
    for project in projects {
        apply_runtime_on_fail(config, &mut project.manifest);
    }
}
