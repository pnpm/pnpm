use std::collections::HashSet;

use axum::{http::StatusCode, response::Response};
use pnpm_package_manager::is_host_filesystem_specifier;
use pnpm_package_manifest::DependencyGroup;

use crate::resolver::{
    json_error,
    protocol::{ProjectDeps, ResolveRequest},
};

/// Reject a request that asks the server to resolve a client-local filesystem
/// dependency (`file:`, `link:`, a bare local path, or a `workspace:` path
/// that escapes the workspace root).
///
/// Runs before every resolve path, including the frozen fast path, so a
/// request carrying a local path never reaches the server's filesystem or is
/// cached as a server resolution.
pub(in crate::resolver) fn reject_local_dependency_specs(
    request: &ResolveRequest,
) -> Option<Response> {
    let declared_prefixes: HashSet<&str> = request.registries
        .values()
        .flat_map(|declaration| declaration.prefixes.iter())
        .collect();
    let projects = request.projects_normalized();
    first_local_project_spec(&projects, &declared_prefixes)
        .or_else(|| first_local_catalog_spec(request, &declared_prefixes))
        .or_else(|| first_local_extension_spec(request, &declared_prefixes))
        .or_else(|| first_local_lockfile_importer_spec(request, &declared_prefixes))
        .map(str::to_owned)
        .or_else(|| {
            request.overrides
                .as_ref()
                .and_then(|overrides| first_local_override_spec(overrides, &declared_prefixes))
        })
        .map(|spec| {
            json_error(
                StatusCode::BAD_REQUEST,
                &format!("local filesystem dependency {spec:?} cannot be resolved by pnpr"),
            )
        })
}

fn is_local_request_spec(
    spec: &str,
    importer_dir: &str,
    declared_prefixes: &HashSet<&str>,
) -> bool {
    // `git+...://...` transports are network URLs owned by the route allowlist
    // check (`reject_off_allowlist_fetches`).
    if spec
        .get(..4)
        .is_some_and(|head| head.eq_ignore_ascii_case("git+"))
        && spec.contains("://")
    {
        return false;
    }
    is_host_filesystem_specifier(spec, importer_dir, |prefix| declared_prefixes.contains(prefix))
}

fn first_local_project_spec<'a>(
    projects: &'a [ProjectDeps],
    declared_prefixes: &HashSet<&str>,
) -> Option<&'a str> {
    for project in projects {
        for dependencies in [
            &project.dependencies,
            &project.dev_dependencies,
            &project.optional_dependencies,
            &project.peer_dependencies,
        ] {
            if let Some(spec) = dependencies
                .values()
                .map(String::as_str)
                .find(|spec| is_local_request_spec(spec, &project.dir, declared_prefixes))
            {
                return Some(spec);
            }
        }
    }
    None
}

fn first_local_catalog_spec<'a>(
    request: &'a ResolveRequest,
    declared_prefixes: &HashSet<&str>,
) -> Option<&'a str> {
    request.catalogs
        .as_ref()?
        .values()
        .flat_map(|catalog| catalog.values())
        .map(String::as_str)
        .find(|spec| is_local_request_spec(spec, ".", declared_prefixes))
}

fn first_local_extension_spec<'a>(
    request: &'a ResolveRequest,
    declared_prefixes: &HashSet<&str>,
) -> Option<&'a str> {
    let extensions = request.package_extensions.as_ref()?;
    for extension in extensions.values() {
        for dependencies in [
            extension.dependencies.as_ref(),
            extension.optional_dependencies.as_ref(),
            extension.peer_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(spec) = dependencies
                .values()
                .map(String::as_str)
                .find(|spec| is_local_request_spec(spec, ".", declared_prefixes))
            {
                return Some(spec);
            }
        }
    }
    None
}

fn first_local_lockfile_importer_spec<'a>(
    request: &'a ResolveRequest,
    declared_prefixes: &HashSet<&str>,
) -> Option<&'a str> {
    if !request.ignore_manifest_check {
        return None;
    }
    let lockfile = request.lockfile.as_ref()?;
    for (importer_id, snapshot) in &lockfile.importers {
        let groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
        if let Some((_, dep)) = snapshot
            .dependencies_by_groups(groups)
            .find(|(_, dep)| is_local_request_spec(&dep.specifier, importer_id, declared_prefixes))
        {
            return Some(&dep.specifier);
        }
    }
    None
}

fn first_local_override_spec(
    value: &serde_json::Value,
    declared_prefixes: &HashSet<&str>,
) -> Option<String> {
    match value {
        serde_json::Value::String(spec) => {
            is_local_request_spec(spec, ".", declared_prefixes).then(|| spec.clone())
        }
        serde_json::Value::Array(items) => {
            items.iter().find_map(|item| first_local_override_spec(item, declared_prefixes))
        }
        serde_json::Value::Object(map) => {
            map.values().find_map(|item| first_local_override_spec(item, declared_prefixes))
        }
        _ => None,
    }
}
