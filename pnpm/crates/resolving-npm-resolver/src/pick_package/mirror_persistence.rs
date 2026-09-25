use super::{
    DateTime, Diagnostic, Display, Error, MetadataCacheScope, Mutex, Package, Path,
    PickPackageError, Utc, get_pkg_mirror_path, save_meta_indexed,
};

/// The in-memory cache + fetch-lock key for a `(registry, package)` pick,
/// namespaced by its [`MetadataCacheScope`].
///
/// A [`MetadataCacheScope::Public`] route keeps the plain
/// `{registry}\x00{name}` key (with the `:full` / `:full:filtered`
/// suffix), so the CLI and public routes are unchanged. A private route
/// prepends its descriptor namespace so one caller's private packument
/// can't satisfy another caller's pick for the same name.
///
/// The registry is part of the key because the same package name can live
/// in two registries (a public `lodash` and a private one); pacquet
/// shares one cache across every pick, so the registry has to be in the
/// key to scope picks per registry. The
/// full-mode suffix keeps a later `optional` pick from reusing an
/// abbreviated entry that dropped `libc`/`cpu`/`os`.
pub(super) fn metadata_cache_key(
    scope: &MetadataCacheScope,
    registry: &str,
    name: &str,
    full_metadata: bool,
    use_filtered_full_metadata: bool,
) -> String {
    let suffix = if full_metadata {
        if use_filtered_full_metadata { ":full:filtered" } else { ":full" }
    } else {
        ""
    };
    match scope {
        MetadataCacheScope::Public => format!("{registry}\x00{name}{suffix}"),
        MetadataCacheScope::Private { descriptor_id } => {
            format!("private\x00{descriptor_id}\x00{registry}\x00{name}{suffix}")
        }
    }
}

pub(super) fn validate_package_name(pkg_name: &str) -> Result<(), PickPackageError> {
    // A slash without a `@scope/` prefix is structurally invalid.
    if pkg_name.contains('/') && !pkg_name.starts_with('@') {
        return Err(PickPackageError::InvalidPackageName { pkg_name: pkg_name.to_string() });
    }
    Ok(())
}

pub(super) fn get_file_mtime(path: &Path) -> Option<DateTime<Utc>> {
    let metadata = std::fs::metadata(path).ok()?;
    let mtime: chrono::DateTime<Utc> = metadata.modified().ok()?.into();
    Some(mtime)
}

/// Time-dependent check a packument with no per-version `time` map
/// takes down. `minimumReleaseAge` and `trustPolicy` both read that one
/// field, so a registry that strips it disables both, and each names
/// itself in the warning it emits.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SkippedTimeCheck {
    #[display("minimumReleaseAge")]
    MinimumReleaseAge,
    #[display("trustPolicy")]
    TrustPolicy,
}

/// Bounded set of `(package name, skipped check)` pairs we've already
/// warned about for the missing-`time` field. Capped at 1024 entries to
/// keep long-lived processes (daemons, store servers) from leaking
/// memory through it.
///
/// Keyed by the pair rather than the package name alone: when both
/// checks are configured they go dark together, and a package-only key
/// would let whichever check ran first silence the other's warning,
/// leaving the user told about only one of the two skips.
///
/// `IndexSet` (not `Vec`) gives O(1) `contains` + cheap insertion-
/// ordered eviction via `shift_remove_index(0)`.
pub(super) const MAX_WARNED_MISSING_TIME: usize = 1024;

pub(super) static WARNED_MISSING_TIME: std::sync::LazyLock<
    Mutex<indexmap::IndexSet<(String, SkippedTimeCheck)>>,
> = std::sync::LazyLock::new(|| Mutex::new(indexmap::IndexSet::new()));

pub(crate) fn warn_missing_time_once(pkg_name: &str, skipped_check: SkippedTimeCheck) {
    let mut warned = WARNED_MISSING_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (pkg_name.to_string(), skipped_check);
    if warned.contains(&key) {
        return;
    }
    if warned.len() >= MAX_WARNED_MISSING_TIME {
        // IndexSet preserves insertion order; drop the oldest entry
        // (index 0) so the bound stays at MAX_WARNED_MISSING_TIME.
        warned.shift_remove_index(0);
    }
    warned.insert(key);
    tracing::warn!(
        target: "pnpm_resolving_npm_resolver::pick_package",
        pkg_name,
        r#"The metadata of {pkg_name} is missing the "time" field; skipping the {skipped_check} check for this package."#,
    );
}

/// Convenience writer: persist `meta` to the on-disk mirror under
/// `<cache_dir>/<meta_dir>/<registry>/<encoded-pkg>.jsonl`. Pass
/// [`crate::mirror::FULL_META_DIR`] when seeding the full-metadata cache (verifier
/// tests, integrated benchmark) and [`crate::mirror::ABBREVIATED_META_DIR`] when
/// seeding the abbreviated cache (resolver tests). Errors are logged
/// at debug by the install path — a cache-write failure should never
/// fail an install. Kept public so the rare caller that
/// constructs a `Package` outside the fetcher (test fixtures, the
/// integrated benchmark's pre-warmer) can seed the mirror without
/// reaching into `crate::mirror`.
pub fn persist_meta_to_mirror(
    cache_dir: &Path,
    meta_dir: &str,
    registry: &str,
    meta: &Package,
) -> Result<(), MirrorPersistError> {
    let path = get_pkg_mirror_path(cache_dir, meta_dir, registry, &meta.name)
        .map_err(|error| MirrorPersistError::EncodePath { error: error.to_string() })?;
    save_meta_indexed(&path, meta, meta.etag.as_deref())
        .map_err(|error| MirrorPersistError::Write { error: error.to_string() })
}

/// Failure modes for [`persist_meta_to_mirror`]. Each variant
/// carries the underlying error as a string because the underlying
/// sources are heterogeneous (`io::Error`, `serde_json::Error`,
/// `EncodeRegistryError`) and the caller only logs.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum MirrorPersistError {
    #[display("Failed to encode mirror path: {error}")]
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_PICK_PACKAGE_ENCODE_PATH))]
    EncodePath {
        #[error(not(source))]
        error: String,
    },
    #[display("Failed to serialize mirror entry: {error}")]
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_PICK_PACKAGE_SERIALIZE))]
    Serialize {
        #[error(not(source))]
        error: String,
    },
    #[display("Failed to write mirror entry: {error}")]
    #[diagnostic(code(ERR_PNPM_RESOLVING_NPM_RESOLVER_PICK_PACKAGE_WRITE))]
    Write {
        #[error(not(source))]
        error: String,
    },
}
