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
/// requests by. Crate names are compared lowercase and Python project names
/// normalized (PEP 503), so a `cargo` or `pypi` registry's exact-name keys
/// are canonicalized here, and a key no registry of that ecosystem could
/// serve is a config error. Only the catch-all `**` applies to every ecosystem.
pub(in super::super) fn ecosystem_package_keys(
    registry: &str,
    ecosystem: Ecosystem,
    packages: IndexMap<String, Option<PackageAccess>>,
) -> Result<IndexMap<String, Option<PackageAccess>>, RegistryError> {
    let mut normalized = IndexMap::new();
    for (key, access) in packages {
        // Through the pattern language, so a wildcard shape is normalized as
        // itself rather than failing the ecosystem's name rules.
        let normalized_key = PackagePattern::parse(&key, ecosystem)
            .map(|pattern| pattern.to_string())
            .map_err(|error| RegistryError::InvalidConfig {
                reason: format!("{ecosystem} registry {registry:?} `packages:` key: {error}"),
            })?;
        if normalized.contains_key(&normalized_key) {
            return Err(RegistryError::InvalidConfig {
                reason: format!(
                    "{ecosystem} registry {registry:?} `packages:` key {key:?} duplicates normalized key {normalized_key:?}",
                ),
            });
        }
        normalized.insert(normalized_key, access);
    }
    Ok(normalized)
}

/// Two hosted registries sharing an `org` would read and write the same
/// storage namespace, so a package published to one would surface through the
/// other — breaking the declared-provenance isolation. Rejected at load,
/// whether the config came from YAML or an embedder.
pub(in super::super) fn org_collision_error(name: &str, org: &str, other: &str) -> RegistryError {
    RegistryError::InvalidConfig {
        reason: format!(
            "hosted registry {name:?} reuses the `org` namespace {org:?} already claimed by \
             registry {other:?}; two hosted registries cannot share a namespace",
        ),
    }
}

/// A registry's name is addressed as the single URL path segment `/~<name>/` and
/// is embedded verbatim into rewritten `dist.tarball` URLs, so it must be one
/// URL-safe segment. A name that can't survive that round trip is rejected at
/// load rather than becoming an unreachable registry (`/` splits it across
/// segments), a URL-parsing ambiguity (`?`, `#`, `%`, whitespace, control
/// characters), or a path-meaningful component intermediaries may normalize
/// away (`.`, `..`, a Windows drive prefix).
pub(in super::super) fn validate_registry_name(name: &str) -> Result<(), RegistryError> {
    let safe = pnpr_package_name::is_safe_path_segment(name)
        && !name.contains(['%', '?', '#'])
        && !name.contains(|ch: char| ch.is_whitespace() || ch.is_control());
    if safe {
        return Ok(());
    }
    Err(RegistryError::InvalidConfig {
        reason: format!(
            "registry name {name:?} is not a single URL-safe path segment: it is served at \
             `/~<name>/`, so it cannot be empty, `.` or `..`, start with `.`, or contain `/`, \
             `\\`, `:`, `%`, `?`, `#`, whitespace, or control characters",
        ),
    })
}

/// A hosted registry's `org` becomes a storage path/key segment (`Storage::for_hosted`),
/// so it must be empty (the flat root) or one safe component under the same
/// rules as every other on-disk segment (no separators, traversal, leading
/// dot, or Windows drive prefix) — otherwise a crafted config could read or
/// write outside the storage root. The leading-dot rule also keeps an org
/// from aliasing the reserved dot-directories inside the storage root (the
/// default `.pnpr-cache` wipeable cache and the `.pnpr-journal` commit
/// journal), which would put authoritative packages under a path an operator
/// is told is safe to delete.
pub(in super::super) fn validate_org_namespace(name: &str, org: &str) -> Result<(), RegistryError> {
    if org.is_empty() || pnpr_package_name::is_safe_path_segment(org) {
        return Ok(());
    }
    Err(RegistryError::InvalidConfig {
        reason: format!(
            "hosted registry {name:?} has an invalid `org` {org:?}: it must be a single path-safe \
             segment (no `/`, `\\`, `:`, leading `.`, or traversal)",
        ),
    })
}
