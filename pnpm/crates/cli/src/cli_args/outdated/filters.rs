use super::{
    Config, DependencyGroup, Matcher, OutdatedArgs, OutdatedQuery, PackageManifest, TargetVersion,
    create_matcher, ignored_dependencies_matcher, project_dir,
};
use crate::cli_args::{
    ecosystem_deps::{
        EcosystemFilterKind, declared_ecosystem_dep_names, patterns_for_ecosystem,
        validate_ecosystem_selectors,
    },
    engine_deps::declared_engine_deps,
};

/// The dependency groups and the name matchers one outdated query walks.
pub(super) struct OutdatedFilters {
    pub(super) include: Vec<DependencyGroup>,
    pub(super) match_names: Option<Matcher>,
    pub(super) ignore_names: Option<Matcher>,
    pub(super) full_metadata: bool,
}

impl OutdatedFilters {
    pub(super) fn validated<'a>(
        args: &OutdatedArgs,
        config: &Config,
        package_patterns: &[String],
        manifests: impl IntoIterator<Item = &'a PackageManifest>,
        recursive: bool,
    ) -> miette::Result<Self> {
        validate_ecosystem_selectors(package_patterns, config)?;
        let include = args.dependency_options.include(config.optional);
        let mut manifests = manifests.into_iter().peekable();
        if !recursive || manifests.peek().is_some() {
            validate_package_patterns(manifests, config, package_patterns, &include, recursive)?;
        }
        let normalized_patterns = normalize_filter_patterns(package_patterns)?;
        Ok(Self {
            include,
            match_names: (!normalized_patterns.is_empty()).then(|| {
                create_matcher(&normalized_patterns)
            }),
            ignore_names: ignored_dependencies_matcher(config),
            full_metadata: args.output.long,
        })
    }

    pub(super) fn query(&self, target_version: TargetVersion) -> OutdatedQuery<'_> {
        OutdatedQuery {
            target_version,
            include_direct: &self.include,
            match_names: self.match_names.as_ref(),
            ignore_names: self.ignore_names.as_ref(),
            include_deprecated: true,
            full_metadata: self.full_metadata,
        }
    }
}

fn normalize_filter_patterns(package_patterns: &[String]) -> miette::Result<Vec<String>> {
    if package_patterns.is_empty() {
        return Ok(Vec::new());
    }
    let mut all = patterns_for_ecosystem(package_patterns, EcosystemFilterKind::Node)?;
    for pattern in patterns_for_ecosystem(package_patterns, EcosystemFilterKind::Cargo)? {
        if !all.contains(&pattern) {
            all.push(pattern);
        }
    }
    for pattern in patterns_for_ecosystem(package_patterns, EcosystemFilterKind::Python)? {
        if !all.contains(&pattern) {
            all.push(pattern);
        }
    }
    Ok(all)
}

pub(super) fn validate_package_patterns<'a>(
    manifests: impl IntoIterator<Item = &'a PackageManifest>,
    config: &Config,
    package_patterns: &[String],
    include: &[DependencyGroup],
    recursive: bool,
) -> miette::Result<()> {
    let positive_patterns: Vec<&String> = package_patterns
        .iter()
        .filter(|pattern| !pattern.starts_with('!'))
        .collect();
    if positive_patterns.is_empty() {
        return Ok(());
    }
    let deps = collect_all_declared_names(manifests, config, include)?;
    let unmatched = positive_patterns
        .iter()
        .any(|pattern| {
            let normalized = normalize_single_pattern(pattern);
            let matcher = create_matcher(std::slice::from_ref(&normalized));
            !deps
                .iter()
                .any(|dep| matcher.matches(dep))
        });
    if unmatched {
        let message = if recursive {
            "None of the specified packages were found in the dependencies of any of the projects."
        } else {
            "None of the specified packages were found in the dependencies."
        };
        return Err(miette::miette!(code = "ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES", "{message}"));
    }
    Ok(())
}

fn collect_all_declared_names<'a>(
    manifests: impl IntoIterator<Item = &'a PackageManifest>,
    config: &Config,
    include: &[DependencyGroup],
) -> miette::Result<Vec<String>> {
    let mut deps = Vec::new();
    for manifest in manifests {
        deps.extend(
            manifest
                .dependencies(include.iter().copied())
                .map(|(name, _)| name.to_string()),
        );
        deps.extend(
            declared_engine_deps(manifest, config, include).into_iter().map(|dep| dep.name),
        );
        deps.extend(declared_ecosystem_dep_names(project_dir(manifest), config)?);
    }
    Ok(deps)
}

fn normalize_single_pattern(pattern: &str) -> String {
    if let Ok(mut names) =
        patterns_for_ecosystem(&[pattern.to_string()], EcosystemFilterKind::Cargo)
        && let Some(first) = names.pop()
        && pattern.to_ascii_lowercase().starts_with("pkg:cargo/")
    {
        return format!("crate:{first}");
    }
    if let Ok(mut names) =
        patterns_for_ecosystem(&[pattern.to_string()], EcosystemFilterKind::Python)
        && let Some(first) = names.pop()
        && pattern.to_ascii_lowercase().starts_with("pkg:pypi/")
    {
        return format!("pypi:{first}");
    }
    if let Ok(mut names) = patterns_for_ecosystem(&[pattern.to_string()], EcosystemFilterKind::Node)
        && let Some(first) = names.pop()
        && pattern.to_ascii_lowercase().starts_with("pkg:npm/")
    {
        return first;
    }
    pattern.to_string()
}
