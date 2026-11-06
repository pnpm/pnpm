//! `pnpm fund` — list the funding URLs of the installed dependencies, or
//! open the funding URL of one of them. Mirrors `npm fund`.

mod funding;
mod human;
mod open;
mod projects;
mod report;

use crate::cli_args::list::print_output;
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_network_web_auth::OpenUrl;
use projects::load_projects;
use report::FundingReport;
use std::{num::NonZeroUsize, path::Path};

#[derive(Debug, Args)]
pub struct FundArgs {
    /// Output the funding information in JSON format.
    #[clap(long)]
    pub json: bool,

    /// Which of the package's funding URLs to open, counting from 1, when
    /// it lists several.
    #[clap(long)]
    pub which: Option<NonZeroUsize>,

    /// The installed package whose funding URL to open.
    pub package: Option<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum FundError {
    #[display("No valid funding method available for: {spec}")]
    #[diagnostic(code(ERR_PNPM_NO_FUNDING))]
    NoFunding { spec: String },

    #[display("Failed to serialize the funding report: {_0}")]
    #[diagnostic(code(ERR_PNPM_FUND_JSON))]
    Json(serde_json::Error),
}

impl FundArgs {
    pub fn run<Sys: OpenUrl>(
        self,
        config: &Config,
        dir: &Path,
        recursive: bool,
    ) -> miette::Result<()> {
        let projects = load_projects(config, dir, recursive)?;
        if let Some(spec) = &self.package {
            return open::open_package_funding::<Sys>(spec, self.which, dir, &projects);
        }
        let reports: Vec<FundingReport> = projects
            .iter()
            .map(FundingReport::build)
            .collect();
        let output = if self.json {
            render_json(&reports, recursive).map_err(FundError::Json)?
        } else {
            reports
                .iter()
                .map(human::render_human)
                .collect::<Vec<_>>()
                .join("\n")
        };
        print_output(&output);
        Ok(())
    }
}

/// One project renders as the object `npm fund --json` prints; a recursive
/// run renders every selected project's object in one array.
fn render_json(reports: &[FundingReport], recursive: bool) -> serde_json::Result<String> {
    match reports {
        [report] if !recursive => serde_json::to_string_pretty(report),
        _ => serde_json::to_string_pretty(reports),
    }
}

#[cfg(test)]
mod tests;
