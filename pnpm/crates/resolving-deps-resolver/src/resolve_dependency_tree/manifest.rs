//! What the walk reads off a resolved package: its
//! `pkgIdWithPatchHash`, its child specs, its peer dependencies, its
//! leaf classification, and its deprecation notice.

use pnpm_catalogs_types::Catalogs;
use pnpm_package_manifest::engines_runtime_dependencies;
use pnpm_patching::get_patch_info;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::resolved_tree::PeerDep;

use super::{
    Deprecation, ResolveDependencyTreeError, catalogs::resolve_catalog_specifier,
    dependency_is_injected, lock_recoverable, tree_ctx::TreeCtx, workspace_ctx::ChildSpec,
};

/// Compute the `pkgIdWithPatchHash` for a freshly-resolved package:
///
/// 1. Prefix the resolver's `id` with `<name>@` when it doesn't already
///    start that way. The npm-registry resolver always returns
///    `name@version`; git / tarball / local resolvers return URL-shaped
///    ids and need the prefix so the snapshot key starts with the
///    package name.
/// 2. Look the `(name, version)` pair up in `ctx.patched_dependencies`
///    via [`get_patch_info`] (exact → unique range → wildcard).
/// 3. On a match, append `(patch_hash=<hash>)` and record the matched
///    key on `ctx.applied_patches` so the post-walk
///    `ERR_PNPM_UNUSED_PATCH` check sees the hit.
///
/// Packages whose resolver didn't supply [`pnpm_resolving_resolver_base::ResolveResult::name_ver`]
/// use the manifest's `name` and, for non-directory resolutions, its
/// `version`. Local directories remain linked rather than patched, matching
/// the TypeScript CLI and the lockfile format, which omits their manifest
/// version. The lookup is skipped when either field is unavailable or no
/// patches are configured.
pub(super) async fn build_pkg_id_with_patch_hash(
    ctx: &TreeCtx,
    result: &pnpm_resolving_resolver_base::ResolveResult,
) -> Result<String, ResolveDependencyTreeError> {
    let raw_id = result.id.as_str();
    if let Some(target) = raw_id.strip_prefix("link:") {
        let relative_target =
            ctx.link_anchor.target_relative_to_lockfile_root(target).unwrap_or_else(|| {
                let target = std::path::Path::new(target);
                let absolute_target = if target.is_absolute() {
                    pnpm_fs::lexical_normalize(target)
                } else {
                    pnpm_fs::lexical_normalize(&ctx.base_opts.project_dir.join(target))
                };
                pathdiff::diff_paths(&absolute_target, &ctx.lockfile_dir)
                    .unwrap_or(absolute_target)
                    .display()
                    .to_string()
                    .replace('\\', "/")
            });
        let relative_target = if relative_target.is_empty() { "." } else { &relative_target };
        return Ok(format!("link:{relative_target}"));
    }
    // Resolvers that learn the name from the fetched manifest (git,
    // tarball, directory) leave `name_ver` unset. The `name` is read
    // from the manifest itself and the id is prefixed regardless — so a
    // `file:project-1` id becomes `project-1@file:project-1`. Skipping
    // the prefix would leave `(` as the first paren-bearing character in
    // the downstream depPath, which `PkgNameVerPeer`'s `@`-split parser
    // can't recover from (it finds the `@` inside the peer suffix first).
    let manifest_name = result
        .manifest
        .as_ref()
        .and_then(|manifest| manifest.get("name"))
        .and_then(serde_json::Value::as_str);
    let manifest_version =
        (!matches!(result.resolution, pnpm_lockfile::LockfileResolution::Directory(_)))
            .then(|| {
                result
                    .manifest
                    .as_ref()
                    .and_then(|manifest| manifest.get("version"))
                    .and_then(serde_json::Value::as_str)
            })
            .flatten();
    let (name, version) = match (result.name_ver.as_ref(), manifest_name) {
        (Some(name_ver), _) => (name_ver.name.to_string(), name_ver.suffix.to_string()),
        (None, Some(name)) => (name.to_string(), manifest_version.unwrap_or_default().to_string()),
        (None, None) => return Ok(raw_id.to_string()),
    };
    let prefixed = if raw_id.starts_with(&format!("{name}@")) {
        raw_id.to_string()
    } else {
        format!("{name}@{raw_id}")
    };
    // `patched_dependencies` keys carry a `name@version` shape. Bail
    // out when the resolver and manifest both omitted the version so
    // the patch lookup doesn't run a `name@""` query.
    if version.is_empty() {
        return Ok(prefixed);
    }
    let Some(groups) = ctx.patched_dependencies.as_deref() else {
        return Ok(prefixed);
    };
    let Some(patch) = get_patch_info(Some(groups), &name, &version)? else {
        return Ok(prefixed);
    };
    lock_recoverable(&ctx.workspace.applied_patches).insert(patch.key.clone());
    Ok(format!("{prefixed}(patch_hash={})", patch.hash))
}

