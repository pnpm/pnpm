use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_versioning::VersioningSettings;
use pacquet_workspace_manifest_writer::update_manifest_field;
use std::collections::{BTreeMap, HashSet};

use crate::cli_args::{
    change::{releasable_pkg_names, to_engine_projects},
    recursive::discover_workspace_projects,
    version::selected_pkg_names,
};

/// The reserved name of the default lane: every package is on it unless
/// assigned elsewhere, packages on it release stable versions, and no
/// prerelease lane can take the name.
pub const MAIN_LANE: &str = "main";

/// `pnpm lane` — manage per-package release lanes: parallel release tracks
/// that emit `X.Y.Z-<lane>.N` prereleases while the main lane keeps releasing
/// stable versions. Membership lives under the `versioning.lanes` key of
/// pnpm-workspace.yaml; this command is a convenience editor for that key.
#[derive(Debug, Args)]
pub struct LaneArgs {
    /// The lane to move the `--filter`-selected packages onto (`main` moves
    /// them back to the default lane). With no name, shows lane membership.
    pub name: Option<String>,
}

/// Errors of `pnpm lane`. Codes and messages match the TypeScript CLI.
#[derive(Debug, Display, Error, Diagnostic)]
enum LaneError {
    #[display("pnpm lane is only supported in a workspace")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_ONLY))]
    WorkspaceOnly,

    #[display(
        r#"Select the packages to move with --filter, e.g. "pnpm lane alpha --filter <pkg>...""#
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_LANE_FILTER_REQUIRED))]
    FilterRequired,

    #[display("The filter selected no releasable packages")]
    #[diagnostic(code(ERR_PNPM_VERSIONING_NO_PACKAGES))]
    NoPackagesSelected,

    #[display(
        "Invalid lane name: {name}. Lane names may contain only alphanumerics and hyphens, and cannot be purely numeric."
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_INVALID_LANE_NAME))]
    InvalidLaneName { name: String },

    #[display(
        r#"{pkg_name} is already on the "{lane}" lane. Move it back with "pnpm lane main" first."#
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_ALREADY_ON_LANE))]
    AlreadyOnLane { pkg_name: String, lane: String },
}

impl LaneArgs {
    pub fn run(self, config: &Config) -> miette::Result<()> {
        let Some(workspace_dir) = config.workspace_dir.clone() else {
            return Err(LaneError::WorkspaceOnly.into());
        };

        let Some(lane_name) = self.name else {
            println!("{}", render_lanes(&config.versioning.lanes));
            return Ok(());
        };

        if config.filter.is_empty() {
            return Err(LaneError::FilterRequired.into());
        }
        let (projects, _) = discover_workspace_projects(&workspace_dir)?;
        let engine_projects = to_engine_projects(&projects);
        let releasable: HashSet<String> =
            releasable_pkg_names(&engine_projects, &config.versioning).into_iter().collect();
        let selected: Vec<String> = selected_pkg_names(&projects, config, &workspace_dir)?
            .into_iter()
            .filter(|name| releasable.contains(name))
            .collect();
        if selected.is_empty() {
            return Err(LaneError::NoPackagesSelected.into());
        }

        let mut settings: VersioningSettings = config.versioning.clone();
        let selected_lines: String = selected.iter().fold(String::new(), |mut lines, name| {
            use std::fmt::Write as _;
            writeln!(lines, "  {name}").expect("write to string");
            lines
        });
        let output = if lane_name == MAIN_LANE {
            for name in &selected {
                settings.lanes.shift_remove(name);
            }
            format!(
                r#"Moved to the main lane:
{selected_lines}The accumulated stable versions release on the next "pnpm version -r" run."#,
            )
        } else {
            // A purely numeric lane name is rejected because semver parses an
            // all-digit prerelease identifier as a number, which changes
            // sorting semantics.
            let valid_name = lane_name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
                && !lane_name.chars().all(|character| character.is_ascii_digit());
            if !valid_name {
                return Err(LaneError::InvalidLaneName { name: lane_name }.into());
            }
            for name in &selected {
                if let Some(existing) = settings.lanes.get(name)
                    && existing != &lane_name
                {
                    return Err(LaneError::AlreadyOnLane {
                        pkg_name: name.clone(),
                        lane: existing.clone(),
                    }
                    .into());
                }
                settings.lanes.insert(name.clone(), lane_name.clone());
            }
            format!("Moved to the \"{lane_name}\" lane:\n{selected_lines}")
        };

        let value = if settings.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::to_value(&settings).expect("versioning settings serialize to JSON")
        };
        update_manifest_field(&workspace_dir.join("pnpm-workspace.yaml"), "versioning", &value)
            .map_err(miette::Report::new)?;
        println!("{output}");
        Ok(())
    }
}

fn render_lanes(lanes: &indexmap::IndexMap<String, String>) -> String {
    if lanes.is_empty() {
        return "All packages are on the main lane.".to_string();
    }
    let mut by_lane: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (pkg_name, lane_name) in lanes {
        by_lane.entry(lane_name).or_default().push(pkg_name);
    }
    use std::fmt::Write as _;
    let mut output = String::from("Lanes:\n");
    for (lane_name, mut members) in by_lane {
        writeln!(output, "  {lane_name}:").expect("write to string");
        members.sort_unstable();
        for pkg_name in members {
            writeln!(output, "    {pkg_name}").expect("write to string");
        }
    }
    output
}
