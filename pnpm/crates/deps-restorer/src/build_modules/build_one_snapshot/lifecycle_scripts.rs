//! Running a package's lifecycle scripts in each of its copies.

use super::{
    super::{
        BuildModulesError, LogEvent, LogLevel, PackageKey, Path, PathBuf, Reporter,
        RunPostinstallHooks, SkippedOptionalDependencyLog, SkippedOptionalPackage,
        SkippedOptionalReason, bin_dirs_in_all_parent_dirs, run_postinstall_hooks,
    },
    BuildOneSnapshot, dependency_bins, linked_copies,
};

/// Runs the build scripts in every copy of the package, `pkg_dir` first, as
/// pnpm builds each node of the hoisted graph. A version conflict nests a
/// copy under each project that needs it, and a copy left unbuilt lacks what
/// the scripts generate. Only `pkg_dir` seeds the side-effects cache.
pub(super) fn run_scripts_in_every_copy<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    pkg_dir: &Path,
    slot_lock: Option<&pnpm_fs::DirLock>,
    run: (bool, bool, &str, &str),
) -> Result<Option<bool>, BuildModulesError> {
    let other_copies = context
        .pkg_roots()
        .all(snapshot_key)
        .into_iter()
        .filter(|dir| dir != pkg_dir && dir.exists());
    let mut has_side_effects = false;
    for dir in std::iter::once(pkg_dir.to_path_buf()).chain(other_copies) {
        let extra_bin_paths = snapshot_extra_bin_paths(context, &dir);
        let Some(ran) = run_snapshot_scripts::<Reporter>(
            context,
            snapshot_key,
            &dir,
            &extra_bin_paths,
            slot_lock,
            run,
        )?
        else {
            return Ok(None);
        };
        has_side_effects |= ran;
    }
    Ok(Some(has_side_effects))
}

/// Whether the lifecycle scripts left side effects behind. `None` means an
/// optional dependency's build failed and the snapshot is skipped.
fn run_snapshot_scripts<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    pkg_dir: &Path,
    extra_bin_paths: &[PathBuf],
    slot_lock: Option<&pnpm_fs::DirLock>,
    run: (bool, bool, &str, &str),
) -> Result<Option<bool>, BuildModulesError> {
    let (should_run_scripts, optional, name, version) = run;
    if !should_run_scripts {
        return Ok(Some(false));
    }
    dependency_bins::refresh(context, snapshot_key)?;
    dependency_bins::record_mutation(context, snapshot_key)?;
    let result =
        run_candidate_hooks::<Reporter>(context, snapshot_key, pkg_dir, extra_bin_paths, optional);
    match result {
        Ok(ran) => Ok(Some(ran)),
        Err(err) => {
            if !optional {
                return Err(BuildModulesError::LifecycleScript(err));
            }
            // A global virtual store slot is removed only by an install that
            // holds its lock. A rebuild may re-run the scripts of a slot other
            // projects use with a working build, and without the lock another
            // install may be writing into the slot. A kept slot stays marked
            // for the next install to rebuild.
            if !context.directories.layout.enable_global_virtual_store()
                || (context.rebuild.is_none() && slot_lock.is_some())
            {
                linked_copies::discard_failed_optional(context, snapshot_key)?;
            }
            Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
                level: LogLevel::Debug,
                details: Some(err.to_string()),
                package: SkippedOptionalPackage::Installed {
                    id: pkg_dir.to_string_lossy().into_owned(),
                    name: name.to_string(),
                    version: version.to_string(),
                },
                parents: None,
                prefix: context.directories.lockfile_dir.to_string_lossy().into_owned(),
                reason: SkippedOptionalReason::BuildFailure,
            }));
            Ok(None)
        }
    }
}

fn run_candidate_hooks<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    pkg_dir: &Path,
    extra_bin_paths: &[PathBuf],
    optional: bool,
) -> Result<bool, pnpm_executor::LifecycleScriptError> {
    run_postinstall_hooks::<Reporter>(&RunPostinstallHooks {
        environment: pnpm_executor::ScriptEnvironment {
            init_cwd: context.directories.lockfile_dir,
            node_execpath: None,
            npm_execpath: None,
            node_gyp_path: pnpm_executor::bundled_node_gyp_entry(),
            user_agent: Some(context.scripts.user_agent),
            extra_env: context.scripts.extra_env,
        },
        execution: pnpm_executor::ScriptExecutionOptions {
            extra_bin_paths,
            node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
            shell: context.scripts.shell,
            shell_emulator: context.scripts.shell_emulator,
            wd_bin_dir: None,
        },
        dep_path: &snapshot_key.to_string(),
        pkg_root: pkg_dir,
        root_modules_dir: context.directories.modules_dir,

        unsafe_perm: context.scripts.unsafe_perm,

        optional,
    })
}

/// The directories a snapshot's build scripts get on `PATH` besides the ones
/// the executor derives from `pkg_dir`. Hoisted gathers every ancestor's
/// `node_modules/.bin` up to `lockfile_dir`, so a script at a nested hoisted
/// location can resolve bins added by parents. Both linkers then add the
/// pinned runtime's `node`.
fn snapshot_extra_bin_paths(context: &BuildOneSnapshot<'_>, pkg_dir: &Path) -> Vec<PathBuf> {
    let mut extra_bin_paths = if context.directories.gather_ancestor_bin_paths {
        bin_dirs_in_all_parent_dirs(pkg_dir, context.directories.lockfile_dir)
    } else {
        Vec::new()
    };
    extra_bin_paths.extend_from_slice(context.project_bin_dirs);
    extra_bin_paths
}
