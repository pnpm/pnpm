use super::git::git_output;
use pnpm_config::Config;
use pnpm_pack::static_pack_sources;
use pnpm_package_manifest::{
    PROJECT_MANIFEST_BASENAMES,
    safe_read_project_manifest_from_dir,
};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{
        Path,
        PathBuf,
    },
};

pub(super) fn can_bump(dir: &Path, config: &Config, recursive: bool) -> miette::Result<bool> {
    if recursive {
        return Ok(super::is_working_tree_clean::<super::Host>(dir));
    }
    let status = git_output(dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    if status.stdout.is_empty() {
        return Ok(true);
    }
    let prefix = git_output(dir, &["rev-parse", "--show-prefix"])?;
    let prefix = prefix.stdout.strip_suffix('\n').unwrap_or(&prefix.stdout);
    let Some(changed) = unstaged_package_files(dir, prefix, &status.stdout) else {
        return Ok(false);
    };
    let Some(manifest) = safe_read_project_manifest_from_dir(dir)? else { return Ok(false) };
    if has_version_hooks(&manifest)
        || crate::config_deps::may_update_config(config, config.root_project_manifest_dir(dir))
    {
        return Ok(false);
    }
    let Some(mut sources) = static_pack_sources(dir, config.workspace_dir.as_deref(), &manifest)?
    else {
        return Ok(false);
    };
    Ok(sources.all(|source| {
        source
            .canonicalize()
            .is_ok_and(|path| !changed.contains(&path))
    }))
}

fn has_version_hooks(manifest: &Value) -> bool {
    ["preversion", "version", "postversion"]
        .iter()
        .any(|stage| manifest["scripts"][stage].is_string())
}

fn unstaged_package_files(dir: &Path, prefix: &str, status: &str) -> Option<HashSet<PathBuf>> {
    // Git's porcelain paths are repository-relative. RunCommand decodes them lossily.
    if status.contains(char::REPLACEMENT_CHARACTER) || prefix.contains(char::REPLACEMENT_CHARACTER)
    {
        return None;
    }
    status
        .split_terminator('\0')
        .map(|record| {
            let path = record
                .strip_prefix(" M ")
                .or_else(|| record.strip_prefix("?? "))?;
            let relative = Path::new(path.strip_prefix(prefix)?);
            let file = dir.join(relative);
            if affects_pack_rules(relative) || !file.symlink_metadata().ok()?.is_file() {
                return None;
            }
            file.canonicalize().ok()
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
