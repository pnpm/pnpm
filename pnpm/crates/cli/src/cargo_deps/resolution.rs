use super::{
    ManagedDirectory, ensure_workspace_directory, git, managed_config_range, read_workspace_file,
    write_workspace_file,
};
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_config::Config;
use pnpm_network::redact_and_sanitize_multiline;
use std::{fs, io, path::Path, process::Command, sync::Mutex};

// A nested Cargo workspace can inherit its parent's managed sources. Keep
// configuration removal and restoration together across selected workspaces.
static RESOLUTION_LOCK: Mutex<()> = Mutex::new(());

pub(super) fn has_git_dependencies(metadata: &str) -> Result<bool> {
    let sources = pnpm_cargo_resolver::git_dependency_sources(metadata)?;
    for source in &sources {
        git::validate_transport(source)?;
    }
    Ok(!sources.is_empty())
}

pub(super) async fn resolve_with_cargo(config: &Config, root: &Path) -> Result<String> {
    if !pnpm_cargo_resolver::is_crates_io(&config.cargo.index_url) {
        return Err(miette::miette!(
            "Resolving Cargo git dependencies requires the default cargo.indexUrl. Use an existing Cargo.lock with a custom Cargo registry.",
        ));
    }
    let root = root.to_path_buf();
    let offline = config.offline;
    tokio::task::spawn_blocking(move || resolve_workspace(&root, offline)).await
        .into_diagnostic()
        .wrap_err("join Cargo git dependency resolution")?
}

struct SourceConfig {
    directory: ManagedDirectory,
    name: &'static str,
    contents: String,
    mode: Option<u32>,
}

fn resolve_workspace(root: &Path, offline: bool) -> Result<String> {
    let _lock = RESOLUTION_LOCK
        .lock()
        .map_err(|error| miette::miette!("lock Cargo resolution: {error}"))?;
    let configs = source_configs(root)?;
    let outcome = resolve_without_managed_sources(root, offline, &configs);
    let failures = configs
        .iter()
        .rev()
        .filter_map(|config| write_config(config, &config.contents).err())
        .map(|error| format!("{error:?}"))
        .collect::<Vec<_>>();
    if failures.is_empty() {
        return outcome;
    }
    let restoration = format!("restore Cargo source configuration: {}", failures.join("; "));
    match outcome {
        Ok(_) => Err(miette::miette!(restoration)),
        Err(error) => Err(error.wrap_err(restoration)),
    }
}

fn source_configs(root: &Path) -> Result<Vec<SourceConfig>> {
    let mut configs = Vec::new();
    for ancestor in root.ancestors() {
        let Some(name) = config_name(ancestor)? else { continue };
        let directory = ensure_workspace_directory(ancestor, &[".cargo"])?;
        let (contents, mode) = read_workspace_file(&directory, name)
            .into_diagnostic()
            .wrap_err_with(|| format!("read {}", directory.path.join(name).display()))?;
        if managed_config_range(&contents)?.is_some() {
            configs.push(SourceConfig { directory, name, contents, mode });
        }
    }
    Ok(configs)
}

pub(super) fn config_name(root: &Path) -> Result<Option<&'static str>> {
    for name in ["config", "config.toml"] {
        let path = root.join(".cargo").join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => return Ok(Some(name)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("inspect {}", path.display()));
            }
        }
    }
    Ok(None)
}

fn resolve_without_managed_sources(
    root: &Path,
    offline: bool,
    configs: &[SourceConfig],
) -> Result<String> {
    for config in configs {
        let range = managed_config_range(&config.contents)?
            .ok_or_else(|| miette::miette!("Cargo source configuration has no managed block"))?;
        let contents =
            format!("{}{}", &config.contents[..range.start], &config.contents[range.end..]);
        write_config(config, &contents)?;
    }
    let mut command = Command::new("cargo");
    command.current_dir(root).arg("generate-lockfile");
    if offline {
        command.arg("--offline");
    }
    let output = command
        .output()
        .into_diagnostic()
        .wrap_err("run cargo generate-lockfile")?;
    if !output.status.success() {
        let stderr = redact_and_sanitize_multiline(&String::from_utf8_lossy(&output.stderr));
        return Err(miette::miette!(
            "cargo generate-lockfile failed for {}: {}",
            root.display(),
            stderr.trim(),
        ));
    }
    let directory = ensure_workspace_directory(root, &[])?;
    read_workspace_file(&directory, "Cargo.lock")
        .map(|(contents, _)| contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", root.join("Cargo.lock").display()))
}

fn write_config(config: &SourceConfig, contents: &str) -> Result<()> {
    write_workspace_file(&config.directory, config.name, contents.as_bytes(), config.mode)
        .into_diagnostic()
        .wrap_err_with(|| format!("write {}", config.directory.path.join(config.name).display()))
}
