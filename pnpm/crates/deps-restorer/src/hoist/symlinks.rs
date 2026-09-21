use super::HoistGraphNode;
use pnpm_lockfile::PackageKey;
use pnpm_modules_yaml::HoistKind;
use std::{collections::HashMap, path::PathBuf};

/// Create the hoist symlinks.
///
/// For each (`snapshot_key`, alias, kind) entry, link
/// `<target_dir>/<alias>` → `<layout.slot_dir(key)>/node_modules/<package_name>`,
/// where `<target_dir>` is `<public_hoisted_modules_dir>` for public-kind
/// or `<private_hoisted_modules_dir>` for private-kind. The
/// [`crate::VirtualStoreLayout`] handle resolves the slot directory in
/// either GVS mode (`<store_dir>/links/<scope>/<name>/<version>/<hash>/`)
/// or legacy flat-name mode
/// (`<virtual_store_dir>/<key.virtual_store_name>/`); the hoist code
/// never has to branch on `enable_global_virtual_store` itself.
///
/// Existing symlinks are introspected — if the existing entry is a
/// symlink pointing at a target inside the virtual store
/// (`layout.package_store_dir()` — the GVS links dir or the local
/// `.pnpm` dir) or inside the internal pnpm directory (the parent of
/// `private_hoisted_modules_dir`), the stale symlink is replaced.
/// External symlinks (or non-symlink occupants) are left in place.
///
/// Two-phase to amortize directory creation:
///
/// 1. Walk the input once to collect every `(target, dest)` symlink
///    pair plus the set of scope-dir parents (`<root>/@scope`)
///    needed by scoped aliases.
/// 2. `create_dir_all` the two hoisted-modules roots and each
///    distinct scope dir — once per dir, not per symlink, so a
///    1k-alias install doesn't pay 1k redundant stats on the same
///    handful of parents.
/// 3. `par_iter` the pair list and issue `symlinkat()` syscalls in
///    parallel via rayon. Each pair is now a single syscall — no
///    parent-dir prep — so the only contention is the kernel's
///    inode lock on the parent directory, which is dominated by
///    the syscall latency itself on macOS APFS / Linux ext4.
pub fn symlink_hoisted_dependencies(
    hoisted_by_node_id: &HashMap<PackageKey, HashMap<String, HoistKind>>,
    hoisted_workspace_aliases: &[(String, HoistKind, PathBuf)],
    graph: &HashMap<PackageKey, HoistGraphNode>,
    layout: &crate::VirtualStoreLayout,
    private_hoisted_modules_dir: &std::path::Path,
    public_hoisted_modules_dir: &std::path::Path,
    skipped: &std::collections::HashSet<PackageKey>,
) -> Result<(), crate::SymlinkPackageError> {
    let dirs = HoistedModulesDirs {
        private: private_hoisted_modules_dir,
        public: public_hoisted_modules_dir,
    };
    let mut plan = HoistSymlinkPlan::default();
    plan.add_slot_links(hoisted_by_node_id, graph, layout, skipped, &dirs)?;
    plan.add_workspace_links(hoisted_workspace_aliases, &dirs)?;
    if plan.work.is_empty() {
        return Ok(());
    }
    plan.create_parents(&dirs)?;
    plan.link(&dirs, layout)
}
/// The two roots a hoisted alias can be linked into.
pub(super) struct HoistedModulesDirs<'a> {
    private: &'a std::path::Path,
    public: &'a std::path::Path,
}
impl HoistedModulesDirs<'_> {
    fn root(&self, kind: HoistKind) -> &std::path::Path {
        match kind {
            HoistKind::Public => self.public,
            HoistKind::Private => self.private,
        }
    }
}
/// The symlinks a hoist pass will create, plus the scope directories
/// they need as parents.
///
/// `dep_dir` is shared through an `Arc` because a node with several
/// hoisted aliases would otherwise clone the `PathBuf` per alias (under
/// legacy flat-name mode it wraps the `to_virtual_store_name()` String
/// the lockfile crate flags as "far from optimal"). Most nodes have a
/// single alias, so the `Arc` overhead is marginal — but the `slot_dir`
/// lookup itself does work (`HashMap` probe + `String` build), so building
/// it once per node is worth the indirection.
#[derive(Default)]
pub(super) struct HoistSymlinkPlan {
    work: Vec<(std::sync::Arc<PathBuf>, PathBuf)>,
    scope_dirs: std::collections::HashSet<PathBuf>,
    destinations: std::collections::BTreeSet<Vec<String>>,
}
impl HoistSymlinkPlan {
    fn add_slot_links(
        &mut self,
        hoisted_by_node_id: &HashMap<PackageKey, HashMap<String, HoistKind>>,
        graph: &HashMap<PackageKey, HoistGraphNode>,
        layout: &crate::VirtualStoreLayout,
        skipped: &std::collections::HashSet<PackageKey>,
        dirs: &HoistedModulesDirs<'_>,
    ) -> Result<(), crate::SymlinkPackageError> {
        for (node_id, alias_map) in hoisted_by_node_id {
            // Skipped snapshots never get a virtual-store slot, so a
            // hoist symlink at their slot path would dangle (Unix) or
            // fail as a junction (Windows).
            // `hoisted_dependencies_by_node_id` records the (target,
            // alias) pair unconditionally, so the filter has to run
            // here too.
            if skipped.contains(node_id) {
                continue;
            }
            let Some(node) = graph.get(node_id) else { continue };
            // `node.name` originates from the lockfile, so a
            // traversal-shaped name is guarded here before it becomes
            // the hoist symlink's `<slot>/node_modules/<name>` target.
            let dep_dir = std::sync::Arc::new(
                crate::safe_join_modules_dir::safe_join_modules_dir(
                    &layout.slot_dir(node_id).join("node_modules"),
                    &node.name.to_string(),
                )
                .map_err(crate::SymlinkPackageError::InvalidAlias)?,
            );
            for (alias, kind) in alias_map {
                let destination =
                    crate::safe_join_modules_dir::safe_join_modules_dir(dirs.root(*kind), alias)
                        .map_err(crate::SymlinkPackageError::InvalidAlias)?;
                self.record_parent_dir(&destination, dirs.root(*kind));
                self.record_destination(&destination);
                self.work.push((std::sync::Arc::clone(&dep_dir), destination));
            }
        }
        Ok(())
    }

