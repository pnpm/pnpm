//! `pacquet list` / `ls`. Ports the global (`-g`) branch of pnpm's
//! [`list`](https://github.com/pnpm/pnpm/blob/1819226b51/deps/inspection/commands/src/listing/list.ts)
//! command. Listing a local project's tree is not ported yet.

use clap::Args;
use pacquet_config::Config;
use pacquet_global::{ListReportAs, list_global_packages};

/// `pacquet list` (alias `ls`).
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Restrict the listing to dependencies matching these names/patterns.
    pub packages: Vec<String>,

    /// List packages in the global install directory instead of the
    /// current project.
    #[clap(short = 'g', long)]
    pub global: bool,

    /// Show extra information (description, repository, homepage, path).
    #[clap(long)]
    pub long: bool,

    /// Output as JSON.
    #[clap(long)]
    pub json: bool,

    /// Output a flat, parseable list of paths.
    #[clap(long)]
    pub parseable: bool,

    /// How deep to inspect the dependency tree. Accepted for parity; the
    /// global listing only reports direct dependencies (depth 0).
    #[clap(long)]
    pub depth: Option<usize>,
}

impl ListArgs {
    /// Print the listing. Only `--global` is supported for now.
    pub fn run(self, config: &Config) -> miette::Result<()> {
        if !self.global {
            return Err(miette::miette!(
                "`pacquet list` without --global is not supported yet; only `pacquet list -g` (global packages) has been ported to pacquet."
            ));
        }
        let global_pkg_dir = config.global_pkg_dir.clone().ok_or_else(|| {
            miette::miette!(
                code = "ERR_PNPM_NO_GLOBAL_BIN_DIR",
                "Unable to find the global packages directory"
            )
        })?;

        let report_as = if self.json {
            ListReportAs::Json
        } else if self.parseable {
            ListReportAs::Parseable
        } else {
            ListReportAs::Tree
        };

        let output = list_global_packages(&global_pkg_dir, &self.packages, report_as, self.long);
        println!("{output}");
        Ok(())
    }
}
