use clap::Args;
use indexmap::IndexSet;
use miette::IntoDiagnostic;
use pnpm_config::Config;
use pnpm_modules_yaml::{
    Host,
    Modules,
    read_modules_manifest,
};
use pnpm_package_manager::allow_build_key_from_ignored_build;
use std::path::PathBuf;

/// Print the list of packages whose build scripts were not run during
/// installation.
#[derive(Debug, Args)]
pub struct IgnoredBuildsArgs {}

/// The automatically-ignored builds recorded in `.modules.yaml`, paired
/// with the manifest they came from.
pub(crate) struct IgnoredBuildsScan {
    /// The permission keys of the packages whose build scripts were
    /// ignored, deduplicated in first-seen order. `None` when there is no
    /// `.modules.yaml` or it records no `ignoredBuilds` field at all, the
    /// "cannot identify as no `node_modules` found" signal.
    pub names: Option<Vec<String>>,
    pub modules_dir: PathBuf,
    pub modules_manifest: Option<Modules>,
}

/// Read `.modules.yaml` and project its `ignoredBuilds` depPaths onto the
/// permission keys a user would approve them under.
pub(crate) fn get_automatically_ignored_builds(
    config: &Config,
) -> miette::Result<IgnoredBuildsScan> {
    let modules_dir = config.modules_dir.clone();
    let modules_manifest = read_modules_manifest::<Host>(&modules_dir).into_diagnostic()?;
    let names = modules_manifest
        .as_ref()
        .and_then(|manifest| manifest.ignored_builds.as_ref())
        .map(|ignored| {
            ignored
                .iter()
                .map(pnpm_modules_yaml::DepPath::as_str)
                .map(allow_build_key_from_ignored_build)
                .collect::<IndexSet<String>>()
                .into_iter()
                .collect()
        });
    Ok(IgnoredBuildsScan { names, modules_dir, modules_manifest })
}

pub(crate) fn render_ignored_builds(config: &Config) -> miette::Result<String> {
    // pnpm preserves permission insertion order; pacquet's
    // `Config::allow_builds` is a `HashMap`, so the source order is already
    // lost. Sort for a deterministic, reproducible listing.
    let mut disallowed_builds: Vec<String> = config.allow_builds
        .iter()
        .filter(|&(_, &allowed)| !allowed)
        .map(|(pkg, _)| pkg.clone())
        .collect();
    disallowed_builds.sort();

    let mut automatically_ignored_builds = get_automatically_ignored_builds(config)?.names;
    if let Some(list) = automatically_ignored_builds.as_mut() {
        list.retain(|build| !disallowed_builds.contains(build));
    }

    let mut output = String::from("Automatically ignored builds during installation:\n");
    match &automatically_ignored_builds {
        None => output.push_str("  Cannot identify as no node_modules found"),
        Some(list) if list.is_empty() => output.push_str("  None"),
        Some(list) => {
            output.push_str("  ");
            output.push_str(&list.join("\n  "));
            output.push_str(
                "\nhint: To allow the execution of build scripts for a package, add its name to \"permissions\" with \"build: true\", then run \"pnpm rebuild\".\nhint: For example:\nhint: permissions:\nhint:   esbuild:\nhint:     build: true\nhint: If you don't want to build a package, set \"build\" to \"false\" instead.",
            );
        }
    }
    output.push('\n');

    if !disallowed_builds.is_empty() {
        output.push_str("\nExplicitly ignored package builds:\n  ");
        output.push_str(&disallowed_builds.join("\n  "));
        output.push('\n');
    }

    Ok(output)
}

#[cfg(test)]
mod tests;
