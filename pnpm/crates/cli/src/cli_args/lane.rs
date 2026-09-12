use crate::cli_args::{
    change::{releasable_projects, to_engine_projects},
    recursive::discover_workspace_projects,
    version::selected_projects,
};
use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_versioning::VersioningSettings;
use pnpm_workspace_manifest_writer::update_manifest_field;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The reserved name of the default lane: every package is on it unless
/// assigned elsewhere, packages on it release stable versions, and no
/// prerelease lane can take the name.
pub const MAIN_LANE: &str = "main";

/// Manage per-package release lanes: parallel release tracks that emit
/// `X.Y.Z-<lane>.N` prereleases while the main lane keeps releasing stable
/// versions.
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
        "Invalid lane name: {name}. \"main\" is the reserved default lane; spell it in lowercase to move packages back onto it."
    )]
    #[diagnostic(code(ERR_PNPM_VERSIONING_INVALID_LANE_NAME))]
    ReservedLaneName { name: String },

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
        let (projects, _) = discover_workspace_projects(&workspace_dir, config)?;
        let engine_projects = to_engine_projects(&projects);
        let refs = pnpm_versioning::index_project_refs(&engine_projects, &workspace_dir);
        let selected =
            selected_lane_entries(&projects, &engine_projects, config, &workspace_dir, &refs)?;
        if selected.is_empty() {
            return Err(LaneError::NoPackagesSelected.into());
        }

        let lane_by_dir = lanes_by_dir(&config.versioning, &refs);

        let mut settings: VersioningSettings = config.versioning.clone();
        let selected_lines = selected_reference_lines(&selected);
        let output = if lane_name == MAIN_LANE {
            clear_lanes(&mut settings, &selected, &lane_by_dir);
            format!(
                r#"Moved to the main lane:
{selected_lines}The accumulated stable versions release on the next "pnpm version -r" run."#,
            )
        } else {
            check_lane_name(&lane_name)?;
            assign_lane(&mut settings, &selected, &lane_by_dir, &lane_name)?;
            format!("Moved to the \"{lane_name}\" lane:\n{selected_lines}")
        };

        update_manifest_field(
            &workspace_dir.join("pnpm-workspace.yaml"),
            "versioning",
            &versioning_value(&settings),
        )
        .map_err(miette::Report::new)?;
        println!("{output}");
        Ok(())
    }
}

/// (name, dir) of every selected releasable project, with the reference an
/// intent or lanes entry should use for it.
fn selected_lane_entries(
    projects: &[pnpm_workspace::Project],
    engine_projects: &[pnpm_versioning::WorkspaceProject],
    config: &Config,
    workspace_dir: &std::path::Path,
    refs: &pnpm_versioning::ProjectRefIndex,
) -> miette::Result<Vec<(String, String, String)>> {
    let releasable_dirs: HashSet<String> =
        releasable_projects(engine_projects, workspace_dir, &config.versioning)
            .into_iter()
            .map(|project| project.dir)
            .collect();
    Ok(selected_projects(projects, config, workspace_dir)?
        .into_iter()
        .filter_map(|(name, dir)| selected_entry(refs, &releasable_dirs, name, dir))
        .collect())
}

fn selected_reference_lines(selected: &[(String, String, String)]) -> String {
    selected.iter().fold(String::new(), |mut lines, (_, _, reference)| {
        use std::fmt::Write as _;
        writeln!(lines, "  {reference}").expect("write to string");
        lines
    })
}

/// The `versioning` setting to write back: absent once nothing is set.
fn versioning_value(settings: &VersioningSettings) -> serde_json::Value {
    if settings.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::to_value(settings).expect("versioning settings serialize to JSON")
    }
}

/// The lane each project directory is on. Existing entries may reference
/// projects by name or by directory; resolving them here keys assignments
/// and removals on the project rather than on the spelling.
fn lanes_by_dir(
    versioning: &VersioningSettings,
    refs: &pnpm_versioning::ProjectRefIndex,
) -> HashMap<String, (String, String)> {
    let mut lane_by_dir: HashMap<String, (String, String)> = HashMap::new();
    for (key, lane) in &versioning.lanes {
        for dir in refs.ref_to_dirs(key) {
            lane_by_dir.insert(dir, (key.clone(), lane.clone()));
        }
    }
    lane_by_dir
}

/// Take every selected project off the lane it is on, whichever spelling
/// recorded it.
fn clear_lanes(
    settings: &mut VersioningSettings,
    selected: &[(String, String, String)],
    lane_by_dir: &HashMap<String, (String, String)>,
) {
    for (_, dir, _) in selected {
        if let Some((key, _)) = lane_by_dir.get(dir) {
            settings.lanes.shift_remove(key);
        }
    }
}

/// One selected project as `(name, dir, reference)`. `None` for a
/// project without a usable name, and for one no lane can release.
///
/// A name shared by several projects is referenced by directory, so the
/// recorded entry names exactly one project.
fn selected_entry(
    refs: &pnpm_versioning::ProjectRefIndex,
    releasable_dirs: &HashSet<String>,
    name: Option<String>,
    dir: String,
) -> Option<(String, String, String)> {
    let name = name?;
    if !releasable_dirs.contains(&dir) {
        return None;
    }
    let reference =
        if refs.name_to_dirs(&name).len() > 1 { format!("./{dir}") } else { name.clone() };
    Some((name, dir, reference))
}

/// A purely numeric lane name is rejected because semver parses an
/// all-digit prerelease identifier as a number, which changes sorting
/// semantics.
fn check_lane_name(lane_name: &str) -> Result<(), LaneError> {
    if lane_name.eq_ignore_ascii_case(MAIN_LANE) {
        return Err(LaneError::ReservedLaneName { name: lane_name.to_owned() });
    }
    let valid_name =
        lane_name.chars().all(|character| character.is_ascii_alphanumeric() || character == '-')
            && !lane_name.chars().all(|character| character.is_ascii_digit());
    if !valid_name {
        return Err(LaneError::InvalidLaneName { name: lane_name.to_owned() });
    }
    Ok(())
}

/// Record each selected project on the lane. A project already on a
/// different lane is an error: moving it would silently reset the
/// prerelease series it is on.
fn assign_lane(
    settings: &mut VersioningSettings,
    selected: &[(String, String, String)],
    lane_by_dir: &HashMap<String, (String, String)>,
    lane_name: &str,
) -> Result<(), LaneError> {
    for (_, dir, reference) in selected {
        match lane_by_dir.get(dir) {
            Some((_, existing)) if existing != lane_name => {
                return Err(LaneError::AlreadyOnLane {
                    pkg_name: reference.clone(),
                    lane: existing.clone(),
                });
            }
            Some(_) => {}
            None => {
                settings.lanes.insert(reference.clone(), lane_name.to_owned());
            }
        }
    }
    Ok(())
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
