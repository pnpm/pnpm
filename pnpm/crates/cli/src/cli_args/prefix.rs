use super::global::GlobalError;
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::{Config, check_global_bin_dir};
use std::path::{Path, PathBuf};

/// Print the current package prefix — the nearest ancestor directory
/// that holds a project.
#[derive(Debug, Args)]
pub struct PrefixArgs {
    /// Print the global prefix
    #[clap(short = 'g', long)]
    pub global: bool,
}

/// Errors specific to `pacquet prefix`.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PrefixError {
    /// IO error while looking up the prefix.
    #[display("failed to access {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_CLI_PREFIX_IO_ERROR))]
    Io { path: PathBuf, source: std::io::Error },
}

/// The markers that make a directory an npm project, as pnpm's
/// `findLocalPrefix` counts them.
const NPM_PROJECT_MARKERS: &[&str] =
    &["node_modules", "package.json", "package.json5", "package.yaml", "pnpm-workspace.yaml"];

/// [`NPM_PROJECT_MARKERS`] plus the manifests pnpm v12 manages beyond
/// `package.json` — a Cargo or Python package without a `package.json` is
/// a project too.
const PROJECT_MARKERS: &[&str] = &[
    "node_modules",
    "package.json",
    "package.json5",
    "package.yaml",
    "pnpm-workspace.yaml",
    "Cargo.toml",
    "pyproject.toml",
];

/// Find the nearest ancestor of `start_dir` that holds a project of any
/// ecosystem pnpm manages. `start_dir` itself counts, and a `start_dir` no
/// ancestor qualifies for is its own prefix.
///
/// Port of pnpm's `findLocalPrefix`, over [`PROJECT_MARKERS`].
pub fn find_local_prefix(start_dir: &Path) -> miette::Result<PathBuf> {
    find_prefix(start_dir, PROJECT_MARKERS)
}

/// Like [`find_local_prefix`], but only an npm project ends the walk.
///
/// The commands that act on `package.json#scripts` and on
/// `node_modules/.bin` have nothing to do in a Cargo or Python package, so
/// they walk past one to the npm project that encloses it, as pnpm 11
/// does.
pub fn find_npm_local_prefix(start_dir: &Path) -> miette::Result<PathBuf> {
    find_prefix(start_dir, NPM_PROJECT_MARKERS)
}

fn find_prefix(start_dir: &Path, targets: &[&str]) -> miette::Result<PathBuf> {
    let mut name = start_dir.to_path_buf();

    while name.file_name().is_some_and(|f| f == "node_modules") {
        if let Some(parent) = name.parent() {
            name = parent.to_path_buf();
        } else {
            break;
        }
    }

    if name == start_dir { find_prefix_up(&name, &name, targets) } else { Ok(name) }
}

fn find_prefix_up(name: &Path, original: &Path, targets: &[&str]) -> miette::Result<PathBuf> {
    let mut current = name.to_path_buf();

    loop {
        match probe_project_markers(&current, targets, original)? {
            MarkerProbe::Found => return Ok(current),
            MarkerProbe::Unreadable => return Ok(original.to_path_buf()),
            MarkerProbe::NotFound => {}
        }
        let Some(parent) = current.parent().filter(|parent| *parent != current) else {
            return Ok(original.to_path_buf());
        };
        current = parent.to_path_buf();
    }
}

/// What one directory of the walk says about being a project root.
enum MarkerProbe {
    Found,
    NotFound,
    /// The directory could not be read, so the walk stops here.
    Unreadable,
}

/// Whether the directory carries one of the markers that make it a
/// project root.
///
/// A directory that cannot be read only aborts the walk with an error
/// when it is the one the caller named. Above it, an unreadable ancestor
/// means the walk found nothing, matching pnpm.
fn probe_project_markers(
    current: &Path,
    targets: &[&str],
    original: &Path,
) -> miette::Result<MarkerProbe> {
    for target in targets {
        let target_path = current.join(target);
        match target_path.try_exists() {
            Ok(true) => return Ok(MarkerProbe::Found),
            Ok(false) => continue,
            Err(error) if current == original => {
                return Err(PrefixError::Io { path: target_path, source: error }.into());
            }
            Err(_) => return Ok(MarkerProbe::Unreadable),
        }
    }
    Ok(MarkerProbe::NotFound)
}

impl PrefixArgs {
    pub fn run(self, dir: &Path, config: &Config) -> miette::Result<()> {
        if self.global {
            // Mirror pnpm's config reader: create then validate the global bin
            // dir for every `--global` command, without the writability check
            // (`globalDirShouldAllowWrite` is false for `root` and `prefix`;
            // see pnpm issue 2700).
            let bin = config.global_bin.clone().ok_or(GlobalError::NoGlobalBinDir)?;
            std::fs::create_dir_all(&bin).map_err(|error| {
                let bin_dir = bin.display();
                miette::miette!("failed to create the global bin directory {bin_dir}: {error}")
            })?;
            check_global_bin_dir(&bin, std::env::var("PATH").ok().as_deref(), false)
                .map_err(miette::Report::new)?;
            // pnpm's `prefix` handler prints the parent of the global packages
            // dir — the global dir root, without the layout-version leaf.
            let pkg_dir =
                config.global_pkg_dir.clone().ok_or(GlobalError::MissingGlobalPackageDir)?;
            let prefix_dir = pkg_dir.parent().ok_or(GlobalError::MissingGlobalPackageDir)?;
            println!("{}", prefix_dir.display());
            return Ok(());
        }
        let prefix_dir = find_local_prefix(dir)?;
        println!("{}", prefix_dir.display());
        Ok(())
    }
}
