use super::{
    HashMap, InstallOptions, NodeApiProject, invalid_manifest_error, unsupported_option_error,
};

/// Reject a project whose `manifest` is not a JSON object up front.
/// `PackageManifest::from_value` coerces a non-object to `{}` as a last-resort
/// panic guard, but a silently-emptied manifest would drive resolution and
/// lockfile writing off missing data — so fail closed with a clear error here.
pub(super) fn reject_non_object_manifests(projects: &[NodeApiProject]) -> napi::Result<()> {
    for project in projects {
        if !project.manifest.is_object()
            || project.dependency_manifest.as_ref().is_some_and(|value| !value.is_object())
        {
            return Err(invalid_manifest_error(&project.root_dir));
        }
    }
    Ok(())
}

pub(super) fn reject_unsupported_install_options(options: &InstallOptions) -> napi::Result<()> {
    reject_non_empty_map(options.auth_config.as_ref(), "authConfig")?;
    // `neverBuiltDependencies` was replaced by `allowBuilds` in pnpm v12:
    // hosts fold it into explicit `allowBuilds: false` entries themselves.
    reject_non_empty_list(options.never_built_dependencies.as_deref(), "neverBuiltDependencies")?;
    Ok(())
}

fn reject_non_empty_map<Value>(
    value: Option<&HashMap<String, Value>>,
    option: &str,
) -> napi::Result<()> {
    reject_if(value.is_some_and(|map| !map.is_empty()), option)
}

fn reject_non_empty_list<Value>(value: Option<&[Value]>, option: &str) -> napi::Result<()> {
    reject_if(value.is_some_and(|items| !items.is_empty()), option)
}

fn reject_if(condition: bool, option: &str) -> napi::Result<()> {
    if condition { Err(unsupported_option_error("install", option)) } else { Ok(()) }
}
