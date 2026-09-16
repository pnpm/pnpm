use std::collections::BTreeSet;

/// How an `allowBuilds` key names a Python distribution: the [Package
/// URL] type for `PyPI`, which every ecosystem pnpm installs has one of.
///
/// [Package URL]: https://github.com/package-url/purl-spec
const PYPI_PURL: &str = "pkg:pypi/";

/// The build requirements the configuration has not approved to run.
pub(crate) fn unapproved(
    config: &pnpm_config::Config,
    requires: &[pep508_rs::Requirement],
) -> Vec<String> {
    if config.dangerously_allow_all_builds {
        return Vec::new();
    }
    let approvals = Approvals::of(config);
    let mut names = requires
        .iter()
        .filter(|requirement| !approvals.any_version.contains(&requirement.name))
        .map(|requirement| approvals.describe(&requirement.name))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// The distributions `allowBuilds` approves to run in a build.
///
/// A key says which ecosystem's package it names, as a Package URL.
/// Approving is a statement about one piece of code, and a bare name is
/// not one: npm and `PyPI` both publish `esbuild`, `ruff` and `black`, so a
/// key naming no ecosystem would let approving a build script approve a
/// build backend nobody looked at.
///
/// A key naming no version approves the distribution however it
/// resolves, which is the only form a check made before resolving can
/// answer. A version-qualified key names a release, and a build approved
/// by one says so rather than silently doing nothing.
struct Approvals {
    any_version: BTreeSet<pep508_rs::PackageName>,
    only_a_version: BTreeSet<pep508_rs::PackageName>,
}

impl Approvals {
    fn of(config: &pnpm_config::Config) -> Self {
        let mut approvals = Self { any_version: BTreeSet::new(), only_a_version: BTreeSet::new() };
        for (spec, allowed) in &config.allow_builds {
            if !allowed {
                continue;
            }
            let Some(spec) = spec.strip_prefix(PYPI_PURL) else { continue };
            let (name, version) = spec
                .rsplit_once('@')
                .map_or((spec, None), |(name, version)| (name, Some(version)));
            // The key is read as a distribution name, so it names the
            // same one however it is spelled.
            if let Ok(name) = name.parse::<pep508_rs::PackageName>() {
                if version.is_some() {
                    approvals.only_a_version.insert(name);
                } else {
                    approvals.any_version.insert(name);
                }
            }
        }
        approvals
    }

    fn describe(&self, name: &pep508_rs::PackageName) -> String {
        if self.only_a_version.contains(name) {
            return format!(
                "{PYPI_PURL}{name} (approved only for a version, which a Python build is not \
                 checked against)",
            );
        }
        format!("{PYPI_PURL}{name}")
    }
}
