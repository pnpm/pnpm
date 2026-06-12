//! Pacquet port of upstream pnpm's `@pnpm/store.controller`
//! [`projectRegistry`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/projectRegistry.ts).
//!
//! The project registry is a flat directory of symlinks at
//! `<store_dir>/projects/<short-hash>` that point back to every project
//! using the global virtual store. The prune sweep walks this directory
//! to learn which projects still reference the shared `<store_dir>/links`
//! slots — without it, a `pacquet store prune` (tracked separately) could
//! not distinguish abandoned packages from packages a project still uses.
//!
//! [`register_project`] (the write half) lives here alongside
//! [`get_registered_projects`] (the read half ported in pnpm/pacquet#458),
//! mirroring upstream's `projectRegistry.ts` module layout.

use crate::StoreDir;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_crypto_hash::create_short_hash;
use pacquet_fs::{lexical_normalize, read_symlink_dir, remove_symlink_dir, symlink_dir};
use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

/// Error type for [`register_project`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum RegisterProjectError {
    #[display("Failed to create the projects registry directory at {dir:?}: {error}")]
    #[diagnostic(code(pacquet_store_dir::register_project::create_registry_dir))]
    CreateRegistryDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display(
        "Failed to inspect the existing entry at {link_path:?} while registering project {project_dir:?}: {error}"
    )]
    #[diagnostic(code(pacquet_store_dir::register_project::inspect_existing))]
    InspectExisting {
        project_dir: PathBuf,
        link_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display(
        "Failed to remove stale entry at {link_path:?} (pointed at {old_target:?}, expected {project_dir:?}): {error}"
    )]
    #[diagnostic(code(pacquet_store_dir::register_project::remove_stale))]
    RemoveStale {
        project_dir: PathBuf,
        link_path: PathBuf,
        old_target: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display(
        "Failed to create the project registry symlink at {link_path:?} pointing to {project_dir:?}: {error}"
    )]
    #[diagnostic(code(pacquet_store_dir::register_project::create_symlink))]
    CreateSymlink {
        project_dir: PathBuf,
        link_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// Register `project_dir` as a user of the global virtual store at
/// `store_dir` by writing a symlink at
/// `<store_dir>/projects/<create_short_hash(project_dir)>` pointing
/// back at `project_dir`. Mirrors upstream's
/// [`registerProject`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/projectRegistry.ts).
///
/// Skips silently when `store_dir` lives inside `project_dir` — the
/// "store inside the project" case (legacy `--store-dir node_modules/.pnpm`
/// setups, or just a typo) would otherwise create a self-referential
/// symlink. Matches upstream's `isSubdir(projectDir, storeDir)` guard.
///
/// Idempotent: if the symlink already exists pointing at the same
/// project, the function is a no-op. If a previous entry under the
/// same short hash points elsewhere (very unlikely — would require a
/// sha256 collision in the first 32 hex chars), the stale entry is
/// removed and re-created so a re-run heals the registry.
pub fn register_project(
    store_dir: &StoreDir,
    project_dir: &Path,
) -> Result<(), RegisterProjectError> {
    // Upstream's `isSubdir(projectDir, storeDir)` is `(parent, child)`
    // — the npm `is-subdir` package signature. Skip when the store
    // root lives at or under the project dir.
    if path_contains(project_dir, store_dir.root()) {
        return Ok(());
    }

    let registry_dir = store_dir.projects();
    fs::create_dir_all(&registry_dir).map_err(|error| RegisterProjectError::CreateRegistryDir {
        dir: registry_dir.clone(),
        error,
    })?;

    let project_dir_str = project_dir.to_string_lossy();
    let link_path = registry_dir.join(create_short_hash(&project_dir_str));

    // Fast path: link doesn't exist yet — just create it.
    match symlink_dir(project_dir, &link_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            // Either the same project re-registering (no-op) or an
            // unrelated path that hashed to the same slug (heal).
            // Resolve and compare the existing link's target. The
            // cross-platform helper handles Windows junctions —
            // `fs::read_link` alone would fail with `EINVAL` for
            // every entry pacquet writes there (see
            // [`rust-lang/rust#28528`](https://github.com/rust-lang/rust/issues/28528)).
            let existing_target = read_symlink_dir(&link_path).map_err(|error| {
                RegisterProjectError::InspectExisting {
                    project_dir: project_dir.to_path_buf(),
                    link_path: link_path.clone(),
                    error,
                }
            })?;
            let canonical_existing = canonicalize_or_join(&link_path, &existing_target);
            let canonical_project =
                dunce::canonicalize(project_dir).unwrap_or_else(|_| project_dir.to_path_buf());
            if canonical_existing == canonical_project {
                return Ok(());
            }
            // Mismatch — remove the stale entry and recreate. The
            // entry is a directory symlink on Unix (file-shaped) and
            // a junction on Windows (directory-shaped); the helper
            // covers both.
            remove_symlink_dir(&link_path).map_err(|error| RegisterProjectError::RemoveStale {
                project_dir: project_dir.to_path_buf(),
                link_path: link_path.clone(),
                old_target: existing_target.clone(),
                error,
            })?;
            symlink_dir(project_dir, &link_path).map_err(|error| {
                RegisterProjectError::CreateSymlink {
                    project_dir: project_dir.to_path_buf(),
                    link_path,
                    error,
                }
            })
        }
        Err(error) => Err(RegisterProjectError::CreateSymlink {
            project_dir: project_dir.to_path_buf(),
            link_path,
            error,
        }),
    }
}

