//! The checks footer lines about the lifecycle scripts a command ran.

use super::{
    super::SideEffectsCacheHitLog,
    BTreeSet,
    ReporterState,
    count_words,
    list_names,
    sanitize,
};

/// The lifecycle scripts a command summarizes, see
/// [`LifecycleOptions::summarize_scripts`](crate::state::LifecycleOptions::summarize_scripts).
#[derive(Default)]
pub(super) struct ScriptsRun {
    /// `<project> <stage>` of every project script that succeeded.
    projects: BTreeSet<String>,
    /// `name@version` of every dependency whose scripts succeeded.
    built: BTreeSet<String>,
    /// `name@version` of every dependency whose build output came from
    /// the side-effects cache.
    cached: BTreeSet<String>,
    /// `name@version` of every optional dependency skipped because its
    /// build failed.
    failed_optional: BTreeSet<String>,
    /// The scripts running now, labeled like `projects` and `built`.
    running: BTreeSet<String>,
}

impl ReporterState {
    pub(in crate::state) fn record_project_script(&mut self, label: &str) {
        self.notices.checks.scripts.projects.insert(sanitize(label));
    }

    pub(in crate::state) fn record_dependency_built(&mut self, label: &str) {
        self.notices.checks.scripts.built.insert(sanitize(label));
    }

    pub(in crate::state) fn record_script_started(&mut self, label: &str) {
        self.notices.checks.scripts.running.insert(sanitize(label));
    }

    pub(in crate::state) fn record_script_finished(&mut self, label: &str) {
        self.notices.checks.scripts.running.remove(&sanitize(label));
    }

    pub(in crate::state) fn record_failed_optional_build(&mut self, name: &str, version: &str) {
        self.notices.checks.scripts.failed_optional.insert(sanitize(&format!("{name}@{version}")));
    }

    pub(in crate::state) fn on_side_effects_cache_hit(&mut self, log: &SideEffectsCacheHitLog) {
        if self.options.lifecycle.summarize_scripts {
            let label = sanitize(&format!("{}@{}", log.pkg_name, log.pkg_version));
            self.notices.checks.scripts.cached.insert(label);
        }
    }

    pub(super) fn scripts_lines(&self) -> Vec<String> {
        let scripts = &self.notices.checks.scripts;
        let tick = self.rendering.colors.green("✓");
        let mut lines = Vec::new();
        if !scripts.projects.is_empty() {
            let (noun, verb) = count_words(
                &scripts.projects,
                ("project script", "ran"),
                ("project scripts", "ran"),
            );
            lines.push(format!(
                "{tick} {} {noun} {verb}: {}",
                scripts.projects.len(),
                list_names(&scripts.projects, str::to_string),
            ));
        }
        for (packages, one, many) in [
            (&scripts.built, ("dependency", "was built"), ("dependencies", "were built")),
            (
                &scripts.cached,
                ("dependency build", "came from the side-effects cache"),
                ("dependency builds", "came from the side-effects cache"),
            ),
        ] {
            if packages.is_empty() {
                continue;
            }
            let (noun, verb) = count_words(packages, one, many);
            lines.push(format!(
                "{tick} {} {noun} {verb}: {}",
                packages.len(),
                list_names(packages, |label| self.package_label(label)),
            ));
        }
        lines
    }

    pub(super) fn running_line(&self) -> Option<String> {
        let running = &self.notices.checks.scripts.running;
        let (noun, verb) =
            count_words(running, ("script", "is running"), ("scripts", "are running"));
        (!running.is_empty()).then(|| {
            format!(
                "{} {} {noun} {verb}: {}",
                self.rendering.colors.grey("·"),
                running.len(),
                list_names(running, str::to_string),
            )
        })
    }

    pub(super) fn failed_optional_line(&self) -> Option<String> {
        let failed = &self.notices.checks.scripts.failed_optional;
        let (noun, verb) = count_words(
            failed,
            ("optional dependency", "failed to build and was skipped"),
            ("optional dependencies", "failed to build and were skipped"),
        );
        (!failed.is_empty()).then(|| {
            format!(
                "{} {} {noun} {verb}: {}",
                self.rendering.colors.red("✗"),
                failed.len(),
                list_names(failed, |label| self.package_label(label)),
            )
        })
    }
}
