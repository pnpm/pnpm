use crate::{PackageSpecObject, is_workspace_local_path_specifier, update_project_manifest_object};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_registry::PinnedVersion;

/// Catalog metadata for a direct dependency requested through the `catalog:`
/// protocol. Port of pnpm's `CatalogLookupMetadata`.
pub struct CatalogLookup {
    pub catalog_name: String,
    pub specifier: String,
    /// The `catalog:` text the user wrote (`catalog:` or `catalog:<name>`).
    /// Written back to the manifest verbatim so a catalog reference survives
    /// `add` / `update` instead of being replaced by the resolved version.
    pub user_specified_bare_specifier: String,
}

/// A direct dependency as it came back from resolution. Port of the subset of
/// pnpm's `ResolvedDirectDependency` that `updateProjectManifest` consumes.
pub struct ResolvedDirectDependency {
    /// Install name in `node_modules` (the manifest key to rewrite).
    pub alias: String,
    /// Resolved concrete version, when the resolver knows one. `None` for
    /// git / tarball deps with no semver version.
    pub version: Option<String>,
    /// Resolver's canonical echo of the bare specifier (e.g. `^4` for a
    /// registry range, `github:owner/repo#sha` for a git shorthand).
    pub normalized_bare_specifier: Option<String>,
    pub catalog_lookup: Option<CatalogLookup>,
}

/// A wanted dependency the manifest writer matches resolved deps against. Only
/// entries flagged [`update_spec`](Self::update_spec) participate, mirroring
/// pnpm's `wantedDependencies` filter.
pub struct WantedDependencyUpdate {
    /// Install alias, when the request carried one. `None` for a no-alias
    /// shorthand such as a bare `owner/repo#sha` GitHub request.
    pub alias: Option<String>,
    pub bare_specifier: String,
    pub update_spec: bool,
}

/// Inputs to [`update_project_manifest`] beyond the manifest itself.
pub struct UpdateProjectManifestOptions<'a> {
    pub wanted_dependencies: &'a [WantedDependencyUpdate],
    pub direct_dependencies: &'a [ResolvedDirectDependency],
    /// Also record the saved deps in `peerDependencies` (pnpm's
    /// `importer.peer`).
    pub peer: bool,
    pub pinned_version: Option<PinnedVersion>,
    /// The dependency field the saved deps belong in (pnpm's
    /// `targetDependenciesField`). `None` leaves each dep in whichever field
    /// already declares it.
    pub target_dependencies_field: Option<DependencyGroup>,
    pub preserve_workspace_protocol: bool,
}

/// Rewrite `manifest`'s dependency specs from a completed resolution, mirroring
/// pnpm's `updateProjectManifest`
/// (`installing/deps-resolver/src/updateProjectManifest.ts`), including the
/// alias-matching fix from [pnpm#11373](https://github.com/pnpm/pnpm/pull/11373).
///
/// Each resolved direct dependency is matched to a wanted dependency **by
/// alias** (not by position), so a dependency that failed to resolve — an
/// optional dep dropped from `direct_dependencies` — cannot shift the match and
/// rewrite an unrelated dependency ([#11267](https://github.com/pnpm/pnpm/issues/11267)).
/// A wanted dep flagged `update_spec` that matched nothing is still upserted
/// with no specifier, which preserves its existing range rather than dropping
/// it.
pub fn update_project_manifest(
    manifest: &mut PackageManifest,
    opts: &UpdateProjectManifestOptions<'_>,
) {
    let mut specs: Vec<PackageSpecObject> = opts
        .direct_dependencies
        .iter()
        .filter_map(|resolved| {
            let wanted = opts
                .wanted_dependencies
                .iter()
                .find(|wanted| wanted_dep_matches_resolved_dep(wanted, resolved))?;
            Some(PackageSpecObject {
                alias: resolved.alias.clone(),
                peer: opts.peer,
                bare_specifier: Some(get_bare_specifier_to_save(
                    wanted,
                    resolved,
                    opts.preserve_workspace_protocol,
                )),
                resolved_version: resolved.version.clone(),
                pinned_version: opts.pinned_version,
                save_type: opts.target_dependencies_field,
            })
        })
        .collect();

    for wanted in opts.wanted_dependencies {
        let Some(alias) = wanted.alias.as_deref().filter(|alias| !alias.is_empty()) else {
            continue;
        };
        if wanted.update_spec && !specs.iter().any(|spec| spec.alias == alias) {
            specs.push(PackageSpecObject {
                alias: alias.to_string(),
                peer: opts.peer,
                bare_specifier: None,
                resolved_version: None,
                pinned_version: None,
                save_type: opts.target_dependencies_field,
            });
        }
    }

    update_project_manifest_object(manifest, &specs);
}

/// Whether `resolved` is the resolution of `wanted`. Matches by alias when the
/// request carried one; otherwise by the resolved normalized specifier, with a
/// `github:` shorthand fallback so a no-alias `owner/repo#sha` request matches
/// its `github:owner/repo#sha` resolution.
fn wanted_dep_matches_resolved_dep(
    wanted: &WantedDependencyUpdate,
    resolved: &ResolvedDirectDependency,
) -> bool {
    if !wanted.update_spec {
        return false;
    }
    if let Some(alias) = wanted.alias.as_deref().filter(|alias| !alias.is_empty()) {
        return alias == resolved.alias;
    }
    let Some(normalized) = resolved.normalized_bare_specifier.as_deref() else {
        return false;
    };
    wanted.bare_specifier == normalized || format!("github:{}", wanted.bare_specifier) == normalized
}

/// The specifier string to write for a matched dependency: a `catalog:`
/// reference is preserved verbatim; a workspace-local path spec is kept when
/// `preserve_workspace_protocol` is set; otherwise the resolver's normalized
/// specifier wins, falling back to the originally-wanted spec.
fn get_bare_specifier_to_save(
    wanted: &WantedDependencyUpdate,
    resolved: &ResolvedDirectDependency,
    preserve_workspace_protocol: bool,
) -> String {
    if let Some(catalog_lookup) = &resolved.catalog_lookup {
        return catalog_lookup.user_specified_bare_specifier.clone();
    }
    if preserve_workspace_protocol && is_workspace_local_path_specifier(&wanted.bare_specifier) {
        return wanted.bare_specifier.clone();
    }
    resolved.normalized_bare_specifier.clone().unwrap_or_else(|| wanted.bare_specifier.clone())
}

#[cfg(test)]
mod tests;
