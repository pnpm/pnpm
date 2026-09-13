use super::{
    BTreeMap, Path, PathBuf, check_ancestors, command_output, create_hex_hash,
    create_hex_hash_from_file, fs, io,
};
pub(in super::super) fn snapshot_entry(
    cache_dir: &Path,
    project: &Path,
    task_key: &str,
    environment: &BTreeMap<String, String>,
) -> io::Result<(PathBuf, String, Vec<String>)> {
    let repo = PathBuf::from(
        command_output(
            "git",
            &["rev-parse", "--show-toplevel"],
            project,
            environment,
        )?
        .trim(),
    );
    let common = command_output(
        "git",
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        project,
        environment,
    )?;
    let common = dunce::canonicalize(common.trim())?;
    let mut inputs = vec!["pnpm-cargo-state:v1".to_string(), task_key.to_string()];
    inputs.push(command_output("rustc", &["-vV"], project, environment)?);
    inputs.push(command_output("cargo", &["-vV"], project, environment)?);
    let local_packages = local_workspace_packages(project, environment, &repo)?;
    inputs.push(serde_json::to_string(environment)?);
    add_repository_inputs(&repo, environment, &mut inputs)?;
    add_config_inputs(project, environment, &mut inputs)?;
    let key = create_hex_hash(&serde_json::to_string(&inputs)?);
    let scope = create_hex_hash(&common.to_string_lossy());
    Ok((
        cache_dir
            .join("cargo-build/v1")
            .join(scope)
            .join(&key),
        key,
        local_packages,
    ))
}

/// Every Cargo config file the build reads: `.cargo/config[.toml]` in each
/// ancestor of the project, then the Cargo home's.
fn add_config_inputs(
    project: &Path,
    environment: &BTreeMap<String, String>,
    inputs: &mut Vec<String>,
) -> io::Result<()> {
    for ancestor in project.ancestors() {
        for name in ["config", "config.toml"] {
            add_config(&ancestor.join(".cargo").join(name), project, inputs)?;
        }
    }
    let cargo_home = environment
        .get("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| home::home_dir().map(|home| home.join(".cargo")));
    if let Some(cargo_home) = cargo_home {
        for name in ["config", "config.toml"] {
            add_config(&cargo_home.join(name), project, inputs)?;
        }
    }
    Ok(())
}

/// The workspace's own packages, and the guarantee that each one's
/// manifest lives inside the repository — a path dependency outside it
/// is an input the cache key cannot cover.
fn local_packages_in_repo(metadata: &serde_json::Value, repo: &Path) -> io::Result<Vec<String>> {
    let canonical_repo = dunce::canonicalize(repo)?;
    let mut local_packages = Vec::new();
    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| io::Error::other("Cargo metadata has no packages"))?;
    for package in packages
        .iter()
        .filter(|package| package["source"].is_null())
    {
        local_packages.push(
            package["name"]
                .as_str()
                .ok_or_else(|| io::Error::other("Cargo package has no name"))?
                .to_string(),
        );
        let manifest = package["manifest_path"]
            .as_str()
            .ok_or_else(|| io::Error::other("Cargo metadata has no manifest path"))?;
        if !dunce::canonicalize(manifest)?.starts_with(&canonical_repo) {
            return Err(io::Error::other(format!(
                "Cargo path dependency is outside the repository: {manifest}",
            )));
        }
    }
    Ok(local_packages)
}

/// Add one tracked file's contents to the cache key. A path git lists
/// but that is gone is simply not an input; anything that is not a
/// regular file is one the hash cannot describe.
fn add_tracked_file_input(repo: &Path, path: &str, inputs: &mut Vec<String>) -> io::Result<()> {
    check_ancestors(repo, Path::new(path))?;
    let absolute = repo.join(path);
    match fs::symlink_metadata(&absolute) {
        Ok(metadata) if metadata.is_file() => {
            inputs.push(format!("{path}:{}", create_hex_hash_from_file(&absolute)?));
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(io::Error::other(format!(
            "Cargo cache input is not a regular file: {}",
            absolute.display(),
        ))),
        Err(error) => Err(error),
    }
}

fn add_config(path: &Path, project: &Path, inputs: &mut Vec<String>) -> io::Result<()> {
    match create_hex_hash_from_file(path) {
        Ok(hash) => {
            let relative =
                pathdiff::diff_paths(path, project).unwrap_or_else(|| path.to_path_buf());
            inputs.push(format!("cargo-config:{}:{hash}", relative.display()));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn add_repository_inputs(
    repo: &Path,
    environment: &BTreeMap<String, String>,
    inputs: &mut Vec<String>,
) -> io::Result<()> {
    let paths = command_output(
        "git",
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
        repo,
        environment,
    )?;
    let mut paths: Vec<_> = paths
        .split('\0')
        .filter(|path| !path.is_empty())
        .collect();
    paths.sort_unstable();
    paths.dedup();
    for path in paths {
        add_tracked_file_input(repo, path, inputs)?;
    }
    Ok(())
}

fn local_workspace_packages(
    project: &Path,
    environment: &BTreeMap<String, String>,
    repo: &Path,
) -> io::Result<Vec<String>> {
    let metadata: serde_json::Value = serde_json::from_str(&command_output(
        "cargo",
        &["metadata", "--format-version=1", "--locked", "--offline"],
        project,
        environment,
    )?)?;
    local_packages_in_repo(&metadata, repo)
}
