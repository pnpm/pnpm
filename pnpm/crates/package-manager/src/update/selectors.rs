use super::UpdateError;
use pnpm_matcher::create_matcher;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_deps_resolver::{UpdateTargets, VersionLine, real_package_name_of};

/// A CLI selector split into its name pattern and optional version part.
pub(super) struct ParsedSelector {
    pub(super) pattern: String,
    pub(super) version: Option<String>,
}
pub(super) fn parse_update_param(input: &str) -> ParsedSelector {
    let search_start = if input.starts_with('!') { 2 } else { 1 };
    let at_index = input
        .get(search_start..)
        .and_then(|rest| rest.find('@'))
        .map(|offset| offset + search_start);
    match at_index {
        Some(idx) => ParsedSelector {
            pattern: input[..idx].to_string(),
            version: Some(input[idx + 1..].to_string()),
        },
        None => ParsedSelector { pattern: input.to_string(), version: None },
    }
}
pub(super) fn parse_selectors(packages: &[String]) -> Vec<ParsedSelector> {
    packages.iter().map(|input| parse_update_param(input)).collect()
}
/// `--latest` forbids versioned selectors.
pub(super) fn reject_versioned_latest_selectors(
    packages: &[String],
    selectors: &[ParsedSelector],
) -> Result<(), UpdateError> {
    let with_spec = packages
        .iter()
        .zip(selectors)
        .filter(|(_, selector)| selector.version.is_some())
        .map(|(raw, _)| raw.as_str())
        .collect::<Vec<_>>();
    if with_spec.is_empty() {
        return Ok(());
    }
    Err(UpdateError::LatestWithSpec(with_spec.join(", ")))
}
/// The selectors an update selector stands for. An `npm:` selector
/// contributes a second one for the aliased package, because that -- not
/// the alias -- is the name the resolver resolves the edge under; it
/// carries the aliased spec's own version so the expansion scopes the same
/// version line the user asked for.
pub(super) fn expand_update_selectors(selectors: &[ParsedSelector]) -> Vec<ParsedSelector> {
    let mut expanded = Vec::with_capacity(selectors.len());
    for selector in selectors {
        expanded.push(ParsedSelector {
            pattern: selector.pattern.clone(),
            version: selector.version.clone(),
        });
        let Some(aliased) =
            selector.version.as_deref().and_then(|version| version.strip_prefix("npm:"))
        else {
            continue;
        };
        let alias = parse_update_param(aliased);
        let pattern = if selector.pattern.starts_with('!') {
            format!("!{}", alias.pattern)
        } else {
            alias.pattern
        };
        expanded.push(ParsedSelector { pattern, version: alias.version });
    }
    expanded
}
/// Record `name` as an update target once per selector that claims it: a
/// selector pinning an exact version scopes the target to that version's
/// line, while a bare or ranged one widens it to every version. Negated
/// selectors exclude names, never versions, so they claim nothing here --
/// the matcher that found `name` has already applied them.
pub(super) fn insert_update_target(
    targets: &mut UpdateTargets,
    selectors: &[ParsedSelector],
    name: &str,
) {
    let mut claimed = false;
    for selector in selectors.iter().filter(|selector| !selector.pattern.starts_with('!')) {
        if !matcher_one(&selector.pattern).matches(name) {
            continue;
        }
        claimed = true;
        targets.insert(name.to_string(), selector.version.as_deref().and_then(VersionLine::parse));
    }
    if !claimed {
        targets.insert(name.to_string(), None);
    }
}
/// Whether any of `manifests` declares a dependency `selector` names, so the
/// update has a manifest entry to write the requested version into.
pub(super) fn selector_matches_a_direct_dependency(
    selector: &ParsedSelector,
    manifests: &[&PackageManifest],
    include_direct: &[DependencyGroup],
) -> bool {
    let matcher = matcher_one(&selector.pattern);
    manifests.iter().any(|manifest| {
        manifest.dependencies(include_direct.iter().copied()).any(|(name, _)| matcher.matches(name))
    })
}
/// `pacquet update <dep>@<version>` where `<dep>` matches no direct dependency
/// has nowhere to record the version. An update resolves such a target the way
/// a fresh install would -- which a command-line version cannot influence -- so
/// honoring the request would mean writing a lockfile entry no manifest backs,
/// and the next fresh resolve would undo it. Neither npm nor Yarn accepts a
/// version here either. Fail rather than resolve to something else and leave
/// the caller a zero exit status to read.
///
/// A range or a tag names no single version to record, so updating within the
/// dependents' ranges is a reasonable reading of it: those only warn. A
/// negated selector excludes names rather than requesting one, so it is not
/// judged here at all.
///
/// The override the hint recommends is scoped to the dependents' declared
/// range so it cannot violate any consumer's range; that range lives in the
/// dependents' manifests, which this layer does not read, hence the
/// placeholder.
pub(super) fn reject_versions_of_indirect_update_specs<Reporter: self::Reporter>(
    selectors: &[ParsedSelector],
    manifests: &[&PackageManifest],
    include_direct: &[DependencyGroup],
    prefix: &str,
) -> Result<(), UpdateError> {
    let mut pinned = Vec::new();
    for selector in selectors {
        let Some(version) = selector.version.as_deref() else { continue };
        // A negated selector excludes names; a version on one asks for nothing.
        if selector.pattern.starts_with('!')
            || selector_matches_a_direct_dependency(selector, manifests, include_direct)
        {
            continue;
        }
        let pattern = &selector.pattern;
        if node_semver::Version::parse(version).is_err() {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    r#""{pattern}" is not a direct dependency, so the requested "{version}" is ignored — "{pattern}" is updated to what a fresh install would resolve."#,
                ),
                prefix: prefix.to_string(),
            }));
            continue;
        }
        pinned.push((pattern.clone(), version.to_string()));
    }
    if pinned.is_empty() {
        return Ok(());
    }
    Err(indirect_version_error(&pinned))
}
pub(super) fn indirect_version_error(pinned: &[(String, String)]) -> UpdateError {
    let subjects = pinned
        .iter()
        .map(|(pattern, version)| format!(r#""{pattern}" (requested "{version}")"#))
        .collect::<Vec<_>>()
        .join(", ");
    let tail = if pinned.len() == 1 {
        "is not a direct dependency, so the requested version cannot"
    } else {
        "are not direct dependencies, so the requested versions cannot"
    };
    let overrides = pinned
        .iter()
        .map(|(pattern, version)| format!("    {pattern}@<declared range>: {version}"))
        .collect::<Vec<_>>()
        .join("\n");
    let names = pinned.iter().map(|(pattern, _)| pattern.as_str()).collect::<Vec<_>>().join(" ");
    UpdateError::UpdateVersionOnIndirectDep {
        message: format!("{subjects} {tail} be recorded."),
        hint: format!(
            "An update resolves a transitive dependency the way a fresh install would, so a version on the command line has no effect on it. To pin one, add an override scoped to the range its dependents declare to pnpm-workspace.yaml:\n\n  overrides:\n{overrides}\n\nTo update it within the range its dependents already declare, drop the version: pnpm update {names}",
        ),
    }
}
/// The name an update target for `matched` is keyed by. A manifest keys a
/// dependency by its alias, but the resolver matches update targets — and
/// [`UpdateSeedPolicy::DropOnly`](crate::UpdateSeedPolicy::DropOnly) keys them — by the package name the edge
/// resolves under, which an `npm:` or `jsr:` selector states separately from
/// the alias. Falls back to the alias, which is the name for every other
/// selector shape.
pub(super) fn update_target_name(selectors: &[ParsedSelector], matched: &str) -> String {
    selectors
        .iter()
        .filter(|selector| matcher_one(&selector.pattern).matches(matched))
        .filter_map(|selector| {
            real_package_name_of(Some(matched), Some(selector.version.as_deref()?))
        })
        .find(|name| name.as_ref() != matched)
        .map_or_else(|| matched.to_string(), std::borrow::Cow::into_owned)
}
/// Compile a single pattern into a matcher. Used to map a matched direct
/// dependency back to the selector that claimed it (so a versioned
/// selector's version is applied to the right dep).
pub(super) fn matcher_one(pattern: &str) -> pnpm_matcher::Matcher {
    create_matcher(std::slice::from_ref(&pattern.to_string()))
}
