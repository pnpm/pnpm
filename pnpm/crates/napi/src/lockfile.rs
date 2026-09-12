//! Reading, writing, and narrowing `pnpm-lock.yaml`.
//!
//! The lockfile format is byte-stable across pnpm's two stacks, so a host
//! *can* read it with `@pnpm/lockfile.fs` — but then it carries a v11
//! JavaScript parser that has to keep agreeing with a v12 engine's writer
//! about a file both of them own. These exports hand the host the engine's
//! own reader and writer instead.
//!
//! The shape crossing the boundary is the file's own: the same JSON the
//! YAML deserializes into, which is `LockfileFile` in
//! `@pnpm/lockfile.types` terms (each importer dependency an
//! `{ specifier, version }` pair, `packages` and `snapshots` separate).
//! There is no in-memory-only variant to convert to or from.
//!
//! Top-level keys pnpm does not define round-trip untouched, so a host that
//! records its own state beside the lockfile — Bit's `bit:` block — can
//! read, edit, and write the file back without losing it.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use napi_derive::napi;
use pnpm_lockfile::{FilterByImportersOptions, IncludedDependencies, Lockfile, PackageKey};

use crate::error::to_napi_error;

/// Which of the two lockfiles an operation addresses.
#[derive(Debug)]
enum LockfileKind {
    /// `<dir>/pnpm-lock.yaml` — what the workspace asks for.
    Wanted,
    /// `<modulesDir>/.pnpm/lock.yaml` — what the last install actually
    /// materialized.
    Current,
}

impl LockfileKind {
    fn parse(kind: Option<&str>) -> napi::Result<Self> {
        match kind {
            None | Some("wanted") => Ok(LockfileKind::Wanted),
            Some("current") => Ok(LockfileKind::Current),
            Some(other) => Err(napi::Error::from_reason(format!(
                r#"unknown lockfile kind {other:?}; expected "wanted" or "current""#,
            ))),
        }
    }
}

/// Inputs for [`read_lockfile`]. Mirrors [`ReadLockfileOptions`] in
/// `index.d.ts`.
#[napi(object)]
pub struct ReadLockfileOptions {
    /// Lockfile / workspace root directory.
    pub dir: String,
    /// `"wanted"` (the default) or `"current"`.
    pub kind: Option<String>,
    /// `node_modules` directory, which the current lockfile lives under.
    /// Defaults to `<dir>/node_modules`.
    pub modules_dir: Option<String>,
}

/// Inputs for [`write_lockfile`]. Mirrors [`WriteLockfileOptions`] in
/// `index.d.ts`.
#[napi(object)]
pub struct WriteLockfileOptions {
    /// Lockfile / workspace root directory.
    pub dir: String,
    /// The lockfile to write, in the shape [`read_lockfile`] returns.
    pub lockfile: serde_json::Value,
    /// `"wanted"` (the default) or `"current"`.
    pub kind: Option<String>,
    /// See [`ReadLockfileOptions::modules_dir`].
    pub modules_dir: Option<String>,
}

/// Inputs for [`filter_lockfile_by_importers`]. Mirrors
/// [`FilterLockfileOptions`] in `index.d.ts`.
#[napi(object)]
pub struct FilterLockfileOptions {
    /// Whether the listed importers keep their `dependencies`. Defaults to
    /// `true`.
    pub include_dependencies: Option<bool>,
    /// Whether they keep their `devDependencies`. Defaults to `true`.
    pub include_dev_dependencies: Option<bool>,
    /// Whether they keep their `optionalDependencies`. Defaults to `true`.
    pub include_optional_dependencies: Option<bool>,
    /// Dep paths to treat as already visited — the optional dependencies
    /// this platform did not install. Neither they nor anything reachable
    /// only through them is kept.
    pub skipped: Option<Vec<String>>,
    /// Whether a dependency reference with no `snapshots` entry fails with
    /// `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY`. Defaults to `false`, which
    /// drops the reference and keeps walking — what a caller inspecting a
    /// possibly-stale lockfile wants.
    pub fail_on_missing_dependencies: Option<bool>,
}

/// The lockfile as JSON, or `null` when the file is absent or empty.
#[napi]
pub async fn read_lockfile(
    options: ReadLockfileOptions,
) -> napi::Result<Option<serde_json::Value>> {
    let kind = LockfileKind::parse(options.kind.as_deref())?;
    let path = lockfile_path(&options.dir, options.modules_dir.as_deref(), &kind);
    let loaded = tokio::task::spawn_blocking(move || Lockfile::load_from_path(&path))
        .await
        .map_err(|join_error| {
            napi::Error::from_reason(format!("readLockfile task panicked: {join_error}"))
        })?
        .map_err(|error| to_napi_error(&error))?;
    loaded
        .map(|lockfile| {
            serde_json::to_value(lockfile)
                .map_err(|err| napi::Error::from_reason(format!("serializing the lockfile: {err}")))
        })
        .transpose()
}