    /// `hoist-workspace-packages` placements: same (target, kind, alias)
    /// shape, with the target being the workspace project dir itself
    /// instead of a virtual-store slot. The alias is a package-manifest
    /// `name`, so the scope-dir prep applies to these too.
    fn add_workspace_links(
        &mut self,
        hoisted_workspace_aliases: &[(String, HoistKind, PathBuf)],
        dirs: &HoistedModulesDirs<'_>,
    ) -> Result<(), crate::SymlinkPackageError> {
        for (alias, kind, project_dir) in hoisted_workspace_aliases {
            let destination = crate::safe_join_modules_dir::safe_join_workspace_modules_dir(
                dirs.root(*kind),
                alias,
            )
            .map_err(crate::SymlinkPackageError::InvalidAlias)?;
            if self.destination_conflicts(&destination) {
                return Err(crate::SymlinkPackageError::InvalidAlias(
                    crate::safe_join_modules_dir::InvalidDependencyAliasError {
                        modules: dirs.root(*kind).to_path_buf(),
                        alias: alias.clone(),
                    },
                ));
            }
            self.record_parent_dir(&destination, dirs.root(*kind));
            self.record_destination(&destination);
            self.work.push((std::sync::Arc::new(project_dir.clone()), destination));
        }
        Ok(())
    }

    /// A scoped alias (`@scope/name`) lands in `<root>/@scope`, which
    /// doesn't exist yet on a fresh install. An alias with no `/` lands in
    /// `<root>`, created unconditionally by
    /// [`HoistSymlinkPlan::create_parents`].
    fn record_parent_dir(
        &mut self,
        destination: &std::path::Path,
        target_dir_root: &std::path::Path,
    ) {
        if let Some(parent) = destination.parent()
            && parent != target_dir_root
        {
            self.scope_dirs.insert(parent.to_path_buf());
        }
    }

    fn record_destination(&mut self, destination: &std::path::Path) {
        self.destinations.insert(normalized_components(destination));
    }

    fn destination_conflicts(&self, destination: &std::path::Path) -> bool {
        let destination = normalized_components(destination);
        if (1..destination.len()).any(|len| self.destinations.contains(&destination[..len])) {
            return true;
        }
        self.destinations
            .range((std::ops::Bound::Excluded(destination.clone()), std::ops::Bound::Unbounded))
            .next()
            .is_some_and(|existing| existing.starts_with(&destination))
    }

