//! The checks footer an install ends with, above `Done in ...`:
//!
//! - a `✓` line when the lockfile passed the supply-chain policies,
//! - `✓` lines for the lifecycle scripts an install ran or restored from
//!   the side-effects cache,
//! - one `✗` line per problem,
//! - `·` lines for what the user may act on: the dependencies with newer
//!   versions, and a skipped resolution.
//!
//! Every line is a present-tense statement that starts with its subject,
//! with no trailing period. A command to learn more follows in
//! `(see "...")`.
//!
//! The deprecation, peer, engine, and platform checks print nothing when
//! they pass, as they did before the footer existed.

pub(super) use scripts::{
    RunningScript,
    ScriptKind,
};

use super::{
    BTreeMap,
    BTreeSet,
    BlockPlacement,
    BlockSlot,
    DeprecationLog,
    InstallCheckLog,
    LifecycleMessage,
    LogEvent,
    MaxLogLevel,
    Output,
    PeerDependencyIssuesLog,
    ReporterState,
    UnsupportedPackage,
    Value,
    WantedPlatform,
    engine_ranges::merge_ranges,
    is_strictly_newer,
};

mod scripts;

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
    /// The peers with issues, by name. `Some` once peer issues were
    /// reported, even when the payload names none.
    peer_issues: Option<BTreeSet<String>>,
    /// `name@version` of every deprecated package resolution reported.
    deprecated: BTreeSet<String>,
    unsupported: UnsupportedPackages,
    scripts: scripts::ScriptsRun,
    slot: BlockSlot,
    rendered: bool,
}

/// The packages that do not support the host.
#[derive(Default)]
struct UnsupportedPackages {
    /// The packages that reject an engine, by the engine and its current
    /// version, then by the range they declare.
    engine: BTreeMap<(&'static str, String), BTreeMap<String, BTreeSet<String>>>,
    /// The packages that reject the platform, by what they want, such as
    /// `os darwin`.
    platform: BTreeMap<String, BTreeSet<String>>,
}

impl ReporterState {
    pub(super) fn record_resolution_skipped(&mut self) {
        self.notices.checks.resolution_skipped = true;
    }

    pub(super) fn record_supply_chain_passed(&mut self) {
        self.notices.checks.supply_chain_passed = true;
    }

    pub(super) fn on_peer_dependency_issues(&mut self, log: &PeerDependencyIssuesLog) {
        self.notices.checks.peer_issues
            .get_or_insert_default()
            .extend(peers_with_issues(&log.issues_by_projects));
    }

    pub(super) fn on_deprecation(&mut self, log: &DeprecationLog) {
        self.notices.checks.deprecated.insert(pkg_label(log));
    }

    pub(super) fn on_install_check(&mut self, log: &InstallCheckLog) {
        // The package comes from the lockfile, which a contributor can edit.
        let sanitize = |text: &str| pnpm_text_sanitize::sanitize_inline(text).into_owned();
        let Some(unsupported) = &log.unsupported else {
            return self.on_pnpm(log.level, &sanitize(&log.message), &log.prefix);
        };
        if self.options.max_log_level < MaxLogLevel::Warn {
            return;
        }
        let checks = &mut self.notices.checks;
        match unsupported {
            UnsupportedPackage::Platform { pkg_id, wanted } => {
                checks.unsupported.platform
                    .entry(sanitize(&platform_requirement(wanted)))
                    .or_default()
                    .insert(sanitize(pkg_id));
            }
            UnsupportedPackage::Engine { pkg_id, wanted, current } => {
                let engines = [
                    ("Node.js", &wanted.node, &current.node),
                    ("pnpm", &wanted.pnpm, &current.pnpm),
                ];
                for (engine, range, version) in engines {
                    let Some(range) = range else { continue };
                    checks.unsupported.engine
                        .entry((engine, sanitize(version.as_deref().unwrap_or_default())))
                        .or_default()
                        .entry(sanitize(range))
                        .or_default()
                        .insert(sanitize(pkg_id));
                }
            }
        }
    }

    /// Print the footer when the command ends, once. A command that ends
    /// without a `Done in ...` line to carry it flushes it here.
    pub fn flush_pending_checks(&mut self, outcome: CommandOutcome) -> Output {
        self.render_checks(outcome);
        self.display.finish(self.options.append_only)
    }

    /// Redraws the live checks footer pinned below the progress while an
    /// install summarizes its scripts. Only events that change the footer
    /// redraw it. The final footer replaces it in [`Self::render_checks`].
    pub(super) fn refresh_live_checks(&mut self, event: &LogEvent) {
        if !self.options.lifecycle.summarize_scripts
            || self.options.append_only
            || self.notices.checks.rendered
            || !changes_checks(event)
        {
            return;
        }
        self.redraw_live_checks();
    }

