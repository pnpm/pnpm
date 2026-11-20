use super::{
    ResolvedOverride,
    VersionsOverrider,
};
use pnpm_config_parse_overrides::VersionOverride;
use pnpm_package_manifest::DependencyGroup;

/// Answers whether an override governs a dependency declared as a given
/// specifier, whether or not it rewrites the text. An override that repeats
/// the declaration verbatim still governs it, and a range-scoped override
/// (`foo@^2`) governs one declaration of `foo` and not another, so the
/// declared specifier is part of the question.
///
/// Built by [`VersionsOverrider::dependency_matcher`] for one manifest.
pub struct OverriddenDependencyMatcher<'a> {
    pub(super) overrider: &'a VersionsOverrider,
    pub(super) applicable_parent_scoped: Vec<&'a ResolvedOverride>,
}

impl OverriddenDependencyMatcher<'_> {
    #[must_use]
    pub fn matches(&self, group: DependencyGroup, dep_name: &str, dep_spec: &str) -> bool {
        self.overrider
            .choose_override(Some(group), &self.applicable_parent_scoped, dep_name, dep_spec)
            .is_some()
            || self.overrider.converge_applies(dep_name, dep_spec)
    }

    /// The override an install applies to `dep_name` declared as `dep_spec`
    /// in `group`: the most specific entry that matches it. A convergence
    /// override is not reported.
    #[must_use]
    pub fn governing_override(
        &self,
        group: DependencyGroup,
        dep_name: &str,
        dep_spec: &str,
    ) -> Option<&VersionOverride> {
        self.overrider
            .choose_override(Some(group), &self.applicable_parent_scoped, dep_name, dep_spec)
            .map(|chosen| &chosen.inner)
    }
}