/// Extract `dependencies` + `optionalDependencies` from a resolved
/// package's manifest. Peer dependencies are **not** walked as regular
/// edges here — they're hoisted to the importer level by the
/// [`fn@crate::resolve_importer`] orchestrator (which calls [`extend_tree`]
/// with the hoist-picker's output) so a peer ends up shared across
/// every consumer, not nested under each one.
///
/// Peers are still recorded on [`ResolvedPackage::peer_dependencies`]
/// (via [`extract_peer_dependencies`]) so the peer-resolution stage
/// can compute the correct depPath suffix once everything is walked.
///
/// Each entry carries `optional` and `injected` flags from the manifest.
/// The walker propagates `optional` through `current_is_optional` so
/// [`ResolvedPackage::optional`] reflects whether every path to the node
/// went through an optional edge. `injected` selects the hard-linked
/// `file:` resolution for a workspace dependency.
///
/// npm merges `optionalDependencies` into `dependencies` at publish
/// time, so registry manifests routinely list the same name in both.
///
/// Bundled names are dropped: npm ships them inside the package's own
/// tarball, so resolving them again would install a second copy the
/// package never loads.
///
/// [`extend_tree`]: super::extend_tree
/// [`ResolvedPackage::peer_dependencies`]: crate::ResolvedPackage::peer_dependencies
/// [`ResolvedPackage::optional`]: crate::ResolvedPackage::optional
pub(super) fn extract_children(
    result: &pnpm_resolving_resolver_base::ResolveResult,
) -> Result<Vec<ChildSpec>, ResolveDependencyTreeError> {
    let Some(manifest) = result.manifest.as_ref() else { return Ok(Vec::new()) };
    let parent = render_parent(result);
    let bundled = bundled_dependency_names(manifest);
    let mut out = Vec::new();
    collect_deps(manifest, "dependencies", false, &parent, &bundled, &mut out)?;
    let mut optional = Vec::new();
    collect_deps(manifest, "optionalDependencies", true, &parent, &bundled, &mut optional)?;
    if !optional.is_empty() {
        let dependency_positions: HashMap<String, usize> =
            out.iter().enumerate().map(|(index, (name, ..))| (name.clone(), index)).collect();
        for spec in optional {
            match dependency_positions.get(&spec.0) {
                Some(&index) => out[index].2 = true,
                None => out.push(spec),
            }
        }
    }
    for (name, specifier) in engines_runtime_dependencies(manifest, "engines", "dependencies") {
        out.push((name.to_string(), specifier, false, false));
    }
    out.sort_unstable();
    Ok(out)
}

/// The dependency names a manifest declares as bundled, read from
/// `bundledDependencies` with `bundleDependencies` as the fallback
/// spelling. `true` stands for "every entry in `dependencies`".
fn bundled_dependency_names(manifest: &Value) -> HashSet<&str> {
    let bundled = ["bundledDependencies", "bundleDependencies"]
        .into_iter()
        .find_map(|key| manifest.get(key).filter(|value| !value.is_null()));
    match bundled {
        Some(Value::Bool(true)) => manifest
            .get("dependencies")
            .and_then(Value::as_object)
            .map(|map| map.keys().map(String::as_str).collect())
            .unwrap_or_default(),
        Some(Value::Array(names)) => names.iter().filter_map(Value::as_str).collect(),
        _ => HashSet::default(),
    }
}

