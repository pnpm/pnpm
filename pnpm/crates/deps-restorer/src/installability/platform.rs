use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_package_is_installable::{
    InstallabilityError, InstallabilityOptions, PackageInstallabilityManifest,
    SupportedArchitectures, WantedEngine, WantedPlatformRef, check_package, inferred_platform,
};
use pnpm_resolving_resolver_base::ResolveResult;
use serde_json::Value;
use std::{borrow::Cow, collections::HashMap};

/// Host context for the installability check. Built once per install
/// so the per-snapshot calls don't each re-spawn `node --version`
/// or re-read `std::env::consts::OS`.
pub struct InstallabilityHost {
    pub node_version: String,
    /// `true` when `node_version` was discovered by spawning
    /// `node --version`; `false` when the field carries the synthetic
    /// fallback. The side-effects-cache key derives from this — a
    /// fallback version must not seed the cache because subsequent
    /// installs would key on the actual node major and miss every
    /// row written under the fallback.
    pub node_detected: bool,
    pub os: &'static str,
    pub cpu: &'static str,
    pub libc: &'static str,
    pub supported_architectures: Option<SupportedArchitectures>,
    pub engine_strict: bool,
}
impl InstallabilityHost {
    /// Resolve the host context from the running process.
    ///
    /// `node_version` is detected via
    /// [`pnpm_graph_hasher::detect_node_version`]; when detection
    /// fails (no `node` on PATH), pacquet falls back to a synthetic
    /// `99999.0.0` so `engines.node` ranges keep accepting packages.
    /// The alternative `0.0.0` would falsely-skip every optional
    /// dependency targeting any concrete node range, which is worse
    /// than the over-acceptance the very-high fallback produces.
    /// `node_detected` records which path was taken so callers can
    /// suppress side-effects-cache lookups when the version is
    /// synthetic. [`Self::detect_with`] overrides both the version
    /// (the `nodeVersion` setting) and the engine-strict policy.
    #[must_use]
    pub fn detect() -> Self {
        let detected = pnpm_graph_hasher::detect_node_version();
        let node_detected = detected.is_some();
        let node_version = detected.unwrap_or_else(|| "99999.0.0".to_string());
        Self {
            node_version,
            node_detected,
            os: pnpm_graph_hasher::host_platform(),
            cpu: pnpm_graph_hasher::host_arch(),
            libc: pnpm_graph_hasher::host_libc(),
            supported_architectures: None,
            engine_strict: false,
        }
    }

