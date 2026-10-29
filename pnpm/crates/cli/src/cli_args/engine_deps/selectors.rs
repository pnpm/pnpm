use std::collections::HashSet;

use pnpm_matcher::{Matcher, create_matcher};
use pnpm_package_manager::{ParsedSelector, UpdateError, parse_update_param};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};

use super::{DeclaredEngineDep, EngineUpdateOptions};
use crate::{cli_args::pipelines::InstallFamilySelection, state::State};

pub(super) struct EngineSelectorPlan<'a> {
    pub(super) raw_packages: &'a [String],
    pub(super) parsed_selectors: Vec<ParsedSelector>,
    pub(super) selector_matchers: Vec<Matcher>,
    pub(super) matcher: Option<Matcher>,
    pub(super) ignore_matcher: Option<Matcher>,
    pub(super) matched_engine_indices: HashSet<usize>,
    pub(super) regular_dep_indices: HashSet<usize>,
    pub(super) has_regular_deps: bool,
}

pub(super) fn remaining_regular_packages(
    packages: &[String],
    plan: &EngineSelectorPlan<'_>,
) -> Vec<String> {
    let is_kept = |idx: usize| {
        !plan.matched_engine_indices.contains(&idx) || plan.regular_dep_indices.contains(&idx)
    };
    let has_positive = plan.parsed_selectors
        .iter()
        .any(|sel| !sel.pattern.starts_with('!'));
    let has_kept_positive = plan.parsed_selectors
        .iter()
        .enumerate()
        .any(|(idx, sel)| !sel.pattern.starts_with('!') && is_kept(idx));
    let keep_negative = if has_positive { has_kept_positive } else { plan.has_regular_deps };
    packages
        .iter()
        .enumerate()
        .filter(|(idx, _)| {
            if plan.parsed_selectors[*idx].pattern.starts_with('!') {
                keep_negative
            } else {
                is_kept(*idx)
            }
        })
        .map(|(_, pkg)| pkg.clone())
        .collect()
}

pub(super) fn build_selector_plan<'a>(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    packages: &'a [String],
) -> EngineSelectorPlan<'a> {
    let parsed_selectors: Vec<_> = packages
        .iter()
        .map(|spec| parse_update_param(spec))
        .collect();
    let selector_matchers = parsed_selectors
        .iter()
        .map(|sel| {
            let positive = sel.pattern.strip_prefix('!').unwrap_or(&sel.pattern);
            create_matcher(std::slice::from_ref(&positive.to_string()))
        })
        .collect();
    let patterns: Vec<String> = parsed_selectors
        .iter()
        .map(|sel| sel.pattern.clone())
        .collect();
    let matcher = (!patterns.is_empty()).then(|| create_matcher(&patterns));
    let ignore_matcher = state.config.update_config.ignore_dependencies
        .as_deref()
        .filter(|patterns| !patterns.is_empty())
        .map(create_matcher);
    let mut plan = EngineSelectorPlan {
        raw_packages: packages,
        parsed_selectors,
        selector_matchers,
        matcher,
        ignore_matcher,
        matched_engine_indices: HashSet::new(),
        regular_dep_indices: HashSet::new(),
        has_regular_deps: false,
    };
    scan_regular_deps(state, selection, &mut plan);
    plan
}

fn scan_regular_deps(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    plan: &mut EngineSelectorPlan<'_>,
) {
    match selection {
        Some(sel) => {
            for project in &sel.projects {
                if sel.selected_dirs.contains(&project.root_dir) {
                    scan_manifest_regular_deps(&project.manifest, plan);
                }
            }
        }
        None => scan_manifest_regular_deps(&state.manifest, plan),
    }
    if !plan.parsed_selectors.is_empty()
        && plan.has_regular_deps
        && let Some(snapshots) = state.lockfile
            .get()
            .ok()
            .flatten()
            .and_then(|lf| lf.snapshots.as_ref())
    {
        for key in snapshots.keys() {
            record_matching_selectors(
                &key.name.to_string(),
                &plan.selector_matchers,
                &mut plan.regular_dep_indices,
            );
        }
    }
}

fn scan_manifest_regular_deps(manifest: &PackageManifest, plan: &mut EngineSelectorPlan<'_>) {
    for (dep_name, _) in manifest.dependencies([
        DependencyGroup::Prod,
        DependencyGroup::Dev,
        DependencyGroup::Optional,
        DependencyGroup::Peer,
    ]) {
        plan.has_regular_deps = true;
        record_matching_selectors(dep_name, &plan.selector_matchers, &mut plan.regular_dep_indices);
    }
}

fn record_matching_selectors(name: &str, matchers: &[Matcher], matched: &mut HashSet<usize>) {
    for (idx, matcher) in matchers.iter().enumerate() {
        if matcher.matches(name) {
            matched.insert(idx);
        }
    }
}

pub(super) fn should_update_engine_dep(
    dep: &DeclaredEngineDep,
    opts: &EngineUpdateOptions<'_>,
    selector_plan: &EngineSelectorPlan<'_>,
) -> bool {
    if opts.packages.is_empty()
        && selector_plan.ignore_matcher
            .as_ref()
            .is_some_and(|ignore| ignore.matches(&dep.name))
    {
        return false;
    }
    selector_plan.matcher
        .as_ref()
        .is_none_or(|matcher| matcher.matches(&dep.name))
}

pub(super) fn match_engine_selector(
    dep_name: &str,
    latest: bool,
    selector_plan: &mut EngineSelectorPlan<'_>,
) -> miette::Result<Option<String>> {
    let mut requested: Option<String> = None;
    for (idx, (sel, matcher)) in selector_plan.parsed_selectors
        .iter()
        .zip(&selector_plan.selector_matchers)
        .enumerate()
    {
        if !sel.pattern.starts_with('!') && matcher.matches(dep_name) {
            selector_plan.matched_engine_indices.insert(idx);
            if requested.is_none() {
                requested.clone_from(&sel.version);
            }
        }
    }
    if latest && requested.is_some() {
        return Err(latest_with_spec_error(
            selector_plan.raw_packages,
            &selector_plan.parsed_selectors,
        ));
    }
    Ok(requested)
}

fn latest_with_spec_error(
    packages: &[String],
    parsed_selectors: &[ParsedSelector],
) -> miette::Report {
    let with_spec = packages
        .iter()
        .zip(parsed_selectors)
        .filter(|(_, selector)| selector.version.is_some())
        .map(|(raw, _)| raw.as_str())
        .collect::<Vec<_>>();
    miette::Report::new(UpdateError::LatestWithSpec(with_spec.join(", ")))
}