    fn redraw_live_checks(&mut self) {
        let footer = self.checks_footer(CommandOutcome::Succeeded);
        let mut slot = std::mem::take(&mut self.notices.checks.slot);
        match footer {
            Some(footer) => {
                self.display.frame.emit(&mut slot, format!("\n{footer}"), BlockPlacement::Pinned);
            }
            None => self.display.frame.clear(&mut slot),
        }
        self.notices.checks.slot = slot;
    }

    pub(super) fn render_checks(&mut self, outcome: CommandOutcome) {
        if self.notices.checks.rendered {
            return;
        }
        if self.summary.seen {
            self.render_summary();
        }
        let Some(footer) = self.checks_footer(outcome) else {
            self.notices.checks.rendered = true;
            self.display.frame.clear(&mut self.notices.checks.slot);
            return;
        };
        self.notices.checks.rendered = true;
        let mut slot = std::mem::take(&mut self.notices.checks.slot);
        // A rendered summary already ends with a blank line.
        let gap = if self.summary.rendered { "" } else { "\n" };
        self.display.frame.emit(&mut slot, format!("{gap}{footer}\n"), BlockPlacement::Scrolling);
        self.notices.checks.slot = slot;
    }

    fn checks_footer(&self, outcome: CommandOutcome) -> Option<String> {
        let shows_info = match outcome {
            CommandOutcome::Succeeded => self.options.max_log_level >= MaxLogLevel::Info,
            CommandOutcome::Failed => false,
        };
        let scripts = self
            .scripts_lines()
            .into_iter()
            .filter(|_| shows_info)
            .map(Some);
        let lines: Vec<String> = std::iter::once(self.passed_line().filter(|_| shows_info))
            .chain(scripts)
            .chain([self.deprecated_line()])
            .chain(self.unsupported_lines().map(Some))
            .chain([
                self.peer_issues_line(),
                self.failed_optional_line(),
                self.outdated_line().filter(|_| shows_info),
                self.resolution_skipped_line().filter(|_| shows_info),
            ])
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
        let see = self.audit_hint("pnpm audit deprecations");
        (!deprecated.is_empty()).then(|| {
            format!(
                "{} {} {noun} {verb} deprecated: {}{see}",
                self.rendering.colors.red("✗"),
                deprecated.len(),
                list_names(deprecated.iter(), |label| self.package_label(label)),
            )
        })
    }

    fn unsupported_lines(&self) -> impl Iterator<Item = String> + '_ {
        let checks = &self.notices.checks;
        let engines = checks.unsupported.engine
            .iter()
            .flat_map(|((engine, current), by_range)| {
                merge_ranges(by_range, current)
                    .into_iter()
                    .map(move |(range, packages)| {
                        (format!("{engine} {range}, not {current}"), packages)
                    })
            });
        let platforms = checks.unsupported.platform
            .iter()
            .map(|(requirement, packages)| (requirement.clone(), packages.clone()));
        engines
            .chain(platforms)
            .map(|(requirement, packages)| self.unsupported_line(&requirement, &packages))
    }

    fn unsupported_line(&self, requirement: &str, packages: &BTreeSet<String>) -> String {
        let (noun, verb) =
            if packages.len() == 1 { ("package", "wants") } else { ("packages", "want") };
        format!(
            "{} {} {noun} {verb} {requirement}: {}{}",
            self.rendering.colors.red("✗"),
            packages.len(),
            list_names(packages.iter(), |label| self.package_label(label)),
            self.audit_hint("pnpm audit engines"),
        )
    }

    /// The `see` hint for a `pnpm audit` subcommand, with a leading space.
    /// Empty after a global install: `pnpm audit` reads a project's
    /// lockfile, which a global install has none of.
    fn audit_hint(&self, command: &str) -> String {
        if self.options.scope.is_global { String::new() } else { format!(" {}", self.see(command)) }
    }

