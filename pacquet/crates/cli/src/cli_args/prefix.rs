use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::path::{Path, PathBuf};

/// `pacquet prefix`: print the current package prefix.
///
/// Ports pnpm's `prefix` handler, walking up to find the nearest
/// package prefix directory (containing package.json, node_modules, etc.).
#[derive(Debug, Args)]
pub struct PrefixArgs {
    /// Print the global prefix
    #[clap(short = 'g', long)]
    pub global: bool,
}

/// Errors specific to `pacquet prefix`.
#[derive(Debug, Display, Error, Diagnostic, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrefixError {
    /// `--global` is rejected because the global-dir machinery (pnpm's
    /// `@pnpm/global.commands`) is not ported to pacquet yet; refuse rather
    /// than print a wrong path.
    #[display(
        "`pacquet prefix --global` is not supported yet; global package management has not been ported to pacquet."
    )]
    #[diagnostic(code(pacquet_cli::prefix_global_unsupported))]
    GlobalUnsupported,
}

/// Find the nearest directory containing package.json, node_modules, etc.
/// Port of findLocalPrefix from pnpm.
pub fn find_local_prefix(start_dir: &Path) -> PathBuf {
    let mut name = start_dir.to_path_buf();

    let mut walked_up = false;
    while name.file_name().map_or(false, |f| f == "node_modules") {
        if let Some(parent) = name.parent() {
            name = parent.to_path_buf();
            walked_up = true;
        } else {
            break;
        }
    }

    if walked_up {
        return name;
    }

    find_prefix_up(&name, &name)
}

fn find_prefix_up(name: &Path, original: &Path) -> PathBuf {
    if name.parent().is_none() {
        return original.to_path_buf();
    }

    let targets =
        ["node_modules", "package.json", "package.json5", "package.yaml", "pnpm-workspace.yaml"];
    for target in &targets {
        if name.join(target).exists() {
            return name.to_path_buf();
        }
    }

    if let Some(parent) = name.parent() {
        if parent == name {
            return original.to_path_buf();
        }
        find_prefix_up(parent, original)
    } else {
        original.to_path_buf()
    }
}

impl PrefixArgs {
    pub fn run(self, dir: &Path) -> miette::Result<()> {
        if self.global {
            return Err(PrefixError::GlobalUnsupported.into());
        }
        let prefix_dir = find_local_prefix(dir);
        println!("{}", prefix_dir.display());
        Ok(())
    }
}
