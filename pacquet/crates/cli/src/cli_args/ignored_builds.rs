use clap::Args;
use indexmap::IndexSet;
use miette::IntoDiagnostic;
use pacquet_config::Config;
use pacquet_modules_yaml::{Host, Modules, read_modules_manifest};
use pacquet_package_manager::allow_build_key_from_ignored_build;
use std::path::PathBuf;

/// `pacquet ignored-builds` — print the list of packages with blocked
/// build scripts. Ports pnpm's
/// [`ignored-builds`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/commands/src/policy/ignoredBuilds.ts).
#[derive(Debug, Args)]
pub struct IgnoredBuildsArgs {}

/// The automatically-ignored builds recorded in `.modules.yaml`, paired
/// with the manifest they came from. Mirrors pnpm's
/// `getAutomaticallyIgnoredBuilds` return at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/commands/src/policy/getAutomaticallyIgnoredBuilds.ts#L8-L12>.
pub(crate) struct IgnoredBuildsScan {
    /// The `allowBuilds` keys of the packages whose build scripts were
    /// ignored, deduplicated in first-seen order. `None` when there is no
    /// `.modules.yaml` or it records no `ignoredBuilds` field at all —
    /// upstream's "cannot identify as no `node_modules` found" signal.
    pub names: Option<Vec<String>>,
    pub modules_dir: PathBuf,
    pub modules_manifest: Option<Modules>,
}

/// Read `.modules.yaml` and project its `ignoredBuilds` depPaths onto the
/// `allowBuilds` keys a user would approve them under. Ports pnpm's
/// [`getAutomaticallyIgnoredBuilds`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/building/commands/src/policy/getAutomaticallyIgnoredBuilds.ts#L14-L32).
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
                .map(|dep_path| allow_build_key_from_ignored_build(dep_path.as_str()))
                .collect::<IndexSet<String>>()
                .into_iter()
                .collect()
        });
    Ok(IgnoredBuildsScan { names, modules_dir, modules_manifest })
}

pub(crate) fn render_ignored_builds(config: &Config) -> miette::Result<String> {
    // pnpm preserves `allowBuilds` insertion order; pacquet's
    // `Config::allow_builds` is a `HashMap`, so the source order is already
    // lost. Sort for a deterministic, reproducible listing.
    let mut disallowed_builds: Vec<String> = config
        .allow_builds
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
                "\nhint: To allow the execution of build scripts for a package, add its name to \"allowBuilds\" and set to \"true\", then run \"pnpm rebuild\".\nhint: For example:\nhint: allowBuilds:\nhint:   esbuild: true\nhint: If you don't want to build a package, set it to \"false\" instead.",
            );
        }
    }
    output.push('\n');

    if !disallowed_builds.is_empty() {
        output.push_str("\nExplicitly ignored package builds (via allowBuilds):\n  ");
        output.push_str(&disallowed_builds.join("\n  "));
        output.push('\n');
    }

    Ok(output)
}

#[cfg(test)]
mod tests;