/// Error type for [`get_registered_projects`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum GetRegisteredProjectsError {
    #[display("Failed to read the projects registry directory at {dir:?}: {error}")]
    #[diagnostic(code(pacquet_store_dir::get_registered_projects::read_registry_dir))]
    ReadRegistryDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    /// Mirrors upstream's `PROJECT_REGISTRY_ENTRY_INACCESSIBLE`. Fires
    /// only when `read_link` failed with something *other* than
    /// `ENOENT` / `EINVAL` (those two are silently skipped to match
    /// upstream).
    #[display("Cannot read project registry entry {link_path:?}: {error}")]
    #[diagnostic(
        code(pacquet_store_dir::get_registered_projects::entry_inaccessible),
        help("To remove this project from the registry, delete the entry at: {link_path:?}")
    )]
    EntryInaccessible {
        link_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    /// Mirrors upstream's `PROJECT_INACCESSIBLE`. The registry entry
    /// exists and points at a path that exists according to the
    /// filesystem, but the stat returned a permission / I/O error.
    /// Surfaces instead of silently dropping the entry — pruning on an
    /// inaccessible project could remove slots the project still
    /// references.
    #[display("Cannot access registered project {project_dir:?} (via {link_path:?}): {error}")]
    #[diagnostic(
        code(pacquet_store_dir::get_registered_projects::project_inaccessible),
        help("To remove this project from the registry, delete the entry at: {link_path:?}")
    )]
    ProjectInaccessible {
        project_dir: PathBuf,
        link_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove stale project registry entry at {link_path:?}: {error}")]
    #[diagnostic(code(pacquet_store_dir::get_registered_projects::unlink_stale))]
    UnlinkStale {
        link_path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// List every project that's still registered against `store_dir` and
/// drop registry entries whose target directory no longer exists.
/// Mirrors upstream's
/// [`getRegisteredProjects`](https://github.com/pnpm/pnpm/blob/94240bc046/store/controller/src/storeController/projectRegistry.ts#L37-L100).
///
/// Returns the surviving project root paths (absolute, after the
/// `path.isAbsolute(target) ? target : path.resolve(path.dirname(linkPath), target)`
/// normalisation upstream performs — pacquet's [`register_project`]
/// always writes absolute targets via [`pacquet_fs::symlink_dir`], but
/// the relative-target branch is preserved so a registry seeded by
/// pnpm (which uses `symlink-dir`'s "make relative when possible"
/// behaviour on some platforms) still resolves correctly).
///
/// Side effects: any registry entry whose target stat returns
/// `NotFound` is unlinked here, so the projects directory self-heals
/// on every prune. Other I/O errors surface as
/// [`GetRegisteredProjectsError`] variants — a `PROJECT_INACCESSIBLE`
/// would otherwise leave the prune unable to tell whether the
/// project's slots are still referenced, so we refuse rather than
/// silently dropping the entry.
///
/// `ENOENT` on the registry directory itself returns an empty `Vec`
/// (matches upstream's `if (err.code === 'ENOENT') return []` branch)
/// — a store that hasn't seen any GVS install yet has no projects
/// dir, and that's not an error.
pub fn get_registered_projects(
    store_dir: &StoreDir,
) -> Result<Vec<PathBuf>, GetRegisteredProjectsError> {
    let registry_dir = store_dir.projects();
    let entries = match fs::read_dir(&registry_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(GetRegisteredProjectsError::ReadRegistryDir { dir: registry_dir, error });
        }
    };

    let mut projects = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                return Err(GetRegisteredProjectsError::ReadRegistryDir {
                    dir: registry_dir,
                    error,
                });
            }
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Skip dotfiles — matches upstream's `if (entry.name.startsWith('.')) return`.
        if name_str.starts_with('.') {
            continue;
        }
        let link_path = entry.path();
        // Only symlinks (junctions on Windows count via `is_symlink`).
        // Surfacing `file_type()` errors instead of swallowing them:
        // a permission failure here would otherwise silently drop a
        // live registry entry, and a downstream prune could then
        // remove slots that project still references. Upstream's
        // `entry.isSymbolicLink()` cannot fail (Node returns the
        // bit it already loaded with `withFileTypes: true`), so
        // there's no upstream analogue to mirror — we err on the
        // side of strictness.
        let file_type = entry.file_type().map_err(|error| {
            GetRegisteredProjectsError::EntryInaccessible { link_path: link_path.clone(), error }
        })?;
        if !file_type.is_symlink() {
            continue;
        }

        // Use the cross-platform symlink reader. On Windows
        // pacquet's writer creates junctions; `fs::read_link` alone
        // would EINVAL on every live entry (see
        // [`rust-lang/rust#28528`](https://github.com/rust-lang/rust/issues/28528)),
        // and the EINVAL silent-skip below would then drop every
        // registered project on that platform.
        let target = match read_symlink_dir(&link_path) {
            Ok(target) => target,
            // Upstream silently skips both ENOENT and EINVAL (the
            // "file is not a symlink" errno on Linux). Now that the
            // helper handles junctions, an EINVAL here means the
            // entry is neither a symlink nor a junction (some other
            // reparse-point shape, or a race) — still benign to
            // skip. EINVAL doesn't have a portable `ErrorKind`
            // variant in stable Rust, so we match raw `errno` via
            // `raw_os_error` when present and fall through to the
            // generic "inaccessible" error otherwise.
            Err(error) if is_enoent_or_einval(&error) => continue,
            Err(error) => {
                return Err(GetRegisteredProjectsError::EntryInaccessible { link_path, error });
            }
        };

        let absolute_target = if target.is_absolute() {
            target.clone()
        } else {
            link_path.parent().map_or_else(|| target.clone(), |p| p.join(&target))
        };

        match fs::metadata(&absolute_target) {
            Ok(_) => projects.push(absolute_target),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // Use the cross-platform helper: the registry entry
                // is a directory symlink on Unix and a junction on
                // Windows, which need different syscalls to unlink.
                remove_symlink_dir(&link_path).map_err(|error| {
                    GetRegisteredProjectsError::UnlinkStale { link_path: link_path.clone(), error }
                })?;
            }
            Err(error) => {
                return Err(GetRegisteredProjectsError::ProjectInaccessible {
                    project_dir: absolute_target,
                    link_path,
                    error,
                });
            }
        }
    }

    Ok(projects)
}