/// Dependency names are validated before the bundled filter runs, so a
/// manifest with an unusable alias is rejected whether or not the
/// package bundles that alias.
fn collect_deps(
    manifest: &Value,
    key: &str,
    optional: bool,
    parent: &str,
    bundled: &HashSet<&str>,
    out: &mut Vec<ChildSpec>,
) -> Result<(), ResolveDependencyTreeError> {
    let Some(map) = manifest.get(key).and_then(Value::as_object) else { return Ok(()) };
    for (name, range) in map {
        if let Some(range_str) = range.as_str() {
            if !crate::is_valid_dependency_alias(name) {
                return Err(ResolveDependencyTreeError::InvalidDependencyName {
                    parent: parent.to_string(),
                    alias: name.clone(),
                });
            }
            if bundled.contains(name.as_str()) {
                continue;
            }
            out.push((
                name.clone(),
                range_str.to_string(),
                optional,
                dependency_is_injected(manifest, name),
            ));
        }
    }
    Ok(())
}

fn render_parent(result: &pnpm_resolving_resolver_base::ResolveResult) -> String {
    if let Some(name_ver) = result.name_ver.as_ref() {
        format!(r#"Package "{}@{}""#, name_ver.name, name_ver.suffix)
    } else {
        format!(r#"Package "{}""#, result.id)
    }
}

/// Extract `peerDependencies` from a resolved package's manifest, with
/// `peerDependenciesMeta[name].optional` folded onto each entry. The
/// package's own name plus names also present in `dependencies` or
/// `optionalDependencies` are skipped because those edges already
/// supply the same package directly — except for the `peer_shadowed`
/// names, whose own-dependency edge the walk drops in favor of the peer
/// (see [`peer_shadowed_dependencies`]). A `peerDependenciesMeta` entry
/// without a matching `peerDependencies` entry only counts when
/// `optional: true` — it is treated as an optional `"*"` peer exactly
/// like an explicitly declared one, and non-optional meta-only entries
/// are ignored. When [`Catalogs`] are supplied, `catalog:` ranges are
/// dereferenced before they enter peer resolution.
///
/// [`peer_shadowed_dependencies`]: crate::parent_pkg_aliases::peer_shadowed_dependencies
pub(super) fn extract_peer_dependencies(
    result: &pnpm_resolving_resolver_base::ResolveResult,
    peer_shadowed: &HashSet<String>,
    catalogs: Option<&Catalogs>,
) -> Result<BTreeMap<String, PeerDep>, ResolveDependencyTreeError> {
    let Some(manifest) = result.manifest.as_ref() else { return Ok(BTreeMap::new()) };
    let mut peers: BTreeMap<String, PeerDep> = BTreeMap::new();

    let dep_names = |key| {
        manifest.get(key).and_then(Value::as_object).into_iter().flat_map(|map| map.keys().cloned())
    };
    // Only `dependencies` are shadowed, so an entry that is *also* an
    // optional dependency still supplies the name itself.
    let mut own_deps: HashSet<String> = dep_names("dependencies")
        .filter(|name| !peer_shadowed.contains(name))
        .chain(dep_names("optionalDependencies"))
        .collect();
    if let Some(name) = manifest.get("name").and_then(Value::as_str) {
        own_deps.insert(name.to_string());
    }

    if let Some(map) = manifest.get("peerDependencies").and_then(Value::as_object) {
        for (name, range) in map {
            if own_deps.contains(name) {
                continue;
            }
            if let Some(range_str) = range.as_str() {
                let version = match catalogs {
                    Some(catalogs) => {
                        resolve_catalog_specifier(name.clone(), range_str.to_string(), catalogs)?.1
                    }
                    None => range_str.to_string(),
                };
                peers.insert(name.clone(), PeerDep { version, optional: false });
            }
        }
    }

    if let Some(meta) = manifest.get("peerDependenciesMeta").and_then(Value::as_object) {
        for (name, info) in meta {
            if own_deps.contains(name)
                || info.get("optional").and_then(Value::as_bool) != Some(true)
            {
                continue;
            }
            peers
                .entry(name.clone())
                .and_modify(|entry| entry.optional = true)
                .or_insert_with(|| PeerDep { version: "*".to_string(), optional: true });
        }
    }

    Ok(peers)
}

/// `true` when the package has no `dependencies`, `optionalDependencies`,
/// `peerDependencies`, or `peerDependenciesMeta`.
///
/// Conservatively returns `false` when the manifest is missing —
/// collapsing onto a leaf `NodeId` would claim knowledge of children
/// there is none of. Resolutions reaching here always carry one, real
/// or synthesized by `walk::fallback_manifest`.
pub(super) fn pkg_is_leaf(result: &pnpm_resolving_resolver_base::ResolveResult) -> bool {
    let Some(manifest) = result.manifest.as_ref() else { return false };
    is_empty_or_absent(manifest.get("dependencies"))
        && is_empty_or_absent(manifest.get("optionalDependencies"))
        && is_empty_or_absent(manifest.get("peerDependencies"))
        && is_empty_or_absent(manifest.get("peerDependenciesMeta"))
}

fn is_empty_or_absent(value: Option<&Value>) -> bool {
    value.and_then(Value::as_object).is_none_or(serde_json::Map::is_empty)
}

/// Emits a [`Deprecation`] when a newly-resolved package's manifest carries
/// a non-empty `deprecated` field not covered by `allowedDeprecatedVersions`.
pub(super) fn emit_deprecation_if_needed(
    ctx: &TreeCtx,
    result: &pnpm_resolving_resolver_base::ResolveResult,
    id: &str,
    depth: i32,
) {
    let Some(deprecated) = extract_deprecated_from_manifest(result.manifest.as_deref()) else {
        return;
    };
    if deprecated.is_empty() {
        return;
    }
    let Some((pkg_name, pkg_version)) = deprecated_pkg_name_ver(result) else {
        return;
    };
    if is_deprecation_allowed(&pkg_name, &pkg_version, &ctx.workspace.allowed_deprecated_versions) {
        return;
    }
    let Some(log) = ctx.workspace.deprecation_log.as_ref() else {
        return;
    };
    log(Deprecation {
        pkg_name,
        pkg_version,
        pkg_id: id.to_string(),
        prefix: ctx.base_opts.project_dir.display().to_string(),
        deprecated,
        depth,
    });
}

/// A missing manifest, an absent `deprecated` field, and a non-string
/// one all count as not deprecated.
fn extract_deprecated_from_manifest(manifest: Option<&Value>) -> Option<String> {
    manifest?.get("deprecated")?.as_str().map(str::to_string)
}

/// The name/version a `pnpm:deprecation` payload reports:
/// [`ResolveResult::name_ver`] when the resolver filled it, otherwise
/// the manifest's `name`/`version` — the canonical source for non-npm
/// resolutions (see the `name_ver` field doc). `None`, suppressing the
/// warning, when neither carries both fields.
///
/// [`ResolveResult::name_ver`]: pnpm_resolving_resolver_base::ResolveResult::name_ver
fn deprecated_pkg_name_ver(
    result: &pnpm_resolving_resolver_base::ResolveResult,
) -> Option<(String, String)> {
    if let Some(nv) = result.name_ver.as_ref() {
        return Some((nv.name.to_string(), nv.suffix.to_string()));
    }
    let manifest = result.manifest.as_deref()?;
    let name = manifest.get("name")?.as_str()?;
    let version = manifest.get("version")?.as_str()?;
    Some((name.to_string(), version.to_string()))
}

/// Whether `pkg_name@pkg_version` satisfies its entry in the
/// `pnpm.allowedDeprecatedVersions` map, matching the TS check at
/// `pnpm11/installing/deps-resolver/src/resolveDependencies.ts:2122`.
fn is_deprecation_allowed(
    pkg_name: &str,
    pkg_version: &str,
    allowed_deprecated_versions: &BTreeMap<String, String>,
) -> bool {
    let Some(range_str) = allowed_deprecated_versions.get(pkg_name) else {
        return false;
    };
    let Ok(range) = range_str.parse::<node_semver::Range>() else {
        return false;
    };
    let Ok(version) = pkg_version.parse::<node_semver::Version>() else {
        return false;
    };
    range.satisfies(&version)
}

/// Provenance tags that count as non-exotic for `blockExoticSubdeps`.
const NON_EXOTIC_RESOLVED_VIA: &[&str] = &[
    "custom-resolver",
    "github.com/denoland/deno",
    "github.com/oven-sh/bun",
    "jsr-registry",
    "local-filesystem",
    "named-registry",
    "nodejs.org",
    "npm-registry",
    "workspace",
];

pub(super) fn is_exotic_resolved_via(resolved_via: &str) -> bool {
    !NON_EXOTIC_RESOLVED_VIA.contains(&resolved_via)
}

#[cfg(test)]
mod tests;