/// Write the lockfile, formatted exactly as an install writes it.
#[napi]
pub async fn write_lockfile(options: WriteLockfileOptions) -> napi::Result<()> {
    let kind = LockfileKind::parse(options.kind.as_deref())?;
    let path = lockfile_path(&options.dir, options.modules_dir.as_deref(), &kind);
    let lockfile: Lockfile = serde_json::from_value(options.lockfile).map_err(|err| {
        napi::Error::from_reason(format!("the lockfile argument is not a lockfile: {err}"))
    })?;
    tokio::task::spawn_blocking(move || lockfile.save_to_path(&path))
        .await
        .map_err(|join_error| {
            napi::Error::from_reason(format!("writeLockfile task panicked: {join_error}"))
        })?
        .map_err(|error| to_napi_error(&error))
}

/// The lockfile narrowed to what `importerIds` reaches: those importers
/// keep only the dependency groups asked for, and `packages` / `snapshots`
/// are pruned to the transitive closure of what they still depend on.
/// Every other importer entry is carried through untouched.
///
/// Synchronous — it is a transform over data the caller already holds.
#[napi]
pub fn filter_lockfile_by_importers(
    lockfile: serde_json::Value,
    importer_ids: Vec<String>,
    options: Option<FilterLockfileOptions>,
) -> napi::Result<serde_json::Value> {
    let lockfile: Lockfile = serde_json::from_value(lockfile).map_err(|err| {
        napi::Error::from_reason(format!("the lockfile argument is not a lockfile: {err}"))
    })?;
    let options = options.unwrap_or(FilterLockfileOptions {
        include_dependencies: None,
        include_dev_dependencies: None,
        include_optional_dependencies: None,
        skipped: None,
        fail_on_missing_dependencies: None,
    });
    let skipped: HashSet<PackageKey> = options
        .skipped
        .unwrap_or_default()
        .iter()
        // An unparsable dep path matches no snapshot key, so skipping it
        // is a no-op either way; dropping it keeps a stale entry in a
        // host's skip list from failing the whole call.
        .filter_map(|dep_path| dep_path.parse().ok())
        .collect();
    let filtered = lockfile
        .filter_by_importers(
            importer_ids,
            &FilterByImportersOptions {
                include: IncludedDependencies {
                    dependencies: options.include_dependencies.unwrap_or(true),
                    dev_dependencies: options.include_dev_dependencies.unwrap_or(true),
                    optional_dependencies: options.include_optional_dependencies.unwrap_or(true),
                },
                skipped,
                fail_on_missing_dependencies: options.fail_on_missing_dependencies.unwrap_or(false),
            },
        )
        .map_err(|error| to_napi_error(&error))?;
    serde_json::to_value(filtered)
        .map_err(|err| napi::Error::from_reason(format!("serializing the lockfile: {err}")))
}

/// The `.modules.yaml` state of an installed `node_modules`, or `null`
/// when the directory has none. Same reader the engine uses, so a host
/// needs no `@pnpm/installing.modules-yaml`.
#[napi]
pub async fn read_modules_manifest(modules_dir: String) -> napi::Result<Option<serde_json::Value>> {
    let manifest = tokio::task::spawn_blocking(move || {
        pnpm_modules_yaml::read_modules_manifest::<pnpm_modules_yaml::Host>(Path::new(&modules_dir))
    })
    .await
    .map_err(|join_error| {
        napi::Error::from_reason(format!("readModulesManifest task panicked: {join_error}"))
    })?
    .map_err(|error| napi::Error::from_reason(format!("reading the modules manifest: {error}")))?;
    manifest
        .map(|manifest| {
            serde_json::to_value(manifest).map_err(|err| {
                napi::Error::from_reason(format!("serializing the modules manifest: {err}"))
            })
        })
        .transpose()
}

fn lockfile_path(dir: &str, modules_dir: Option<&str>, kind: &LockfileKind) -> PathBuf {
    let dir = Path::new(dir);
    match kind {
        LockfileKind::Wanted => dir.join(Lockfile::FILE_NAME),
        LockfileKind::Current => {
            let modules_dir = modules_dir.map_or_else(
                || dir.join("node_modules"),
                |modules_dir| {
                    let modules_dir = Path::new(modules_dir);
                    if modules_dir.is_absolute() {
                        modules_dir.to_path_buf()
                    } else {
                        dir.join(modules_dir)
                    }
                },
            );
            modules_dir.join(".pnpm").join(Lockfile::CURRENT_FILE_NAME)
        }
    }
}

#[cfg(test)]
mod tests;
