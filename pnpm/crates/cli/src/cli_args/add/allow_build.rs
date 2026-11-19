use crate::cli_args::approve_builds::ApprovalStore;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_package_manager::parse_allow_build_selector;

/// One parsed `--allow-build` selector (`<pkg>` allows, `!<pkg>` denies).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AllowBuildSelector<'a> {
    pub name: &'a str,
    pub allowed: bool,
}

/// Parse and validate `--allow-build` selectors (`<pkg>` or `!<pkg>`),
/// rejecting empty package names. A value may hold a comma-separated list,
/// see [`split_allow_build_value`].
pub(crate) fn parse_allow_build_selectors(
    allow_build: &[String],
) -> Result<Vec<AllowBuildSelector<'_>>, AllowBuildError> {
    allow_build
        .iter()
        .flat_map(|value| split_allow_build_value(value))
        .map(|selector| {
            let (name, allowed) = parse_allow_build_selector(selector);
            (!name.is_empty())
                .then_some(AllowBuildSelector { name, allowed })
                .ok_or(AllowBuildError::MissingPackage)
        })
        .collect()
}

/// Split an `--allow-build` value at its commas when every part is a
/// package selector (`[!]<name>[@<version>]`). Any other value is kept
/// whole: git and tarball approval keys are URLs, and a URL may contain a
/// comma.
fn split_allow_build_value(value: &str) -> Vec<&str> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.iter().all(|part| is_package_selector(part)) { parts } else { vec![value] }
}

fn is_package_selector(selector: &str) -> bool {
    let (spec, _) = parse_allow_build_selector(selector);
    let version_at = spec
        .char_indices()
        .skip(1)
        .find_map(|(index, ch)| (ch == '@').then_some(index));
    let (name, version) = version_at.map_or((spec, ""), |index| spec.split_at(index));
    let name_slashes = name.matches('/').count();
    let name_is_plain = if name.starts_with('@') { name_slashes == 1 } else { name_slashes == 0 };
    name_is_plain && !spec.contains([':', '#']) && !version.contains('/')
}

/// Honor `--allow-build`: reject any package the root project explicitly
/// disallows (`allowBuilds: false`), persist the allowed names to `store`,
/// and enable them for this install. Mirrors pnpm's `add` handler.
pub(crate) fn apply_allow_build(
    config: &mut Config,
    allow_build: &[String],
    store: ApprovalStore<'_>,
) -> miette::Result<()> {
    if allow_build.is_empty() {
        return Ok(());
    }
    let selectors = parse_allow_build_selectors(allow_build)?;
    let overlap: Vec<&str> = selectors
        .iter()
        .filter_map(|&AllowBuildSelector { name, allowed }| {
            (allowed && config.allow_builds.get(name) == Some(&false)).then_some(name)
        })
        .collect();
    if !overlap.is_empty() {
        return Err(AllowBuildError::OverridingIgnoredBuiltDependencies {
            dependencies: overlap.join(", "),
        }
        .into());
    }
    store.write_allow_builds(
        selectors
            .iter()
            .map(|&AllowBuildSelector { name, allowed }| (name, allowed)),
    )?;
    for AllowBuildSelector { name, allowed } in selectors {
        config.allow_builds.insert(name.to_string(), allowed);
    }
    Ok(())
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum AllowBuildError {
    #[display(
        "The following dependencies are ignored by the root project, but are allowed to be built by the current command: {dependencies}"
    )]
    #[diagnostic(
        code(ERR_PNPM_OVERRIDING_IGNORED_BUILT_DEPENDENCIES),
        help(
            "If you are sure you want to allow those dependencies to run installation scripts, remove them from the allowBuilds list (or change their value to true)."
        )
    )]
    OverridingIgnoredBuiltDependencies { dependencies: String },

    #[display(
        "The --allow-build flag is missing a package name. Please specify the package name(s) that are allowed to run installation scripts."
    )]
    #[diagnostic(code(ERR_PNPM_ALLOW_BUILD_MISSING_PACKAGE))]
    MissingPackage,
}
