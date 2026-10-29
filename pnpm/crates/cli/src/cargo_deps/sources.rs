use super::{
    CRATES_SOURCE_DIRECTORY, GIT_SOURCE_DIRECTORY, GIT_SOURCE_NAME, GitSource, IntoDiagnostic,
    ManagedDirectory, Path, Result, SOURCES_CONFIG, SOURCES_CONFIG_DIRECTORY,
    ensure_workspace_directory, force_workspace_symlink, io, is_crates_io, read_workspace_file,
    write_workspace_file,
};
use miette::WrapErr;
use std::path::PathBuf;

const MANAGED_START: &str = "# >>> pnpm-managed cargo sources >>>";
const MANAGED_END: &str = "# <<< pnpm-managed cargo sources <<<";

pub(super) fn link_workspace(
    root_dir: &Path,
    directory: &[&str],
    slots: &[(String, PathBuf)],
) -> Result<()> {
    let source_dir = ensure_workspace_directory(root_dir, directory)?;
    link_workspace_in(&source_dir, slots)
}

pub(super) fn link_workspace_in(
    source_dir: &ManagedDirectory,
    slots: &[(String, PathBuf)],
) -> Result<()> {
    for (name, slot) in slots {
        let outcome = force_workspace_symlink(source_dir, slot, name)
            .into_diagnostic()
            .wrap_err_with(|| format!("link cargo package {name}"))?;
        if let Some(warning) = outcome.warning {
            tracing::warn!(target: "pacquet::cargo", ?warning, "cargo package link warning");
        }
    }
    Ok(())
}

pub(super) fn write_sources_config(
    root_dir: &Path,
    index_url: &str,
    git_sources: &[GitSource],
) -> Result<()> {
    let directory = ensure_workspace_directory(root_dir, &SOURCES_CONFIG_DIRECTORY)?;
    let path = directory.path.join(SOURCES_CONFIG);
    let config = sources_config(index_url, git_sources);
    let (existing, mode) = read_existing_file(&directory, SOURCES_CONFIG)?;
    if existing != config {
        write_workspace_file(&directory, SOURCES_CONFIG, config.as_bytes(), mode)
            .into_diagnostic()
            .wrap_err_with(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

pub(super) fn write_cargo_config(root_dir: &Path) -> Result<()> {
    let cargo_dir = ensure_workspace_directory(root_dir, &[".cargo"])?;
    write_cargo_config_in(&cargo_dir)
}

pub(super) fn write_cargo_config_in(cargo_dir: &ManagedDirectory) -> Result<()> {
    let config_path = cargo_dir.path.join("config.toml");
    let (existing, mode) = read_existing_file(cargo_dir, "config.toml")?;
    let updated = update_managed_config(&existing)?;
    if updated != existing {
        write_workspace_file(cargo_dir, "config.toml", updated.as_bytes(), mode)
            .into_diagnostic()
            .wrap_err_with(|| format!("write {}", config_path.display()))?;
    }
    Ok(())
}

/// The file's contents and mode, or an empty string when it does not exist.
fn read_existing_file(directory: &ManagedDirectory, name: &str) -> Result<(String, Option<u32>)> {
    match read_workspace_file(directory, name) {
        Ok(existing) => Ok(existing),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok((String::new(), None)),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err_with(|| format!("read {}", directory.path.join(name).display())),
    }
}

pub(super) fn update_managed_config(existing: &str) -> Result<String> {
    let updated = splice_managed_config(existing)?;
    // `[[include]]` cannot extend an `include` key written inline.
    if toml::from_str::<toml::Table>(existing).is_ok()
        && let Err(error) = toml::from_str::<toml::Table>(&updated)
    {
        return Err(miette::miette!(
            "pnpm cannot add its `[[include]]` entry to .cargo/config.toml. Write `include` as `[[include]]` tables: {error}"
        ));
    }
    Ok(updated)
}

fn splice_managed_config(existing: &str) -> Result<String> {
    let managed_config = managed_config();
    match managed_config_range(existing)? {
        None => {
            let separator = if existing.is_empty() || existing.ends_with("\n\n") {
                ""
            } else if existing.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            Ok(format!("{existing}{separator}{managed_config}\n"))
        }
        Some(range) => {
            Ok(format!("{}{}{}", &existing[..range.start], managed_config, &existing[range.end..]))
        }
    }
}

fn managed_config_range(existing: &str) -> Result<Option<std::ops::Range<usize>>> {
    match (existing.find(MANAGED_START), existing.find(MANAGED_END)) {
        (None, None) => Ok(None),
        (Some(start), Some(end)) if start <= end => Ok(Some(start..end + MANAGED_END.len())),
        _ => Err(miette::miette!(
            ".cargo/config.toml contains an incomplete pnpm-managed Cargo source block"
        )),
    }
}

pub(super) fn managed_config() -> String {
    let path = format!("../{}/{SOURCES_CONFIG}", SOURCES_CONFIG_DIRECTORY.join("/"));
    format!("{MANAGED_START}\n[[include]]\npath = \"{path}\"\noptional = true\n{MANAGED_END}")
}

/// Cargo resolves a relative `directory` against the parent of the directory
/// holding the config file, so these paths start below `.pnpm`.
pub(super) fn sources_config(index_url: &str, git_sources: &[GitSource]) -> String {
    let crates = CRATES_SOURCE_DIRECTORY[1..].join("/");
    let mut body = if is_crates_io(index_url) {
        format!(
            "[source.crates-io]\nreplace-with = \"pnpm-crates-io\"\n\n[source.pnpm-crates-io]\ndirectory = \"{crates}\"\n",
        )
    } else {
        let source = toml::Value::from(pnpm_cargo_resolver::sparse_source(index_url));
        format!(
            "[source.crates-io]\nreplace-with = \"pnpm-registry\"\n\n[source.pnpm-registry]\nregistry = {source}\nreplace-with = \"pnpm-registry-directory\"\n\n[source.pnpm-registry-directory]\ndirectory = \"{crates}\"\n",
        )
    };
    if !git_sources.is_empty() {
        let git = GIT_SOURCE_DIRECTORY[1..].join("/");
        let blocks = git_sources
            .iter()
            .fold(String::new(), |mut blocks, source| {
                blocks.push('\n');
                blocks.push_str(&source.config_block());
                blocks
            });
        body = format!("{body}\n[source.{GIT_SOURCE_NAME}]\ndirectory = \"{git}\"\n{blocks}");
    }
    body
}