    fn peer_issues_line(&self) -> Option<String> {
        let command =
            if self.options.scope.is_global { "pnpm peers check -g" } else { "pnpm peers check" };
        let peers = self.notices.checks.peer_issues.as_ref()?;
        let cross = self.rendering.colors.red("✗");
        let see = self.see(command);
        if peers.is_empty() {
            return Some(format!("{cross} Peer dependencies have issues {see}"));
        }
        let (noun, verb) = if peers.len() == 1 {
            ("peer dependency", "has")
        } else {
            ("peer dependencies", "have")
        };
        Some(format!(
            "{cross} {} {noun} {verb} issues: {} {see}",
            peers.len(),
            list_names(peers.iter(), |name| self.rendering.colors.package_name(name)),
        ))
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
                "{} {} {noun} {verb}: {} {}",
                self.rendering.colors.grey("·"),
                outdated.len(),
                list_names(outdated.iter(), |name| self.rendering.colors.package_name(name)),
                self.see("pnpm outdated"),
            )
        })
    }

    /// The hint that closes a line: the command that tells more.
    fn see(&self, command: &str) -> String {
        format!(r#"(see "{}")"#, self.rendering.colors.command(command))
    }

    /// A `name@version` label with its name highlighted.
    fn package_label(&self, label: &str) -> String {
        match label.rsplit_once('@') {
            Some((name, version)) if !name.is_empty() => {
                format!("{}@{version}", self.rendering.colors.package_name(name))
            }
            _ => self.rendering.colors.package_name(label),
        }
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

/// The names of the peers a `pnpm:peer-dependency-issues` payload reports
/// as bad or missing, in any project. The names come from manifests, so
/// they are sanitized.
fn peers_with_issues(issues_by_projects: &Value) -> impl Iterator<Item = String> + '_ {
    issues_by_projects
        .as_object()
        .into_iter()
        .flat_map(serde_json::Map::values)
        .flat_map(|issues| ["bad", "missing"].map(|kind| issues.get(kind)))
        .flatten()
        .filter_map(Value::as_object)
        .flat_map(serde_json::Map::keys)
        .map(String::as_str)
        .map(pnpm_text_sanitize::sanitize_inline)
        .map(std::borrow::Cow::into_owned)
}

/// `name@version` as the footer prints it.
///
/// Both halves come from the resolved manifest, which a git or tarball
/// dependency writes itself, so neither is a validated npm package name.
fn pkg_label(log: &DeprecationLog) -> String {
    pnpm_text_sanitize::sanitize_inline(&format!("{}@{}", log.pkg_name, log.pkg_version))
        .into_owned()
}

/// The declared `os`, `cpu`, and `libc` lists, such as
/// `os darwin or linux and cpu arm64`.
#[must_use]
pub fn platform_requirement(wanted: &WantedPlatform) -> String {
    let fields: Vec<String> = [("os", &wanted.os), ("cpu", &wanted.cpu), ("libc", &wanted.libc)]
        .into_iter()
        .filter_map(|(field, values)| Some((field, values.as_deref()?)))
        .filter(|(_, values)| !values.is_empty())
        .map(|(field, values)| format!("{field} {}", platform_values(values)))
        .collect();
    if fields.is_empty() {
        return "another platform".to_string();
    }
    fields.join(" and ")
}

/// The values of one platform field. A `!value` entry rejects that value,
/// so a list of them reads `other than a or b`. A list with plain values
/// needs one of them, which already leaves out the rejected ones.
fn platform_values(values: &[String]) -> String {
    let wanted: Vec<&str> = values
        .iter()
        .map(String::as_str)
        .filter(|value| !value.starts_with('!'))
        .collect();
    if !wanted.is_empty() {
        return wanted.join(" or ");
    }
    let rejected: Vec<&str> = values
        .iter()
        .filter_map(|value| value.strip_prefix('!'))
        .collect();
    format!("other than {}", rejected.join(" or "))
}

/// Whether the event can change a line of the checks footer.
fn changes_checks(event: &LogEvent) -> bool {
    matches!(
        event,
        LogEvent::Lifecycle(pnpm_reporter::LifecycleLog {
            message: LifecycleMessage::Script { .. } | LifecycleMessage::Exit { .. },
            ..
        }) | LogEvent::SideEffectsCacheHit(_)
            | LogEvent::SkippedOptionalDependency(_)
            | LogEvent::Deprecation(_)
            | LogEvent::InstallCheck(_)
            | LogEvent::PeerDependencyIssues(_)
            | LogEvent::LockfileVerification(_)
            | LogEvent::Pnpm(_),
    )
}

fn count_words<'a>(
    items: &BTreeSet<String>,
    one: (&'a str, &'a str),
    many: (&'a str, &'a str),
) -> (&'a str, &'a str) {
    if items.len() == 1 { one } else { many }
}

/// Text from a manifest or the lockfile, on one line and without control
/// characters.
fn sanitize(text: &str) -> String {
    pnpm_text_sanitize::sanitize_inline(text).into_owned()
}

/// At most [`MAX_LISTED`] names, each through `paint`, then a count of
/// the rest.
fn list_names<'a>(
    names: impl ExactSizeIterator<Item = &'a String>,
    paint: impl Fn(&str) -> String,
) -> String {
    let total = names.len();
    let listed: Vec<String> = names
        .take(MAX_LISTED)
        .map(|name| paint(name))
        .collect();
    let rest = total - listed.len();
    match listed.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] if rest == 0 => format!("{} and {last}", init.join(", ")),
        _ => format!("{} and {rest} more", listed.join(", ")),
    }
}
