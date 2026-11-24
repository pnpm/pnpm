use super::{
    VersionArgs,
    VersionError,
    git::git_output,
};
use pnpm_config::Config;
use pnpm_pack::static_pack_sources;
use pnpm_package_manifest::{
    PROJECT_MANIFEST_BASENAMES,
    safe_read_project_manifest_from_dir,
};
use pnpm_publish::{
    Host,
    is_git_repo,
    is_working_tree_clean,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{
        Path,
        PathBuf,
    },
};

#[derive(Clone, Copy)]
pub(super) enum Scope<'a> {
    Repository,
    Package(&'a Path),
}

impl VersionArgs {
    pub(super) fn check_working_tree(
        &self,
        config: &Config,
        git_dir: &Path,
        scope: Scope<'_>,
    ) -> miette::Result<()> {
        if self.dry_run
            || !config.git_checks
            || self.git.no_git_checks
            || !is_git_repo::<Host>(git_dir)
        {
            return Ok(());
        }
        let clean = match scope {
            Scope::Repository => is_working_tree_clean::<Host>(git_dir),
            Scope::Package(package_dir) => can_bump_package(package_dir, git_dir, config),
        };
        if !clean {
            return Err(VersionError::UncleanWorkingTree.into());
        }
        Ok(())
    }
}

fn can_bump_package(dir: &Path, git_dir: &Path, config: &Config) -> bool {
    let Ok(status) =
        git_output(git_dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])
    else {
        return false;
    };
    if status.stdout.is_empty() {
        return true;
    }
    let Ok(root) = git_output(git_dir, &["rev-parse", "--show-toplevel"]) else { return false };
    let root = Path::new(root.stdout.strip_suffix('\n').unwrap_or(&root.stdout));
    let Some(changed) = unstaged_package_files(dir, root, &status.stdout) else { return false };
    let Ok(Some(manifest)) = safe_read_project_manifest_from_dir(dir) else { return false };
    if has_version_hooks(&manifest)
        || crate::config_deps::may_update_config(config, config.root_project_manifest_dir(dir))
    {
        return false;
    }
    let Ok(Some(sources)) = static_pack_sources(dir, config.workspace_dir.as_deref(), &manifest)
    else {
        return false;
    };
    sources_exclude_changes(sources, &changed)
}

fn sources_exclude_changes(
    mut sources: impl Iterator<Item = PathBuf>,
    changed: &HashSet<PathBuf>,
) -> bool {
    // Git reports regular files. Packing preserves leaf symlinks and can follow directory
    // symlinks for bundled dependencies, so only matching basenames need a realpath check.
    let names: Option<HashSet<_>> = changed
        .iter()
        .map(|path| ascii_basename(path))
        .collect();
    sources.all(|source| {
        if let Some(names) = &names
            && let Some(name) = ascii_basename(&source)
            && !names.contains(&name)
        {
            return true;
        }
        source
            .canonicalize()
            .is_ok_and(|path| !changed.contains(&path))
    })
}

fn ascii_basename(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    name.is_ascii().then(|| name.to_ascii_lowercase())
}

fn has_version_hooks(manifest: &Value) -> bool {
    ["preversion", "version", "postversion"]
        .iter()
        .any(|stage| manifest["scripts"][stage].is_string())
}

fn unstaged_package_files(dir: &Path, root: &Path, status: &str) -> Option<HashSet<PathBuf>> {
    // Git's porcelain paths are repository-relative. RunCommand decodes them lossily.
    if status.contains(char::REPLACEMENT_CHARACTER)
        || root.to_str()?.contains(char::REPLACEMENT_CHARACTER)
    {
        return None;
    }
    let dir = dir.canonicalize().ok()?;
    status
        .split_terminator('\0')
        .map(|record| {
            let path = record
                .strip_prefix(" M ")
                .or_else(|| record.strip_prefix("?? "))?;
            let file = root.join(path);
            if !file.symlink_metadata().ok()?.is_file() {
                return None;
            }
            let file = file.canonicalize().ok()?;
            let relative = file.strip_prefix(&dir).ok()?;
            (!affects_pack_rules(relative)).then_some(file)
        })
        .collect()
}

fn affects_pack_rules(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return true };
    path.starts_with(pnpm_versioning::CHANGES_DIR)
        || PROJECT_MANIFEST_BASENAMES
            .iter()
            .chain(
                [".gitignore", ".npmignore", ".npmrc", ".gitattributes", "pnpm-workspace.yaml"]
                    .iter(),
            )
            .any(|control| name.eq_ignore_ascii_case(control))
}
