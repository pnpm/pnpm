use pnpm_config::Config;
use pnpm_package_manifest::PackageManifest;

/// Apply the `runtimeOnFail` setting to a manifest read from disk, so it
/// declares the runtime dependencies the install acts on.
pub fn apply_runtime_on_fail(config: &Config, manifest: &mut PackageManifest) {
    if let Some(runtime_on_fail) = config.runtime_on_fail {
        pnpm_package_manifest::apply_runtime_on_fail_override(
            manifest.value_mut(),
            runtime_on_fail.as_str(),
        );
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

/// The install view of a declared manifest, when the local runtime policy applies.
pub fn runtime_on_fail_manifest(
    config: &Config,
    manifest: &PackageManifest,
) -> Option<PackageManifest> {
    config.runtime_on_fail?;
    if !declares_runtime(manifest) {
        return None;
    }
    let mut effective = manifest.clone();
    apply_runtime_on_fail(config, &mut effective);
    Some(effective)
}

pub(crate) fn declares_runtime(manifest: &PackageManifest) -> bool {
    ["devEngines", "engines"]
        .into_iter()
        .any(|field| manifest.value()[field].get("runtime").is_some())
}
