use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::path::{Path, PathBuf};

/// Print the current package prefix — the nearest directory containing a
/// `package.json`, `node_modules`, or `pnpm-workspace.yaml`.
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
    /// `--global` is rejected because the global-dir machinery (pnpm's
    /// `@pnpm/global.commands`) is not ported to pacquet yet; refuse rather
    /// than print a wrong path.
    #[display(
        "`pnpm prefix --global` is not supported yet; global package management has not been ported to pnpm."
    )]
    #[diagnostic(code(pacquet_cli::prefix_global_unsupported))]
    GlobalUnsupported,

    /// IO error while looking up the prefix.
    #[display("failed to access {}: {source}", path.display())]
    #[diagnostic(code(pacquet_cli::prefix_io_error))]
    Io { path: PathBuf, source: std::io::Error },
}

/// Find the nearest directory containing package.json, `node_modules`, etc.
/// Port of findLocalPrefix from pnpm.
pub fn find_local_prefix(start_dir: &Path) -> miette::Result<PathBuf> {
    let mut name = start_dir.to_path_buf();

    while name.file_name().is_some_and(|f| f == "node_modules") {
        if let Some(parent) = name.parent() {
            name = parent.to_path_buf();
        } else {
            break;
        }
    }

    if name == start_dir { find_prefix_up(&name, &name) } else { Ok(name) }
}

fn find_prefix_up(name: &Path, original: &Path) -> miette::Result<PathBuf> {
    let mut current = name.to_path_buf();
    let targets =
        ["node_modules", "package.json", "package.json5", "package.yaml", "pnpm-workspace.yaml"];

    loop {
        for target in &targets {
            let target_path = current.join(target);
            match target_path.try_exists() {
                Ok(true) => return Ok(current),
                Ok(false) => continue,
                Err(e) => {
                    return Err(PrefixError::Io { path: target_path, source: e }.into());
                }
            }
        }

        match current.parent() {
            Some(parent) => {
                if parent == current {
                    return Ok(original.to_path_buf());
                }
                current = parent.to_path_buf();
            }
            None => return Ok(original.to_path_buf()),
        }
    }
}

impl PrefixArgs {
    pub fn run(self, dir: &Path) -> miette::Result<()> {
        if self.global {
            return Err(PrefixError::GlobalUnsupported.into());
        }
        let prefix_dir = find_local_prefix(dir)?;
        println!("{}", prefix_dir.display());
        Ok(())
    }
}