    /// Pre-create the destination parents serially — cheap, deduplicated,
    /// and a no-op for dirs that already exist — so the parallel symlink
    /// pass is one syscall per link.
    fn create_parents(
        &self,
        dirs: &HoistedModulesDirs<'_>,
    ) -> Result<(), crate::SymlinkPackageError> {
        if let Some(trusted_root) = dirs.public
            .ancestors()
            .find(|ancestor| !ancestor.as_os_str().is_empty() && dirs.private.starts_with(ancestor))
        {
            create_hoist_root(trusted_root, dirs.public)?;
            create_hoist_root(trusted_root, dirs.private)?;
        } else {
            create_hoist_root(filesystem_root(dirs.public)?, dirs.public)?;
            create_hoist_root(filesystem_root(dirs.private)?, dirs.private)?;
        }
        for parent in &self.scope_dirs {
            let root = if parent.starts_with(dirs.private) { dirs.private } else { dirs.public };
            create_hoist_parent_dirs(root, parent)?;
        }
        Ok(())
    }

    /// Fire the symlink syscalls in parallel. `try_for_each`
    /// short-circuits on the first error, propagating it through rayon's
    /// collector. `dest` is built inside the parallel closure (one
    /// `PathBuf::join` per task) so the sequential collection pass
    /// doesn't pay for it.
    fn link(
        &self,
        dirs: &HoistedModulesDirs<'_>,
        layout: &crate::VirtualStoreLayout,
    ) -> Result<(), crate::SymlinkPackageError> {
        use rayon::prelude::*;

        self.work.par_iter().try_for_each(
            |(dep_dir, dest)| -> Result<(), crate::SymlinkPackageError> {
                match pnpm_fs::symlink_dir(dep_dir.as_path(), dest) {
                    Ok(()) => Ok(()),
                    Err(ref error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        update_stale_hoist_symlink(
                            dep_dir.as_path(),
                            dest,
                            layout.package_store_dir(),
                            dirs.private.parent().expect(
                                "private_hoisted_modules_dir (<vs>/node_modules) always has a parent",
                            ),
                        )
                    }
                    Err(error) => Err(crate::SymlinkPackageError::SymlinkDir {
                        symlink_target: dep_dir.as_path().to_path_buf(),
                        symlink_path: dest.clone(),
                        error,
                    }),
                }
            },
        )
    }
}

fn filesystem_root(path: &std::path::Path) -> Result<&std::path::Path, crate::SymlinkPackageError> {
    path.ancestors()
        .last()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .ok_or_else(|| crate::SymlinkPackageError::CreateParentDir {
            dir: path.to_path_buf(),
            error: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "workspace hoist directory must be absolute",
            ),
        })
}

fn create_hoist_root(
    trusted_root: &std::path::Path,
    root: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    match create_hoist_parent_dirs(trusted_root, root) {
        Ok(()) => return Ok(()),
        Err(crate::SymlinkPackageError::CreateParentDir { error, .. })
            if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let existing_root = trusted_root
        .ancestors()
        .skip(1)
        .find(|ancestor| match std::fs::symlink_metadata(ancestor) {
            Ok(_) => true,
            Err(error) => error.kind() != std::io::ErrorKind::NotFound,
        })
        .ok_or_else(|| crate::SymlinkPackageError::CreateParentDir {
            dir: trusted_root.to_path_buf(),
            error: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "workspace hoist directory has no existing ancestor",
            ),
        })?;
    create_or_validate_hoist_parent(existing_root)?;
    create_hoist_parent_dirs(existing_root, root)
}

fn create_hoist_parent_dirs(
    root: &std::path::Path,
    parent: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    let relative = parent
        .strip_prefix(root)
        .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
            dir: parent.to_path_buf(),
            error: std::io::Error::new(std::io::ErrorKind::InvalidInput, error),
        })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        create_or_validate_hoist_parent(&current)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn create_or_validate_hoist_parent(
    dir: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    match std::fs::symlink_metadata(dir) {
        Ok(_) => validate_real_hoist_dir(dir),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(dir) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    validate_real_hoist_dir(dir)
                }
                Err(error) => Err(crate::SymlinkPackageError::CreateParentDir {
                    dir: dir.to_path_buf(),
                    error,
                }),
            }
        }
        Err(error) => {
            Err(crate::SymlinkPackageError::CreateParentDir { dir: dir.to_path_buf(), error })
        }
    }
}

