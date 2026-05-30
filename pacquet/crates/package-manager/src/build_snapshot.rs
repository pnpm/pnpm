use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, ParsePkgVerPeerError, PkgName, PkgNameVerPeer,
    PkgVerPeer, RegistryResolution, SnapshotDepRef, SnapshotEntry,
};
use pacquet_registry::PackageVersion;
use std::collections::HashMap;

/// Result of converting a resolved [`PackageVersion`] into the v9 lockfile
/// shape: a `PackageKey` (used to index both `packages:` and `snapshots:`), the
/// per-version `PackageMetadata`, and the per-instance `SnapshotEntry`.
#[derive(Debug)]
pub struct BuiltSnapshot {
    pub package_key: PackageKey,
    pub metadata: PackageMetadata,
    pub snapshot: SnapshotEntry,
}

/// Error type of [`build_package_snapshot`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum BuildSnapshotError {
    #[display(
        "Package `{name}@{version}` was returned from the registry without an `integrity` field; cannot build a lockfile entry for it."
    )]
    #[diagnostic(code(pacquet_package_manager::build_snapshot::missing_integrity))]
    MissingIntegrity { name: String, version: String },

    #[display("Failed to parse package name `{name}`: {source}")]
    #[diagnostic(code(pacquet_package_manager::build_snapshot::parse_name))]
    ParseName {
        name: String,
        #[error(source)]
        source: pacquet_lockfile::ParsePkgNameError,
    },

    #[display(
        "Package `{name}` reported version `{version}` that cannot be parsed as a PkgVerPeer: {source}"
    )]
    #[diagnostic(code(pacquet_package_manager::build_snapshot::parse_version))]
    ParseVersion {
        name: String,
        version: String,
        #[error(source)]
        source: ParsePkgVerPeerError,
    },
}

/// Build the v9 lockfile `PackageKey` (name@version, no peer suffix) for a
/// package installed from the default registry.
pub fn registry_package_key(package: &PackageVersion) -> Result<PackageKey, BuildSnapshotError> {
    let name = PkgName::parse(package.name.as_str())
        .map_err(|source| BuildSnapshotError::ParseName { name: package.name.clone(), source })?;
    let version_string = package.version.to_string();
    let peer = version_string.parse::<PkgVerPeer>().map_err(|source| {
        BuildSnapshotError::ParseVersion {
            name: package.name.clone(),
            version: version_string,
            source,
        }
    })?;
    Ok(PkgNameVerPeer::new(name, peer))
}

/// Convert a [`PackageVersion`] into a v9 [`BuiltSnapshot`].
///
/// `resolved_dependencies` maps each of this package's declared dependency
/// names to the version-with-peer-suffix that was actually picked by the
/// resolver. Callers that install without peer resolution may pass empty peer
/// suffixes.
pub fn build_package_snapshot(
    package: &PackageVersion,
    resolved_dependencies: &HashMap<String, PkgVerPeer>,
) -> Result<BuiltSnapshot, BuildSnapshotError> {
    let package_key = registry_package_key(package)?;

    let integrity =
        package.dist.integrity.clone().ok_or_else(|| BuildSnapshotError::MissingIntegrity {
            name: package.name.clone(),
            version: package.version.to_string(),
        })?;

    let mut dependencies: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    for (dep_name, ver_peer) in resolved_dependencies {
        let parsed = PkgName::parse(dep_name.as_str())
            .map_err(|source| BuildSnapshotError::ParseName { name: dep_name.clone(), source })?;
        dependencies.insert(parsed, SnapshotDepRef::Plain(ver_peer.clone()));
    }

    let metadata = PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution { integrity }),
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    };

    let snapshot = SnapshotEntry {
        id: None,
        dependencies: (!dependencies.is_empty()).then_some(dependencies),
        optional_dependencies: None,
        transitive_peer_dependencies: None,
        patched: None,
        optional: false,
    };

    Ok(BuiltSnapshot { package_key, metadata, snapshot })
}

#[cfg(test)]
mod tests;
