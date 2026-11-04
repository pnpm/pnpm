use std::path::Path;

pub(crate) fn local_path_base<'a>(
    config: &'a pnpm_config::Config,
    lockfile_dir: &'a Path,
) -> &'a Path {
    config.lockfile_dir
        .as_deref()
        .or(config.workspace_dir.as_deref())
        .unwrap_or(lockfile_dir)
}

#[cfg(test)]
mod tests;