#[cfg(windows)]
fn create_or_validate_hoist_parent(
    dir: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    match windows_file_attributes(dir) {
        Ok(attributes) => validate_real_hoist_dir(attributes)
            .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
                dir: dir.to_path_buf(),
                error,
            }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(dir) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    windows_file_attributes(dir).and_then(validate_real_hoist_dir)
                }
                Err(error) => Err(error),
            }
            .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
                dir: dir.to_path_buf(),
                error,
            })
        }
        Err(error) => {
            Err(crate::SymlinkPackageError::CreateParentDir { dir: dir.to_path_buf(), error })
        }
    }
}

#[cfg(not(windows))]
fn validate_real_hoist_dir(dir: &std::path::Path) -> Result<(), crate::SymlinkPackageError> {
    let metadata = std::fs::symlink_metadata(dir)
        .map_err(|error| crate::SymlinkPackageError::CreateParentDir {
            dir: dir.to_path_buf(),
            error,
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(crate::SymlinkPackageError::CreateParentDir {
            dir: dir.to_path_buf(),
            error: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "workspace hoist directory is not a real directory",
            ),
        });
    }
    Ok(())
}

#[cfg(windows)]
fn windows_file_attributes(dir: &std::path::Path) -> std::io::Result<u32> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesW, INVALID_FILE_ATTRIBUTES};

    match std::fs::symlink_metadata(dir) {
        Ok(metadata) => return Ok(metadata.file_attributes()),
        Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => {}
        Err(error) => return Err(error),
    }
    let path = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `path` is NUL-terminated and remains alive for the duration of the call.
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(attributes)
    }
}

#[cfg(windows)]
fn validate_real_hoist_dir(attributes: u32) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };

    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || attributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workspace hoist directory is not a real directory",
        ));
    }
    Ok(())
}

fn normalized_components(path: &std::path::Path) -> Vec<String> {
    path.components()
        .map(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .to_lowercase()
        })
        .collect()
}

/// Read the existing symlink at `dest` and decide whether it should
/// be replaced. If it already points at `dep_dir`, leave it untouched.
/// If it points inside `package_store_dir` or `internal_pnpm_dir`
/// (a pnpm-internal symlink — e.g., a stale link from a prior non-GVS
/// install), remove it and create a new symlink to `dep_dir`. External
/// symlinks (and non-symlink occupants) are left in place.
///
/// The already-correct fast path skips the unlink + recreate churn (and
/// the transient missing-link window it opens) on warm reinstalls, the
/// same way [`pnpm_fs::force_symlink_dir`] does — see its
/// `existing_symlink_up_to_date` helper.
pub(super) fn update_stale_hoist_symlink(
    dep_dir: &std::path::Path,
    dest: &std::path::Path,
    package_store_dir: &std::path::Path,
    internal_pnpm_dir: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    let Ok(existing_raw) = pnpm_fs::read_symlink_dir(dest) else {
        return Ok(());
    };
    let existing = if existing_raw.is_relative() {
        dest.parent()
            .unwrap_or_else(|| std::path::Path::new(""))
            .join(&existing_raw)
    } else {
        existing_raw
    };
    if pnpm_fs::lexical_normalize(&existing) == pnpm_fs::lexical_normalize(dep_dir) {
        return Ok(());
    }
    if !pnpm_fs::is_subdir(package_store_dir, &existing)
        && !pnpm_fs::is_subdir(internal_pnpm_dir, &existing)
    {
        return Ok(());
    }
    pnpm_fs::remove_symlink_dir(dest)
        .map_err(|error| crate::SymlinkPackageError::SymlinkDir {
            symlink_target: dep_dir.to_path_buf(),
            symlink_path: dest.to_path_buf(),
            error,
        })?;
    pnpm_fs::symlink_dir(dep_dir, dest)
        .map_err(|error| crate::SymlinkPackageError::SymlinkDir {
            symlink_target: dep_dir.to_path_buf(),
            symlink_path: dest.to_path_buf(),
            error,
        })
}
