use super::{Ecosystem, IndexMap, PackageAccess, PackagePattern, RegistryError};

pub(in super::super) fn validate_registry_key(key: &str) -> Result<(), RegistryError> {
    if let Some((prefix, name)) = key.split_once('/')
        && Ecosystem::all().any(|ecosystem| ecosystem.as_str() == prefix)
    {
        return validate_registry_name(name);
    }
    validate_registry_name(key)
}

/// A registry's `packages:` keys in the spelling its ecosystem matches
/// requests by. Rejects invalid patterns and duplicate canonical keys.
pub(in super::super) fn ecosystem_package_keys(
    registry: &str,
    ecosystem: Ecosystem,
    packages: IndexMap<String, Option<PackageAccess>>,
) -> Result<IndexMap<String, (PackagePattern, Option<PackageAccess>)>, RegistryError> {
    let mut normalized = IndexMap::new();
    for (key, access) in packages {
        // Through the pattern language, so a wildcard shape is normalized as
        // itself rather than failing the ecosystem's name rules.
        let pattern = PackagePattern::parse(&key, ecosystem)
            .map_err(|error| RegistryError::InvalidConfig {
                reason: format!("{ecosystem} registry {registry:?} `packages:` key: {error}"),
            })?;
        let normalized_key = pattern.to_string();
        if normalized.contains_key(&normalized_key) {
            return Err(RegistryError::InvalidConfig {
                reason: format!(
                    "{ecosystem} registry {registry:?} `packages:` key {key:?} duplicates normalized key {normalized_key:?}",
                ),
            });
        }
        normalized.insert(normalized_key, (pattern, access));
    }
    Ok(normalized)
}

/// Two hosted registries of the same ecosystem sharing a `namespace` would read and
/// write the same storage namespace, so a package published to one would
/// surface through the other — breaking the declared-provenance isolation.
/// Rejected at load, whether the config came from YAML or an embedder.
pub(in super::super) fn namespace_collision_error(
    name: &str,
    namespace: &str,
    other: &str,
) -> RegistryError {
    RegistryError::InvalidConfig {
        reason: format!(
            "hosted registry {name:?} reuses the `namespace` {namespace:?} already claimed by \
             registry {other:?}; two hosted registries cannot share a namespace",
        ),
    }
}

/// A registry's name is the URL path segment `/~<name>/` and the prefix of a
/// registry-qualified lockfile key (`<pkg>@<name>:<version>`), so it follows
/// the grammar of that prefix.
pub(in super::super) fn validate_registry_name(name: &str) -> Result<(), RegistryError> {
    if !pnpm_deps_path::is_well_formed_registry_name(name) {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "registry name {name:?} is invalid: a registry name must start with a letter and \
                 contain only letters, digits, `.`, `_`, and `-`",
            ),
        });
    }
    if pnpm_deps_path::shadows_reserved_version_prefix(name) {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "registry name {name:?} is invalid: it is a reserved dependency specifier prefix",
            ),
        });
    }
    Ok(())
}

/// An artifact owner's name is one URL path segment and one storage path
/// segment. A name that can't survive that round trip is rejected at load:
/// `/` splits it across segments, `?`, `#`, `%`, whitespace, and control
/// characters make the URL ambiguous, and `.`, `..`, or a Windows drive prefix
/// are path components intermediaries may normalize away.
pub(in super::super) fn validate_artifact_owner_name(name: &str) -> Result<(), RegistryError> {
    let safe = pnpr_package_name::is_safe_path_segment(name)
        && !name.contains(['%', '?', '#'])
        && !name.contains(|ch: char| ch.is_whitespace() || ch.is_control());
    if safe {
        return Ok(());
    }
    Err(RegistryError::InvalidConfig {
        reason: format!(
            "artifact owner {name:?} is not a single URL-safe path segment: it cannot be empty, \
             `.` or `..`, start with `.`, or contain `/`, `\\`, `:`, `%`, `?`, `#`, whitespace, \
             or control characters",
        ),
    })
}

/// A hosted registry's `namespace` becomes a storage path/key segment (`Storage::for_hosted`),
/// so it must be empty (the default namespace) or one safe component under the same
/// rules as every other on-disk segment (no separators, traversal, leading
/// dot, or Windows drive prefix) — otherwise a crafted config could read or
/// write outside the storage root. The leading-dot rule also keeps a namespace
/// from aliasing the reserved dot-directories inside the storage root (the
/// default `.pnpr-cache` wipeable cache and the `.metadata` database), which
/// would put authoritative packages under a path an operator is told is safe
/// to delete.
pub(in super::super) fn validate_namespace(
    name: &str,
    namespace: &str,
) -> Result<(), RegistryError> {
    if namespace.is_empty() || pnpr_package_name::is_safe_path_segment(namespace) {
        return Ok(());
    }
    Err(RegistryError::InvalidConfig {
        reason: format!(
            "hosted registry {name:?} has an invalid `namespace` {namespace:?}: it must be a single path-safe \
             segment (no `/`, `\\`, `:`, leading `.`, or traversal)",
        ),
    })
}
