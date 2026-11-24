//! The checks footer lines about the lifecycle scripts a command ran.

use super::{
    super::{
        Output,
        SideEffectsCacheHitLog,
    },
    BTreeMap,
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
    projects: Labels,
    /// The project scripts running now, labeled like `projects`.
    running_projects: Labels,
    dependencies: DependencyBuilds,
    /// `name@version` of every dependency whose build output came from
    /// the side-effects cache.
    cached: BTreeSet<String>,
    /// `name@version` of every optional dependency skipped because its
    /// build failed.
    failed_optional: BTreeSet<String>,
    /// The frame of the spinner the lines of running scripts show.
    spinner: usize,
}

/// Display labels with how many entries carry each, kept up to date as
/// entries come and go, so a redraw reads them without collecting.
#[derive(Default)]
struct Labels(BTreeMap<String, usize>);

impl Labels {
    fn add(&mut self, label: String) {
        *self.0.entry(label).or_default() += 1;
    }

    fn remove(&mut self, label: &str) {
        let Some(count) = self.0.get_mut(label) else { return };
        *count -= 1;
        if *count == 0 {
            self.0.remove(label);
        }
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn names(&self) -> impl ExactSizeIterator<Item = &String> {
        self.0.keys()
    }
}

/// The dependency builds of the command. Peer variants share a
/// `name@version` label, and the hoisted linker builds each copy of a
/// package in its own directory, so builds are kept by lockfile key and
/// by directory, with their labels counted for display.
#[derive(Default)]
struct DependencyBuilds {
    /// The label of every dependency whose scripts succeeded, by lockfile
    /// key.
    built: BTreeMap<String, String>,
    built_labels: Labels,
    /// The builds running now, by the directory their scripts run in.
    building: BTreeMap<String, DependencyBuild>,
    building_labels: Labels,
    /// The lockfile key of every directory a build ran in, which is how
    /// the skip event of a project-local build names it.
    keys: BTreeMap<String, String>,
}

struct DependencyBuild {
    label: String,
    key: String,
}

impl DependencyBuilds {
    fn start(&mut self, key: &str, dir: &str, label: String) {
        self.keys.insert(dir.to_string(), key.to_string());
        let build = DependencyBuild { label: label.clone(), key: key.to_string() };
        if let Some(previous) = self.building.insert(dir.to_string(), build) {
            self.building_labels.remove(&previous.label);
        }
        self.building_labels.add(label);
    }

    fn finish(&mut self, dir: &str) {
        if let Some(build) = self.building.remove(dir) {
            self.building_labels.remove(&build.label);
        }
    }

    fn succeed(&mut self, key: &str, label: String) {
        if let Some(previous) = self.built.insert(key.to_string(), label.clone()) {
            self.built_labels.remove(&previous);
        }
        self.built_labels.add(label);
    }

    /// Drops the result and the running copies of the build `id` names:
    /// its lockfile key, or the directory of one of its copies.
    fn retract(&mut self, id: &str) {
        let key = self.keys
            .get(id)
            .map(String::as_str)
            .unwrap_or(id)
            .to_string();
        if let Some(label) = self.built.remove(&key) {
            self.built_labels.remove(&label);
        }
        let building_labels = &mut self.building_labels;
        self.building.retain(|_, build| {
            let retracted = build.key == key;
            if retracted {
                building_labels.remove(&build.label);
            }
            !retracted
        });
    }
}

/// Whether a lifecycle script belongs to a workspace project or builds a
/// dependency.
#[derive(Clone, Copy)]
pub(in crate::state) enum ScriptKind {
    Project,
    Dependency,
}

/// A lifecycle script as the footer tracks it while it runs.
pub(in crate::state) struct RunningScript<'a> {
    pub(in crate::state) kind: ScriptKind,
    /// The lockfile key of a dependency, unused for a project script.
    pub(in crate::state) id: &'a str,
    /// The directory the script runs in.
    pub(in crate::state) dir: &'a str,
    /// `<project> <stage>`, or the dependency's `name@version`.
    pub(in crate::state) label: &'a str,
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
        let projects = &mut self.notices.checks.scripts.projects;
        let label = sanitize(label);
        if !projects.0.contains_key(&label) {
            projects.add(label);
        }
    }

    pub(in crate::state) fn record_dependency_built(&mut self, script: &RunningScript<'_>) {
        self.notices.checks.scripts.dependencies.succeed(script.id, sanitize(script.label));
    }

    pub(in crate::state) fn record_script_started(&mut self, script: &RunningScript<'_>) {
        let scripts = &mut self.notices.checks.scripts;
        match script.kind {
            ScriptKind::Project => scripts.running_projects.add(sanitize(script.label)),
            ScriptKind::Dependency => {
                scripts.dependencies.start(script.id, script.dir, sanitize(script.label));
            }
        }
    }

    pub(in crate::state) fn record_script_finished(&mut self, script: &RunningScript<'_>) {
        let scripts = &mut self.notices.checks.scripts;
        match script.kind {
            ScriptKind::Project => scripts.running_projects.remove(&sanitize(script.label)),
            ScriptKind::Dependency => scripts.dependencies.finish(script.dir),
        }
    }

    /// A skipped package is not built, even when one of its scripts or
    /// copies succeeded before another failed, and none of its scripts runs
    /// on. `id` is its lockfile key or the directory of the failed copy, so
    /// a peer variant that shares its label keeps its own result.
    pub(in crate::state) fn record_failed_optional_build(
        &mut self,
        id: &str,
        name: &str,
        version: &str,
    ) {
        let scripts = &mut self.notices.checks.scripts;
        scripts.dependencies.retract(id);
        scripts.failed_optional.insert(sanitize(&format!("{name}@{version}")));
    }

    /// Whether a script runs that the live checks footer shows with a
    /// spinner, which [`Self::tick_scripts`] then animates.
    #[must_use]
    pub fn scripts_running(&self) -> bool {
        let scripts = &self.notices.checks.scripts;
        self.options.lifecycle.summarize_scripts
            && !self.options.append_only
            && !self.notices.checks.rendered
            && (!scripts.running_projects.is_empty()
                || !scripts.dependencies.building.is_empty())
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
        let dependencies = &scripts.dependencies;
        let built = self.script_line(
            &dependencies.built_labels,
            &dependencies.building_labels,
            &DEPENDENCY_BUILDS,
            |label| self.package_label(label),
        );
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
                list_names(scripts.cached.iter(), |label| self.package_label(label)),
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
        finished: &Labels,
        running: &Labels,
        words: &ScriptWords,
        paint: impl Fn(&str) -> String,
    ) -> Option<String> {
        let count = |names: &Labels, (one, many): (&'static str, &'static str)| {
            format!("{} {}", names.len(), if names.len() == 1 { one } else { many })
        };
        if running.is_empty() {
            return (!finished.is_empty()).then(|| {
                format!(
                    "{} {} {}: {}",
                    self.rendering.colors.green("✓"),
                    count(finished, words.noun),
                    pick(finished, words.finished),
                    list_names(finished.names(), &paint),
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
            list_names(running.names(), &paint),
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
                list_names(failed.iter(), |label| self.package_label(label)),
            )
        })
    }
}

fn pick(names: &Labels, (one, many): (&'static str, &'static str)) -> &'static str {
    if names.len() == 1 { one } else { many }
}