/// Match upstream's "silently skip on ENOENT or EINVAL" branch in
/// `getRegisteredProjects`. `EINVAL` from `readlink` means "not a
/// symbolic link" on Linux — the entry is benign garbage (e.g. a
/// stray regular file) and gets ignored. `ErrorKind::InvalidInput`
/// is the stable mapping for EINVAL on Unix; the raw errno fallback
/// covers platforms where the std mapping hasn't been tightened.
fn is_enoent_or_einval(error: &io::Error) -> bool {
    if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::InvalidInput) {
        return true;
    }
    #[cfg(unix)]
    {
        // EINVAL = 22 on every Unix pacquet supports (Linux, macOS, *BSD).
        // Hardcoded because there's no `libc::EINVAL` access without a
        // libc dep in this crate; the value is part of the POSIX
        // standard and won't change.
        if error.raw_os_error() == Some(22) {
            return true;
        }
    }
    false
}

/// Port of npm `is-subdir`: returns `true` when `inner` is `outer`
/// itself or any descendant of it. Renamed from upstream's
/// `isSubdir(parent, child)` because the bare name reads ambiguously
/// at the call site — `path_contains(outer, inner)` reads
/// unambiguously as "does `outer` contain `inner`".
///
/// Both paths are compared by their canonical (resolved) form so
/// symlinks don't fool the check. When either path can't be
/// canonicalized (typically the store dir hasn't been created yet),
/// fall back to a *lexical* normalization that collapses `.` and `..`
/// segments — matching upstream's `path.relative`-backed
/// [`is-subdir`](https://github.com/whitecolor/is-subdir/blob/main/index.js)
/// check, which is purely lexical. A raw `starts_with` on the
/// unnormalized path would treat `<workspace>/../pacquet-store` as
/// inside `<workspace>` even though resolving `..` puts it outside.
fn path_contains(outer: &Path, inner: &Path) -> bool {
    let outer_canonical = dunce::canonicalize(outer).unwrap_or_else(|_| lexical_normalize(outer));
    let inner_canonical = dunce::canonicalize(inner).unwrap_or_else(|_| lexical_normalize(inner));
    inner_canonical.starts_with(&outer_canonical)
}

/// Best-effort canonicalization for a symlink target: if the target is
/// absolute and canonicalizable, return its canonical form; otherwise
/// resolve it relative to the link's parent dir and try again; on any
/// failure return the lexically resolved path. Mirrors how upstream's
/// `getRegisteredProjects` handles `path.isAbsolute(target) ? target :
/// path.resolve(path.dirname(linkPath), target)`.
fn canonicalize_or_join(link_path: &Path, target: &Path) -> PathBuf {
    let absolute = if target.is_absolute() {
        target.to_path_buf()
    } else {
        link_path.parent().map_or_else(|| target.to_path_buf(), |p| p.join(target))
    };
    dunce::canonicalize(&absolute).unwrap_or(absolute)
}

#[cfg(test)]
mod tests;
