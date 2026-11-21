//! Deprecation warnings. They are held until the install summary renders:
//! a summary line of a deprecated package carries the warning, and the
//! deprecated packages no line names follow the summary's sections, named
//! when this install added them and counted when it did not. What no summary
//! carries is printed when the command ends.

use super::{
    BTreeMap,
    BTreeSet,
    BlockPlacement,
    DeprecationLog,
    HashSet,
    Output,
    PackageDiff,
    ReporterState,
    normalized_prefix,
};

impl ReporterState {
    pub(super) fn on_deprecation(&mut self, log: &DeprecationLog) {
        self.notices.deprecation_index
            .entry(log.pkg_name.clone())
            .or_default()
            .entry(log.pkg_version.clone())
            .or_insert(self.notices.deprecations.len());
        self.notices.deprecations.push(log.clone());
    }

    /// The index of the held warning for the package a summary line names.
    pub(super) fn deprecation_of(&self, diff: &PackageDiff) -> Option<usize> {
        let version = diff.version.as_deref()?;
        let name = diff.real_name.as_deref().unwrap_or(&diff.name);
        self.notices.deprecation_index
            .get(name)?
            .get(version)
            .copied()
    }

    /// The `name@version` of the deprecated package a summary line names.
    pub(super) fn deprecated_label_of(&self, diff: &PackageDiff) -> Option<String> {
        self.deprecation_of(diff)
            .map(|index| pkg_label(&self.notices.deprecations[index]))
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

    /// The warnings for the held deprecations of packages that no summary
    /// line in `shown` (by `name@version`) carries. Packages this install
    /// added get one line per direct dependency, then one listing the
    /// subdependencies. Packages the previous install already had are only
    /// counted.
    pub(super) fn deprecation_warnings(&self, shown: &HashSet<String>) -> Vec<String> {
        let mut direct: BTreeMap<String, (&DeprecationLog, Vec<String>)> = BTreeMap::new();
        let mut subdependencies: BTreeSet<String> = BTreeSet::new();
        let mut still_installed: BTreeSet<String> = BTreeSet::new();
        for log in &self.notices.deprecations {
            let label = pkg_label(log);
            if shown.contains(&label) {
                continue;
            }
            if log.already_installed {
                still_installed.insert(label);
            } else if log.depth != 0 {
                subdependencies.insert(label);
            } else {
                direct
                    .entry(label)
                    .or_insert_with(|| (log, Vec::new()))
                    .1
                    .push(normalized_prefix(&self.rendering.cwd, &log.prefix));
            }
        }
        let mut warnings: Vec<String> = direct
            .into_iter()
            .map(|(label, (log, projects))| self.direct_deprecation_warning(&label, log, &projects))
            .collect();
        if !subdependencies.is_empty() {
            warnings.push(self.subdependencies_warning(&subdependencies));
        }
        if !still_installed.is_empty() {
            warnings.push(self.still_installed_warning(still_installed.len()));
        }
        warnings
    }

    fn direct_deprecation_warning(
        &self,
        label: &str,
        log: &DeprecationLog,
        projects: &[String],
    ) -> String {
        let projects = self
            .project_names(
                projects
                    .iter()
                    .map(String::as_str)
                    .collect(),
            )
            .map(|names| format!(" ({names})"))
            .unwrap_or_default();
        format!(
            "{} {} {label}{projects}{}",
            self.rendering.colors.warn_label(),
            self.rendering.colors.red("deprecated:"),
            alternative_hint(log),
        )
    }

    fn subdependencies_warning(&self, labels: &BTreeSet<String>) -> String {
        let found = format!("{} deprecated subdependencies found:", labels.len());
        let labels: Vec<&str> = labels
            .iter()
            .map(String::as_str)
            .collect();
        format!(
            "{} {} {}",
            self.rendering.colors.warn_label(),
            self.rendering.colors.red(&found),
            labels.join(", "),
        )
    }

    fn still_installed_warning(&self, count: usize) -> String {
        let message = if count == 1 {
            "1 deprecated package is still installed".to_string()
        } else {
            format!("{count} deprecated packages are still installed")
        };
        format!("{} {}", self.rendering.colors.warn_label(), self.rendering.colors.red(&message))
    }

    pub(super) fn clear_deprecations(&mut self) {
        self.notices.deprecations.clear();
        self.notices.deprecation_index.clear();
    }

    /// Print the held warnings no summary carried.
    pub(super) fn flush_deprecations(&mut self) {
        let warnings = self.deprecation_warnings(&HashSet::new());
        self.clear_deprecations();
        if warnings.is_empty() {
            return;
        }
        let mut slot = std::mem::take(&mut self.notices.deprecated_slot);
        self.display.frame.emit(&mut slot, warnings.join("\n"), BlockPlacement::Scrolling);
        self.notices.deprecated_slot = slot;
    }

    /// Print the held deprecation warnings when the command ends without a
    /// summary or a `Done in ...` footer to carry them, such as on a failure.
    pub fn flush_pending_deprecations(&mut self) -> Output {
        self.flush_deprecations();
        self.display.finish(self.options.append_only)
    }
}

/// `name@version` as a deprecation warning prints it.
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
