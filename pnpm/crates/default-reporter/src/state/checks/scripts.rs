//! The checks footer lines about the lifecycle scripts a command ran.

use super::{
    super::{
        Output,
        SideEffectsCacheHitLog,
    },
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
    /// The project scripts running now, labeled like `projects`.
    running_projects: BTreeSet<String>,
    /// The dependencies whose scripts are running now, labeled like
    /// `built`.
    building: BTreeSet<String>,
    /// The frame of the spinner the lines of running scripts show.
    spinner: usize,
}

/// The frames of the spinner, one per [`ReporterState::tick_scripts`].
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The texts of one kind of script for [`ReporterState::script_line`].
struct ScriptWords {
    /// The noun for one and for several scripts.
    noun: (&'static str, &'static str),
    /// The verb of finished scripts, for one and for several.
    finished: (&'static str, &'static str),
    /// The verb of running scripts, for one and for several.
    running: (&'static str, &'static str),
}

const PROJECT_SCRIPTS: ScriptWords = ScriptWords {
    noun: ("project script", "project scripts"),
    finished: ("ran", "ran"),
    running: ("is running", "are running"),
};

const DEPENDENCY_BUILDS: ScriptWords = ScriptWords {
    noun: ("dependency", "dependencies"),
    finished: ("was built", "were built"),
    running: ("is building", "are building"),
};

impl ReporterState {
    pub(in crate::state) fn record_project_script(&mut self, label: &str) {
        self.notices.checks.scripts.projects.insert(sanitize(label));
    }

    pub(in crate::state) fn record_dependency_built(&mut self, label: &str) {
        self.notices.checks.scripts.built.insert(sanitize(label));
    }

    /// `dependency` tells a dependency build from a project script.
    pub(in crate::state) fn record_script_started(&mut self, label: &str, dependency: bool) {
        let scripts = &mut self.notices.checks.scripts;
        let running =
            if dependency { &mut scripts.building } else { &mut scripts.running_projects };
        running.insert(sanitize(label));
    }

    pub(in crate::state) fn record_script_finished(&mut self, label: &str, dependency: bool) {
        let scripts = &mut self.notices.checks.scripts;
        let running =
            if dependency { &mut scripts.building } else { &mut scripts.running_projects };
        running.remove(&sanitize(label));
    }

    /// Whether a script runs that the live checks footer shows with a
    /// spinner, which [`Self::tick_scripts`] then animates.
    #[must_use]
    pub fn scripts_running(&self) -> bool {
        let scripts = &self.notices.checks.scripts;
        self.options.lifecycle.summarize_scripts
            && !self.options.append_only
            && !self.notices.checks.rendered
            && (!scripts.running_projects.is_empty() || !scripts.building.is_empty())
    }

    /// Advances the spinner of the running scripts and redraws the live
    /// checks footer. Called on a timer while [`Self::scripts_running`].
    pub fn tick_scripts(&mut self) -> Output {
        if !self.scripts_running() {
            return Output::None;
        }
        self.notices.checks.scripts.spinner += 1;
        self.redraw_live_checks();
        self.display.finish(false)
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
        let projects = self.script_line(
            &scripts.projects,
            &scripts.running_projects,
            &PROJECT_SCRIPTS,
            str::to_string,
        );
        let built =
            self.script_line(&scripts.built, &scripts.building, &DEPENDENCY_BUILDS, |label| {
                self.package_label(label)
            });
        let cached = (!scripts.cached.is_empty()).then(|| {
            let (noun, verb) = count_words(
                &scripts.cached,
                ("dependency build", "came from the side-effects cache"),
                ("dependency builds", "came from the side-effects cache"),
            );
            format!(
                "{} {} {noun} {verb}: {}",
                self.rendering.colors.green("✓"),
                scripts.cached.len(),
                list_names(&scripts.cached, |label| self.package_label(label)),
            )
        });
        [projects, built, cached]
            .into_iter()
            .flatten()
            .collect()
    }

    /// The line of one kind of script: a spinner with the running ones
    /// while any runs, then a `✓` with the finished ones.
    fn script_line(
        &self,
        finished: &BTreeSet<String>,
        running: &BTreeSet<String>,
        words: &ScriptWords,
        paint: impl Fn(&str) -> String,
    ) -> Option<String> {
        let count = |names: &BTreeSet<String>, (one, many): (&'static str, &'static str)| {
            format!("{} {}", names.len(), if names.len() == 1 { one } else { many })
        };
        if running.is_empty() {
            return (!finished.is_empty()).then(|| {
                format!(
                    "{} {} {}: {}",
                    self.rendering.colors.green("✓"),
                    count(finished, words.noun),
                    pick(finished, words.finished),
                    list_names(finished, &paint),
                )
            });
        }
        let spinner = SPINNER[self.notices.checks.scripts.spinner % SPINNER.len()];
        let done = if finished.is_empty() {
            String::new()
        } else {
            format!("{} {}, ", count(finished, words.noun), pick(finished, words.finished))
        };
        let running_count = if finished.is_empty() {
            count(running, words.noun)
        } else {
            running.len().to_string()
        };
        Some(format!(
            "{} {done}{running_count} {}: {}",
            self.rendering.colors.green(spinner),
            pick(running, words.running),
            list_names(running, &paint),
        ))
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

/// `one` for a single name, `many` otherwise.
fn pick(names: &BTreeSet<String>, (one, many): (&'static str, &'static str)) -> &'static str {
    if names.len() == 1 { one } else { many }
}
