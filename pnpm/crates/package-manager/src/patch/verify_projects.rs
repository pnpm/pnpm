use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_reporter::Reporter;
use std::path::PathBuf;

#[derive(Debug, Display, Error, Diagnostic, derive_more::From)]
#[non_exhaustive]
pub enum VerifyProjectPatchesError {
    #[diagnostic(transparent)]
    LoadLockfile(#[error(source)] pnpm_lockfile::LoadLockfileError),

    #[diagnostic(transparent)]
    ResolvePatchedDependencies(#[error(source)] pnpm_patching::ResolvePatchedDependenciesError),

    #[diagnostic(transparent)]
    UnusedPatch(#[error(source)] pnpm_patching::UnusedPatchError),
}

pub fn verify_patches_against_project_lockfiles<Reporter: self::Reporter>(
    config: &Config,
    project_dirs: &[PathBuf],
) -> Result<(), VerifyProjectPatchesError> {
    if !config.lockfile || config.shares_one_lockfile() {
        return Ok(());
    }
    let Some(groups) = config.resolved_patched_dependencies()? else {
        return Ok(());
    };
    let selection = config.wanted_lockfile_selection();
    let mut applied = std::collections::HashSet::new();
    for project_dir in project_dirs {
        if let Some(lockfile) =
            Lockfile::load_wanted(config.lockfile_dir_for(project_dir), &selection)?
            && let Some(keys) = pnpm_lockfile::applied_patch_keys(&lockfile, &groups)
        {
            applied.extend(keys.into_iter().map(str::to_owned));
        }
    }
    match pnpm_patching::verify_patches(&groups, &applied, config.allow_unused_patches)? {
        None => Ok(()),
        Some(warning) => {
            Reporter::emit(&pnpm_reporter::LogEvent::Global(pnpm_reporter::GlobalLog {
                level: pnpm_reporter::LogLevel::Warn,
                message: warning.to_string(),
            }));
            Ok(())
        }
    }
}
