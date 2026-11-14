use super::{
    ErrorKind, HashMap, HashSet, Matcher, OpenOptions, Path, PathBuf, UpdateChangesetError, Value,
    create_matcher, fs, is_dir_ref, normalize_project_dir, to_project_dir,
};
use std::io::Write as _;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ChangesetDirState {
    Present,
    Missing,
}

pub(super) struct IgnoredProjects {
    config_matcher: Matcher,
    ignored_names: HashSet<String>,
    ignored_dirs: HashSet<String>,
}

impl IgnoredProjects {
    pub(super) fn new(config_ignore: &[String], versioning_ignore: &[String]) -> Self {
        let mut ignored_names = HashSet::new();
        let mut ignored_dirs = HashSet::new();
        for entry in versioning_ignore {
            if is_dir_ref(entry) {
                ignored_dirs.insert(normalize_project_dir(entry));
            } else {
                ignored_names.insert(entry.clone());
            }
        }
        Self { config_matcher: create_matcher(config_ignore), ignored_names, ignored_dirs }
    }

    pub(super) fn matches(&self, package_name: &str, project_dir: &str) -> bool {
        self.config_matcher.matches(package_name)
            || self.ignored_names.contains(package_name)
            || self.ignored_dirs.contains(project_dir)
    }
}

/// The `ignore` patterns in `.changeset/config.json`, or `None` when the
/// file does not exist.
pub(super) fn read_config_ignore_patterns(
    config_path: &Path,
) -> Result<Option<Vec<String>>, UpdateChangesetError> {
    let config_text = match fs::read_to_string(config_path) {
        Ok(config_text) => config_text,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(UpdateChangesetError::ReadConfig {
                path: config_path.to_path_buf(),
                source,
            });
        }
    };
    let config: Value = serde_json::from_str(&config_text)
        .map_err(|source| UpdateChangesetError::ParseConfig {
            path: config_path.to_path_buf(),
            source,
        })?;
    let ignore_patterns = config
        .get("ignore")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    Ok(Some(ignore_patterns))
}

pub(super) fn project_reference(
    package_name: &str,
    workspace_dir: &Path,
    root_dir: &Path,
    name_counts: &HashMap<String, usize>,
) -> String {
    if name_counts
        .get(package_name)
        .copied()
        .unwrap_or(0)
        > 1
    {
        format!("./{}", to_project_dir(workspace_dir, root_dir))
    } else {
        package_name.to_string()
    }
}

/// Write `content` to a changeset file under a freshly generated id,
/// retrying until the name is one no file already holds.
pub(super) fn write_changeset(
    changeset_dir: &Path,
    content: &str,
) -> Result<PathBuf, UpdateChangesetError> {
    if ensure_changeset_dir_is_safe(changeset_dir)? == ChangesetDirState::Missing {
        match fs::create_dir(changeset_dir) {
            Ok(()) => {}
            Err(source) if source.kind() == ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(UpdateChangesetError::WriteChangeset {
                    path: changeset_dir.to_path_buf(),
                    source,
                });
            }
        }
    }
    ensure_changeset_dir_is_safe(changeset_dir)?;
    loop {
        let mut random = [0_u8; 4];
        getrandom::fill(&mut random).map_err(|source| UpdateChangesetError::GenerateId { source })?;
        let id = format!("pnpm-update-{:08x}", u32::from_be_bytes(random));
        let changeset_path = changeset_dir.join(format!("{id}.md"));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&changeset_path)
        {
            Ok(file) => file,
            Err(source) if source.kind() == ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(UpdateChangesetError::WriteChangeset { path: changeset_path, source });
            }
        };
        file.write_all(content.as_bytes())
            .map_err(|source| UpdateChangesetError::WriteChangeset {
                path: changeset_path.clone(),
                source,
            })?;
        break Ok(changeset_path);
    }
}

pub(super) fn ensure_changeset_dir_is_safe(
    changeset_dir: &Path,
) -> Result<ChangesetDirState, UpdateChangesetError> {
    let metadata = match fs::symlink_metadata(changeset_dir) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == ErrorKind::NotFound => {
            return Ok(ChangesetDirState::Missing);
        }
        Err(source) => {
            return Err(UpdateChangesetError::InspectChangesetDir {
                path: changeset_dir.to_path_buf(),
                source,
            });
        }
    };
    if metadata.file_type().is_symlink()
        || pnpm_fs::read_symlink_dir(changeset_dir).is_ok()
        || !metadata.is_dir()
    {
        return Err(UpdateChangesetError::UnsafeChangesetDir { path: changeset_dir.to_path_buf() });
    }
    Ok(ChangesetDirState::Present)
}
