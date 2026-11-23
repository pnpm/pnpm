//! Deprecations. A summary line of a deprecated package carries a marker,
//! and the checks footer counts every deprecated package.

use super::{
    DeprecationLog,
    PackageDiff,
    ReporterState,
};

impl ReporterState {
    pub(super) fn on_deprecation(&mut self, log: &DeprecationLog) {
        self.notices.deprecation_index
            .entry(log.pkg_name.clone())
            .or_default()
            .entry(log.pkg_version.clone())
            .or_insert(self.notices.deprecations.len());
        self.notices.deprecations.push(log.clone());
        self.record_deprecated(pkg_label(log));
    }

    /// The index of the deprecation of the package a summary line names.
    pub(super) fn deprecation_of(&self, diff: &PackageDiff) -> Option<usize> {
        let version = diff.version.as_deref()?;
        let name = diff.real_name.as_deref().unwrap_or(&diff.name);
        self.notices.deprecation_index
            .get(name)?
            .get(version)
            .copied()
    }

    /// What a summary line of a deprecated package ends with.
    pub(super) fn deprecation_marker(&self, diff: &PackageDiff) -> String {
        self.deprecation_of(diff)
            .map(|index| {
                format!(
                    " {}{}",
                    self.rendering.colors.red("deprecated"),
                    alternative_hint(&self.notices.deprecations[index]),
                )
            })
            .unwrap_or_default()
    }
}

/// `name@version` as the checks footer prints it.
///
/// Both halves come from the resolved manifest, which a git or tarball
/// dependency writes itself, so neither is a validated npm package name.
fn pkg_label(log: &DeprecationLog) -> String {
    pnpm_text_sanitize::sanitize_inline(&format!("{}@{}", log.pkg_name, log.pkg_version))
        .into_owned()
}

/// What to move to, when the resolver found a version that is not deprecated.
///
/// The version is pnpm's own reading of the packument rather than anything the
/// publisher wrote, so unlike the notice it is safe to print. Empty when every
/// published version is deprecated, or when the resolution came from the
/// lockfile and no packument was fetched.
fn alternative_hint(log: &DeprecationLog) -> String {
    let Some(alternative) = log.non_deprecated_alternative.as_ref() else {
        return String::new();
    };
    let version = pnpm_text_sanitize::sanitize_inline(&alternative.version);
    if alternative.outside_declared_range {
        format!(". {version} is not deprecated, outside the range you declared.")
    } else {
        format!(". {version} is not deprecated.")
    }
}
