use node_semver::{Range, Version};
use pacquet_package_manifest::{DependencyGroup, PackageManifest, PackageManifestError};
use pacquet_registry::PinnedVersion;
use serde_json::{Map, Value};

/// pnpm's `DEPENDENCIES_FIELDS`, in its canonical order. A direct dependency
/// is written to exactly one of these and removed from the other two.
const DEPENDENCIES_FIELDS: [&str; 3] = ["optionalDependencies", "dependencies", "devDependencies"];

/// pnpm's `DEPENDENCIES_OR_PEER_FIELDS`: the three dependency fields plus
/// `peerDependencies`. `guess_dependency_type` scans them in this order and
/// returns the first that already declares the alias.
const DEPENDENCIES_OR_PEER_FIELDS: [&str; 4] =
    ["optionalDependencies", "dependencies", "devDependencies", "peerDependencies"];

/// One manifest mutation request. Port of pnpm's `PackageSpecObject`.
///
/// `save_type` and `bare_specifier` together select the behaviour:
/// * `save_type` set → upsert into that field, deleting the alias from the
///   other dependency fields; `bare_specifier` is the spec to write, falling
///   back to the alias's existing spec when `None` (the path that preserves a
///   failed optional update). When neither a `bare_specifier` nor an existing
///   spec is found, the request is a no-op.
/// * `save_type` `None` but `bare_specifier` set → write into whichever field
///   already declares the alias (defaulting to `dependencies`), without
///   moving it between fields.
pub struct PackageSpecObject {
    pub alias: String,
    pub peer: bool,
    pub bare_specifier: Option<String>,
    pub resolved_version: Option<String>,
    pub pinned_version: Option<PinnedVersion>,
    pub save_type: Option<DependencyGroup>,
}

/// Apply `specs` to `manifest` in memory, mirroring pnpm's
/// [`updateProjectManifestObject`](https://github.com/pnpm/pnpm/blob/fc2f33912e/pkg-manifest/utils/src/updateProjectManifestObject.ts).
///
/// Each entry either upserts a dependency into its `save_type` field (removing
/// the alias from the other dependency fields and, when `peer` is set, also
/// recording a peer range), or — with no `save_type` — rewrites the spec in
/// whichever field already declares the alias. Empty specifiers are treated as
/// absent, matching pnpm's truthiness guard.
///
/// Errors when a dependency field that must be written is present but is not a
/// JSON object (e.g. `"dependencies": "oops"`), mirroring pnpm throwing on the
/// same input and [`PackageManifest::add_dependency`]. The mutation is applied
/// atomically — a copy is mutated and committed only on success — so an error
/// mid-way leaves the manifest untouched rather than partially updated.
pub fn update_project_manifest_object(
    manifest: &mut PackageManifest,
    specs: &[PackageSpecObject],
) -> Result<(), PackageManifestError> {
    // Nothing to write: skip the clone + write-back the atomic path would do.
    if specs.is_empty() {
        return Ok(());
    }
    let mut root = manifest.value().clone();
    for spec in specs {
        if let Some(save_type) = spec.save_type {
            let field: &str = save_type.into();
            let resolved_spec = spec
                .bare_specifier
                .clone()
                .or_else(|| find_spec(&spec.alias, &root))
                .filter(|spec| !spec.is_empty());
            let Some(spec_str) = resolved_spec else { continue };
            define_dep_entry(&mut root, field, &spec.alias, &spec_str)?;
            for dep_field in DEPENDENCIES_FIELDS {
                if dep_field != field {
                    delete_dep_entry(&mut root, dep_field, &spec.alias);
                }
            }
            if spec.peer {
                let peer_spec = get_peer_specifier(
                    &spec_str,
                    spec.resolved_version.as_deref(),
                    spec.pinned_version,
                );
                define_dep_entry(&mut root, "peerDependencies", &spec.alias, &peer_spec)?;
            }
        } else if let Some(bare_specifier) = spec.bare_specifier.as_deref()
            && !bare_specifier.is_empty()
        {
            let used = guess_dependency_type(&spec.alias, &root).unwrap_or("dependencies");
            if used != "peerDependencies" {
                define_dep_entry(&mut root, used, &spec.alias, bare_specifier)?;
            }
        }
    }
    *manifest.value_mut() = root;
    Ok(())
}