    /// Build the host context with a caller-supplied engine-strict policy and
    /// optional Node.js version override (the `engineStrict` / `nodeVersion`
    /// config settings).
    ///
    /// An explicit `node_version` is authoritative: no `node --version` probe
    /// runs and `node_detected` is `true`, so the side-effects cache keys off
    /// the pinned major exactly as it would off a detected one. A leading `v`
    /// (as in `process.version` / `node --version`, e.g. `v22.11.0`) is
    /// stripped so the value parses as exact semver, matching the auto-detect
    /// path. `None` falls back to [`Self::detect`], then overrides
    /// `engine_strict`.
    #[must_use]
    pub fn detect_with(engine_strict: bool, node_version: Option<String>) -> Self {
        match node_version.map(|version| normalize_node_version(&version)) {
            Some(node_version) => Self {
                node_version,
                node_detected: true,
                os: pnpm_graph_hasher::host_platform(),
                cpu: pnpm_graph_hasher::host_arch(),
                libc: pnpm_graph_hasher::host_libc(),
                supported_architectures: None,
                engine_strict,
            },
            None => Self { engine_strict, ..Self::detect() },
        }
    }
}
/// Canonicalize a Node.js version string for the engine check: trim surrounding
/// whitespace and drop a single leading `v` (`v22.11.0` → `22.11.0`) so a value
/// copied from `process.version` / `node --version` parses as exact semver.
pub(super) fn normalize_node_version(version: &str) -> String {
    let trimmed = version.trim();
    trimmed.strip_prefix('v').unwrap_or(trimmed).to_string()
}
pub fn check_installability(
    package_id: &str,
    manifest: &PackageInstallabilityManifest,
    options: &InstallabilityOptions<'_>,
) -> Result<Option<InstallabilityError>, Box<InstallabilityError>> {
    let manifest = if options.optional {
        manifest_with_inferred_platform(manifest)
    } else {
        Cow::Borrowed(manifest)
    };
    check_package(package_id, manifest.as_ref(), options)
        .map_err(|invalid| Box::new(InstallabilityError::InvalidNodeVersion(invalid)))
}
#[must_use]
pub fn manifest_with_inferred_platform(
    manifest: &PackageInstallabilityManifest,
) -> Cow<'_, PackageInstallabilityManifest> {
    let Some(platform) = inferred_platform(
        &manifest.name,
        WantedPlatformRef {
            os: manifest.os.as_deref(),
            cpu: manifest.cpu.as_deref(),
            libc: manifest.libc.as_deref(),
        },
    ) else {
        return Cow::Borrowed(manifest);
    };
    Cow::Owned(PackageInstallabilityManifest {
        name: manifest.name.clone(),
        engines: manifest.engines.clone(),
        os: platform.os,
        cpu: platform.cpu,
        libc: platform.libc,
    })
}
#[must_use]
pub fn platform_manifest_from_resolve_result(
    result: &ResolveResult,
    fallback_alias: Option<&str>,
) -> PackageInstallabilityManifest {
    let manifest = result.manifest.as_deref();
    PackageInstallabilityManifest {
        name: result
            .name_ver
            .as_ref()
            .map(|name_ver| name_ver.name.to_string())
            .or_else(|| {
                manifest
                    .and_then(|manifest| manifest.get("name"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .or_else(|| result.alias.clone())
            .or_else(|| fallback_alias.map(ToString::to_string))
            .unwrap_or_default(),
        engines: None,
        cpu: read_string_list(manifest, "cpu"),
        os: read_string_list(manifest, "os"),
        libc: read_string_list(manifest, "libc"),
    }
}
pub(super) fn read_string_list(manifest: Option<&Value>, key: &str) -> Option<Vec<String>> {
    let value = manifest?.get(key)?;
    let out: Vec<String> = match value {
        Value::String(value) => vec![value.clone()],
        Value::Array(items) => {
            items.iter().filter_map(Value::as_str).map(ToString::to_string).collect()
        }
        _ => Vec::new(),
    };
    (!out.is_empty()).then_some(out)
}
/// True if any package metadata row in the lockfile declares an
/// `engines` / `cpu` / `os` / `libc` constraint pacquet would need
/// to evaluate, or any optional snapshot's package name infers a
/// platform constraint its metadata row doesn't declare.
/// Short-circuits on the first hit. When this returns false, both
/// [`compute_skipped_snapshots`](crate::installability::compute_skipped_snapshots) and the caller can short-circuit:
/// no need to spawn `node --version` or build the host context,
/// because the verdict is unconditionally an empty skip set.
///
/// `pub` so `install_frozen_lockfile` can gate the host detection
/// on it — the spawn is otherwise on the critical path of
/// `CreateVirtualStore::run` and serializes ~100ms of node-binary
/// startup with extraction it used to overlap with.
pub fn any_installability_constraint(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> bool {
    packages.values().any(metadata_has_meaningful_constraint)
        || snapshots.iter().any(|(snapshot_key, snapshot)| {
            snapshot.optional && {
                let metadata_key = snapshot_key.without_peer();
                packages.get(&metadata_key).is_some_and(|metadata| {
                    inferred_platform(
                        metadata_key.name.bare.as_str(),
                        WantedPlatformRef {
                            os: metadata.os.as_deref(),
                            cpu: metadata.cpu.as_deref(),
                            libc: metadata.libc.as_deref(),
                        },
                    )
                    .is_some()
                })
            }
        })
}
#[must_use]
pub fn any_optional_installability_constraint(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> bool {
    snapshots.iter().any(|(snapshot_key, snapshot)| {
        if !snapshot.optional {
            return false;
        }
        let metadata_key = snapshot_key.without_peer();
        packages.get(&metadata_key).is_some_and(|metadata| {
            metadata_has_meaningful_constraint(metadata)
                || inferred_platform(
                    metadata_key.name.bare.as_str(),
                    WantedPlatformRef {
                        os: metadata.os.as_deref(),
                        cpu: metadata.cpu.as_deref(),
                        libc: metadata.libc.as_deref(),
                    },
                )
                .is_some()
        })
    })
}
/// True if a single metadata row carries a constraint pacquet would
/// actually evaluate.
pub(super) fn metadata_has_meaningful_constraint(metadata: &PackageMetadata) -> bool {
    let engines_meaningful = metadata
        .engines
        .as_ref()
        .is_some_and(|engines| engines.contains_key("node") || engines.contains_key("pnpm"));
    engines_meaningful
        || platform_axis_meaningful(metadata.cpu.as_deref())
        || platform_axis_meaningful(metadata.os.as_deref())
        || platform_axis_meaningful(metadata.libc.as_deref())
}
/// One axis of `cpu` / `os` / `libc` carries no constraint when the
/// list is absent, empty, or exactly the `["any"]` sentinel that
/// `check_list` short-circuits as "accept everything".
pub(super) fn platform_axis_meaningful(axis: Option<&[String]>) -> bool {
    match axis {
        None | Some([]) => false,
        Some([only]) if only == "any" => false,
        Some(_) => true,
    }
}
pub(super) fn manifest_from_metadata(
    metadata_key: &PackageKey,
    metadata: &PackageMetadata,
) -> PackageInstallabilityManifest {
    PackageInstallabilityManifest {
        name: metadata_key.name.to_string(),
        engines: metadata.engines.as_ref().map(|map| WantedEngine {
            node: map.get("node").cloned(),
            pnpm: map.get("pnpm").cloned(),
        }),
        cpu: metadata.cpu.clone(),
        os: metadata.os.clone(),
        libc: metadata.libc.as_deref().map(<[String]>::to_vec),
    }
}
