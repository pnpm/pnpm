//! The checks footer an install ends with, above `Done in ...`:
//!
//! - a `✓` line when the lockfile passed the supply-chain policies,
//! - one `✗` line per problem,
//! - `·` lines for what the user may act on: the dependencies with newer
//!   versions, and a skipped resolution.
//!
//! Every line is a present-tense statement that starts with its subject,
//! with no trailing period. A command to learn more follows in
//! `(see "...")`.
//!
//! The deprecation and peer checks print nothing when they pass, as they
//! did before the footer existed.

use super::{
    BTreeSet,
    BlockPlacement,
    BlockSlot,
    DeprecationLog,
    MaxLogLevel,
    Output,
    ReporterState,
    is_strictly_newer,
};

/// How a command ended, which decides the footer lines it prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    Succeeded,
    /// Only problems print: a passing verdict or a newer version is noise
    /// beside the error.
    Failed,
}

/// How many names a footer line lists before it counts the rest.
const MAX_LISTED: usize = 3;

/// The info line pnpm logs when an install reuses the lockfile as is.
pub(super) const RESOLUTION_SKIPPED: &str = "Lockfile is up to date, resolution step is skipped";

#[derive(Default)]
pub(super) struct ChecksState {
    resolution_skipped: bool,
    supply_chain_passed: bool,
    peer_issues: bool,
    /// `name@version` of every deprecated package resolution reported.
    deprecated: BTreeSet<String>,
    slot: BlockSlot,
    rendered: bool,
}

impl ReporterState {
    pub(super) fn record_resolution_skipped(&mut self) {
        self.notices.checks.resolution_skipped = true;
    }

    pub(super) fn record_supply_chain_passed(&mut self) {
        self.notices.checks.supply_chain_passed = true;
    }

    pub(super) fn on_peer_dependency_issues(&mut self) {
        self.notices.checks.peer_issues = true;
    }

    pub(super) fn on_deprecation(&mut self, log: &DeprecationLog) {
        self.notices.checks.deprecated.insert(pkg_label(log));
    }

    /// Print the footer when the command ends, once. A command that ends
    /// without a `Done in ...` line to carry it flushes it here.
    pub fn flush_pending_checks(&mut self, outcome: CommandOutcome) -> Output {
        self.render_checks(outcome);
        self.display.finish(self.options.append_only)
    }

    pub(super) fn render_checks(&mut self, outcome: CommandOutcome) {
        if self.notices.checks.rendered {
            return;
        }
        let Some(footer) = self.checks_footer(outcome) else { return };
        self.notices.checks.rendered = true;
        let mut slot = std::mem::take(&mut self.notices.checks.slot);
        // A rendered summary already ends with a blank line.
        let gap = if self.summary.rendered { "" } else { "\n" };
        self.display.frame.emit(&mut slot, format!("{gap}{footer}\n"), BlockPlacement::Scrolling);
        self.notices.checks.slot = slot;
    }

    fn checks_footer(&self, outcome: CommandOutcome) -> Option<String> {
        let shows_info =
            outcome == CommandOutcome::Succeeded && self.options.max_log_level >= MaxLogLevel::Info;
        let lines: Vec<String> = [
            self.passed_line().filter(|_| shows_info),
            self.deprecated_line(),
            self.peer_issues_line(),
            self.outdated_line().filter(|_| shows_info),
            self.resolution_skipped_line().filter(|_| shows_info),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    fn passed_line(&self) -> Option<String> {
        self.notices.checks.supply_chain_passed.then(|| {
            format!("{} Lockfile passes supply-chain policies", self.rendering.colors.green("✓"))
        })
    }

    fn deprecated_line(&self) -> Option<String> {
        let deprecated = &self.notices.checks.deprecated;
        let (noun, verb) =
            if deprecated.len() == 1 { ("package", "is") } else { ("packages", "are") };
        (!deprecated.is_empty()).then(|| {
            format!(
                "{} {} {noun} {verb} deprecated: {}",
                self.rendering.colors.red("✗"),
                deprecated.len(),
                list_names(deprecated),
            )
        })
    }

    fn peer_issues_line(&self) -> Option<String> {
        let command =
            if self.options.scope.is_global { "pnpm peers check -g" } else { "pnpm peers check" };
        self.notices.checks.peer_issues.then(|| {
            format!(
                r#"{} Peer dependencies have issues (see "{command}")"#,
                self.rendering.colors.red("✗"),
            )
        })
    }

    fn outdated_line(&self) -> Option<String> {
        let outdated = self.outdated_dependencies();
        let (noun, verb) = if outdated.len() == 1 {
            ("dependency", "has a newer version")
        } else {
            ("dependencies", "have newer versions")
        };
        (!outdated.is_empty()).then(|| {
            format!(
                r#"{} {} {noun} {verb}: {} (see "pnpm outdated")"#,
                self.rendering.colors.grey("·"),
                outdated.len(),
                list_names(&outdated),
            )
        })
    }

    fn resolution_skipped_line(&self) -> Option<String> {
        self.notices.checks.resolution_skipped.then(|| {
            format!("{} {RESOLUTION_SKIPPED}", self.rendering.colors.grey("·"))
        })
    }

    /// The dependencies this install added whose registry `latest` is
    /// newer than the installed version. Only a dependency resolution
    /// read the registry metadata of carries a `latest`.
    fn outdated_dependencies(&self) -> BTreeSet<String> {
        let other_projects = self.summary.workspace
            .is_some()
            .then_some(&self.summary.other_projects)
            .into_iter()
            .flat_map(|projects| projects.values());
        std::iter::once(&self.summary.diff)
            .chain(other_projects)
            .flat_map(|buckets| buckets.values())
            .flat_map(|bucket| bucket.values())
            .filter(|diff| diff.added)
            .filter(|diff| !self.is_hidden_linked(diff))
            .filter(|diff| match (&diff.latest, &diff.version) {
                (Some(latest), Some(version)) => is_strictly_newer(latest, version),
                _ => false,
            })
            .map(|diff| pnpm_text_sanitize::sanitize_inline(&diff.name).into_owned())
            .collect()
    }
}

/// `name@version` as the footer prints it.
///
/// Both halves come from the resolved manifest, which a git or tarball
/// dependency writes itself, so neither is a validated npm package name.
fn pkg_label(log: &DeprecationLog) -> String {
    pnpm_text_sanitize::sanitize_inline(&format!("{}@{}", log.pkg_name, log.pkg_version))
        .into_owned()
}

/// At most [`MAX_LISTED`] names, then a count of the rest.
fn list_names(names: &BTreeSet<String>) -> String {
    let listed: Vec<&str> = names
        .iter()
        .take(MAX_LISTED)
        .map(String::as_str)
        .collect();
    let rest = names.len() - listed.len();
    match listed.as_slice() {
        [] => String::new(),
        [only] => (*only).to_string(),
        [init @ .., last] if rest == 0 => format!("{} and {last}", init.join(", ")),
        _ => format!("{} and {rest} more", listed.join(", ")),
    }
}