/// The peer range to record alongside a saved dependency: keep the saved spec
/// when it is itself a valid peer range, otherwise derive a range from the
/// resolved version, falling back to `*` when no version is available (git /
/// tarball deps).
fn get_peer_specifier(
    spec: &str,
    resolved_version: Option<&str>,
    pinned_version: Option<PinnedVersion>,
) -> String {
    if is_valid_peer_range(spec) {
        return spec.to_string();
    }
    resolved_version
        .and_then(|version| create_version_spec_from_resolved_version(version, pinned_version))
        .unwrap_or_else(|| "*".to_string())
}

/// Build a manifest range from a concrete resolved version and a pin operator,
/// mirroring pnpm's `createVersionSpecFromResolvedVersion`: a prerelease is
/// pinned exactly, otherwise the [`PinnedVersion`] operator is prepended.
/// Returns `None` when `resolved_version` is not valid semver.
fn create_version_spec_from_resolved_version(
    resolved_version: &str,
    pinned_version: Option<PinnedVersion>,
) -> Option<String> {
    let parsed = Version::parse(resolved_version).ok()?;
    if !parsed.pre_release.is_empty() {
        return Some(resolved_version.to_string());
    }
    let prefix = pinned_version.unwrap_or(PinnedVersion::Major).range_prefix();
    Some(format!("{prefix}{resolved_version}"))
}

/// Whether `version` is acceptable as a `peerDependencies` range: a valid
/// semver range, or a `workspace:` / `catalog:` reference. Mirrors pnpm's
/// `isValidPeerRange`, which uses `includes` so the protocol can appear inside
/// a wider range expression.
fn is_valid_peer_range(version: &str) -> bool {
    Range::parse(version).is_ok() || version.contains("workspace:") || version.contains("catalog:")
}

/// The alias's currently-declared spec, looked up in the first field that
/// declares it. `None` when no field declares the alias.
fn find_spec(alias: &str, root: &Value) -> Option<String> {
    let field = guess_dependency_type(alias, root)?;
    root.get(field)?.get(alias)?.as_str().map(ToString::to_string)
}

/// The first of pnpm's dependency-or-peer fields that already declares `alias`
/// with a string spec. Mirrors pnpm's `guessDependencyType`.
fn guess_dependency_type(alias: &str, root: &Value) -> Option<&'static str> {
    DEPENDENCIES_OR_PEER_FIELDS.into_iter().find(|field| {
        root.get(*field)
            .and_then(Value::as_object)
            .and_then(|deps| deps.get(alias))
            .is_some_and(Value::is_string)
    })
}

fn define_dep_entry(
    root: &mut Value,
    field: &str,
    alias: &str,
    value: &str,
) -> Result<(), PackageManifestError> {
    let Some(obj) = root.as_object_mut() else {
        return Err(PackageManifestError::InvalidAttribute(
            "the manifest root must be an object".to_string(),
        ));
    };
    // Mirror pnpm's `manifest[field] = manifest[field] ?? {}`: a missing or
    // `null` field becomes a fresh object before the entry is written.
    let deps = obj.entry(field).or_insert_with(|| Value::Object(Map::new()));
    if deps.is_null() {
        *deps = Value::Object(Map::new());
    }
    let Some(deps) = deps.as_object_mut() else {
        return Err(PackageManifestError::InvalidAttribute(format!(
            "the {field} field must be an object",
        )));
    };
    deps.insert(alias.to_string(), Value::String(value.to_string()));
    Ok(())
}

fn delete_dep_entry(root: &mut Value, field: &str, alias: &str) {
    if let Some(deps) = root.get_mut(field).and_then(Value::as_object_mut) {
        deps.remove(alias);
    }
}

#[cfg(test)]
mod tests;
