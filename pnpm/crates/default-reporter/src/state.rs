//! Event-folding renderer: the in-process equivalent of
//! `@pnpm/cli.default-reporter`'s `RxJS` graph. Each [`LogEvent`] is folded into
//! [`ReporterState`], which recomputes the terminal frame. The frame model
//! pins fixed blocks below scrolling non-fixed blocks, with one rendering
//! path per log channel.

pub use options::{
    LifecycleOptions,
    ProgressOptions,
    ReporterOptions,
    ScopeOptions,
};

mod options;

use std::{
    collections::HashMap,
    path::{
        Component,
        Path,
        PathBuf,
    },
};

use chrono::{
    DateTime,
    Utc,
};
use pnpm_reporter::{
    AddedRoot,
    ContextLog,
    DedupeCheckLog,
    DependencyType,
    DeprecationLog,
    ExecutionTimeLog,
    FetchingProgressMessage,
    HookLog,
    IgnoredScriptsLog,
    InstallingConfigDepsLog,
    InstallingConfigDepsStatus,
    LifecycleMessage,
    LifecycleStdio,
    LockfileVerificationMessage,
    LogEvent,
    LogLevel,
    PackageImportMethod,
    PackageManifestMessage,
    ProgressMessage,
    RemovedRoot,
    RequestRetryLog,
    ScopeLog,
    SkippedOptionalDependencyLog,
    SkippedOptionalPackage,
    Stage,
    StatsMessage,
    UpdateCheckLog,
};
use serde_json::Value;

use pnpm_config::standalone_install_command;
use pnpm_matcher::{
    Matcher,
    create_matcher,
};

use crate::{
    MaxLogLevel,
    SummaryScope,
    colors::Colors,
    format::{
        contains_path,
        cut_line,
        format_prefix,
        format_prefix_no_trim,
        highlight_last_folder,
        normalize,
        pretty_bytes,
        pretty_ms,
        pretty_ms_compact,
        relative,
        visible_width,
        zoom_out,
    },
};

/// What [`ReporterState::handle`] asks the sink to do after folding one event.
pub enum Output {
    /// Nothing changed; the sink writes nothing.
    None,
    /// The full recomputed frame (in-place mode).
    Frame(String),
    /// Lines to append verbatim (append-only mode).
    Lines(Vec<String>),
}

#[derive(Debug, Default, Clone, Copy)]
struct ProgressStats {
    resolved: u64,
    reused: u64,
    fetched: u64,
    imported: u64,
}

#[derive(Debug, Default)]
struct ProgressEntry {
    stats: ProgressStats,
    slot: BlockSlot,
}

/// One dependency added or removed, ready to render.
#[derive(Debug, Clone)]
struct PackageDiff {
    added: bool,
    from: Option<String>,
    name: String,
    real_name: Option<String>,
    version: Option<String>,
    latest: Option<String>,
}

#[derive(Debug, Default)]
struct ManifestDiff {
    initial: Option<Value>,
    updated: Option<Value>,
}

/// The five dependency buckets, in summary render order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepKind {
    Prod,
    Optional,
    Peer,
    Dev,
    NodeModulesOnly,
}

const SUMMARY_ORDER: [DepKind; 5] =
    [DepKind::Prod, DepKind::Optional, DepKind::Peer, DepKind::Dev, DepKind::NodeModulesOnly];

impl DepKind {
    fn header(self) -> &'static str {
        match self {
            DepKind::Prod => "dependencies",
            DepKind::Optional => "optionalDependencies",
            DepKind::Peer => "peerDependencies",
            DepKind::Dev => "devDependencies",
            DepKind::NodeModulesOnly => "node_modules",
        }
    }

    fn from_dependency_type(dt: Option<DependencyType>) -> Self {
        match dt {
            Some(DependencyType::Prod) => DepKind::Prod,
            Some(DependencyType::Dev) => DepKind::Dev,
            Some(DependencyType::Optional) => DepKind::Optional,
            None => DepKind::NodeModulesOnly,
        }
    }
}

#[derive(Debug, Default)]
struct LifecycleEntry {
    collapsed: bool,
    label: Option<String>,
    output: Vec<String>,
    script: String,
    status: String,
    start: Option<std::time::Instant>,
}

#[derive(Debug)]
struct BigTarball {
    size: u64,
    slot: BlockSlot,
}

/// The whole renderer state. One instance lives behind the sink's mutex in
/// production; tests construct it directly.
pub struct ReporterState {
    options: ReporterOptions,
    rendering: RenderingContext,
    display: DisplayState,
    install: InstallProgress,
    summary: SummaryState,
    scripts: LifecycleState,
    notices: NoticeState,
    downloads: DownloadState,
}

struct RenderingContext {
    cwd: String,
    width: usize,
    colors: Colors,
    /// Compiled [`ReporterOptions::hide_linked_pkgs_diff`]. Never matches
    /// when no patterns were configured.
    hidden_linked_pkgs: Matcher,
}

#[derive(Default)]
struct DisplayState {
    frame: Frame,
    last_frame: Option<String>,
    scope_slot: BlockSlot,
    config_deps_slot: BlockSlot,
    lockfile_verification_slot: BlockSlot,
    pending_lockfile_message: Option<String>,
    exec_slot: BlockSlot,
}

#[derive(Default)]
struct InstallProgress {
    context: Option<ContextLog>,
    import_method: Option<PackageImportMethod>,
    context_slot: BlockSlot,
    context_rendered: bool,
    stats_added: Option<u64>,
    stats_removed: Option<u64>,
    stats_slot: BlockSlot,
}

struct SummaryState {
    diff: HashMap<&'static str, HashMap<String, PackageDiff>>,
    manifest_diffs: HashMap<String, ManifestDiff>,
    slot: BlockSlot,
    seen: bool,
    rendered: bool,
}

#[derive(Default)]
struct LifecycleState {
    entries: HashMap<String, LifecycleEntry>,
    /// Events withheld under [`LifecycleOptions::aggregate_output`], keyed
    /// the same way [`Self::entries`] is.
    buffers: HashMap<String, Vec<LifecycleMessage>>,
    slots: HashMap<String, BlockSlot>,
    colors: HashMap<String, usize>,
    color_wheel: usize,
}

#[derive(Default)]
struct NoticeState {
    warnings: usize,
    collapsed_slot: BlockSlot,
    deprecated_subdeps: Vec<DeprecationLog>,
    deprecated_slot: BlockSlot,
    reported_peer_issues: bool,
}

#[derive(Default)]
struct DownloadState {
    progress: HashMap<String, ProgressEntry>,
    tarballs: HashMap<String, BigTarball>,
}

const MAX_SHOWN_WARNINGS: usize = 5;

/// Lifecycle-script prefix color wheel.
const COLOR_WHEEL: [fn(&Colors, &str) -> String; 6] = [
    |colors, text| colors.cyan(text),
    |colors, text| colors.magenta_bright(text),
    // chalk's `blue` has no dedicated helper here; bright_cyan is the closest
    // already-mapped tone and keeps the wheel visually distinct.
    |colors, text| colors.cyan_bright(text),
    |colors, text| colors.yellow(text),
    |colors, text| colors.green(text),
    |colors, text| colors.red(text),
];

impl ReporterState {
    #[must_use]
    pub fn new(cwd: String, width: usize, colors: Colors, append_only: bool) -> Self {
        Self::new_with_options(
            cwd,
            width,
            colors,
            ReporterOptions { append_only, ..ReporterOptions::default() },
        )
    }

    #[must_use]
    pub fn new_with_summary_scope(
        cwd: String,
        width: usize,
        colors: Colors,
        append_only: bool,
        summary_scope: SummaryScope,
    ) -> Self {
        Self::new_with_options(
            cwd,
            width,
            colors,
            ReporterOptions {
                append_only,
                scope: crate::state::ScopeOptions { summary: summary_scope, ..Default::default() },
                ..ReporterOptions::default()
            },
        )
    }

    #[must_use]
    pub fn new_with_options(
        cwd: String,
        width: usize,
        colors: Colors,
        options: ReporterOptions,
    ) -> Self {
        ReporterState {
            rendering: RenderingContext {
                cwd,
                width,
                colors,
                hidden_linked_pkgs: create_matcher(&options.hide_linked_pkgs_diff),
            },
            display: DisplayState { frame: Frame::new(options.append_only), ..Default::default() },
            install: InstallProgress::default(),
            summary: SummaryState::default(),
            scripts: LifecycleState::default(),
            notices: NoticeState::default(),
            downloads: DownloadState::default(),
            options,
        }
    }

    pub fn handle(&mut self, event: &LogEvent) -> Output {
        if !self.level_permits(event) {
            return Output::None;
        }
        if matches!(event, LogEvent::Summary(_) | LogEvent::ExecutionTime(_)) {
            self.flush_pending_lockfile_message();
        }
        self.handle_event(event);
        if matches!(
            event,
            LogEvent::LockfileVerification(log)
                if matches!(
                    &log.message,
                    LockfileVerificationMessage::Cached { .. }
                        | LockfileVerificationMessage::Done { .. }
                        | LockfileVerificationMessage::Failed { .. }
                ),
        ) {
            self.flush_pending_lockfile_message();
        }
        self.display.finish(self.options.append_only)
    }

    fn handle_event(&mut self, event: &LogEvent) {
        match event {
            LogEvent::Context(log) => self.on_context(log),
            // Prompt lifetime is handled by `Sink` before state folding.
            LogEvent::Prompt(_) => {}
            LogEvent::PackageImportMethod(log) => {
                self.install.import_method = Some(log.method);
                self.install.maybe_render_context(&self.rendering.cwd, &mut self.display.frame);
            }
            LogEvent::Progress(log) => self.on_progress(&log.message),
            LogEvent::Stage(log) => self.on_stage(&log.prefix, log.stage),
            LogEvent::Scope(log) => self.on_scope(log),
            LogEvent::FetchingProgress(log) => self.on_fetching(&log.message),
            LogEvent::Stats(log) => self.on_stats(&log.message),
            LogEvent::Root(log) => self.on_root(&log.message),
            LogEvent::PackageManifest(log) => self.on_manifest(&log.message),
            LogEvent::Summary(log) => self.on_summary(&log.prefix),
            LogEvent::Lifecycle(log) => self.on_lifecycle(&log.message),
            LogEvent::IgnoredScripts(log) => self.on_ignored_scripts(log),
            LogEvent::UpdateCheck(log) => self.on_update_check(log),
            LogEvent::SkippedOptionalDependency(log) => self.on_skipped_optional(log),
            LogEvent::InstallingConfigDeps(log) => self.on_config_deps(log),
            LogEvent::LockfileVerification(log) => self.on_lockfile_verification(&log.message),
            LogEvent::Lockfile(log) => self.on_pnpm(log.level, &log.message, &log.prefix),
            LogEvent::RequestRetry(log) => self.on_request_retry(log),
            LogEvent::Pnpm(log) => self.on_pnpm(log.level, &log.message, &log.prefix),
            LogEvent::DedupeCheck(log) => self.on_dedupe_check(log),
            // `pnpm:global` shares the "other" log stream with the `pnpm`
            // channel but carries no prefix, so it always renders (the
            // empty-prefix path in `on_pnpm`).
            LogEvent::Global(log) => self.on_pnpm(log.level, &log.message, ""),
            LogEvent::ExecutionTime(log) => self.on_execution_time(log),
            LogEvent::Hook(log) => self.on_hook(log),
            LogEvent::Deprecation(log) => self.on_deprecation(log),
            LogEvent::PeerDependencyIssues(_) => self.on_peer_dependency_issues(),
            // Debug-only / non-rendered channels in pnpm's default reporter.
            LogEvent::BrokenModules(_) => {}
        }
    }

    /// Which events render at the configured `--loglevel`, mirroring the
    /// tiers in `@pnpm/cli.default-reporter`'s `reporterForClient`: the
    /// request-retry and deprecation streams need `warn`, the visual
    /// streams (progress, stats, lifecycle, summary, `Done in ...`) need
    /// `info`, and the `pnpm` / `pnpm:global` misc streams filter per
    /// message level in [`Self::on_pnpm`], so errors always pass.
    /// Dedupe-check issues always pass too — upstream reports them as an
    /// error-level log (`ERR_PNPM_DEDUPE_CHECK_ISSUES` in
    /// `reportError.ts`).
    fn level_permits(&self, event: &LogEvent) -> bool {
        match event {
            LogEvent::Lockfile(_)
            | LogEvent::Pnpm(_)
            | LogEvent::Global(_)
            | LogEvent::DedupeCheck(_) => true,
            LogEvent::RequestRetry(_)
            | LogEvent::Deprecation(_)
            | LogEvent::PeerDependencyIssues(_) => self.options.max_log_level >= MaxLogLevel::Warn,
            _ => self.options.max_log_level >= MaxLogLevel::Info,
        }
    }
}

fn lifecycle_ids(message: &LifecycleMessage) -> (&str, &str, &str) {
    match message {
        LifecycleMessage::Script { stage, dep_path, wd, .. }
        | LifecycleMessage::Stdio { stage, dep_path, wd, .. }
        | LifecycleMessage::Exit { stage, dep_path, wd, .. } => (stage, dep_path, wd),
    }
}

fn entries_label(entries: u64) -> String {
    if entries == 1 { "1 entry".to_string() } else { format!("{entries} entries") }
}

/// How a cache-satisfied verification verdict is dated: relative to `now`
/// when the record carries a parseable timestamp, timeless otherwise. The
/// age is clamped at zero so a clock that moved backwards between the
/// verification run and this install cannot render a negative age.
fn cached_verdict(verified_at: Option<&str>, now: DateTime<Utc>) -> String {
    let elapsed_ms = verified_at
        .and_then(|verified_at| DateTime::parse_from_rfc3339(verified_at).ok())
        .map(|verified_at| (now - verified_at.with_timezone(&Utc)).num_milliseconds().max(0));
    match elapsed_ms {
        Some(elapsed_ms) => {
            format!("verified {} ago", pretty_ms_compact(elapsed_ms.unsigned_abs().into()))
        }
        None => "previously verified".to_string(),
    }
}

#[cfg(test)]
mod tests;

mod progress;

mod summary;

mod manifest_diff;
use manifest_diff::{
    added_diff,
    diff_key,
    manifest_dep_versions,
    record_missing,
    remove_optional_from_prod,
    removed_diff,
};

mod lifecycle;

mod notices;

mod update_check;
use update_check::{
    detect_install_source,
    is_strictly_newer,
    update_command,
};

mod frame;
use frame::{
    BlockSlot,
    Frame,
};

mod paths;
use paths::normalized_prefix;

impl DisplayState {
    fn finish(&mut self, append_only: bool) -> Output {
        if append_only {
            let lines = std::mem::take(&mut self.frame.pending);
            if lines.is_empty() { Output::None } else { Output::Lines(lines) }
        } else {
            let frame = self.frame.render();
            if self.last_frame.as_deref() == Some(frame.as_str()) {
                Output::None
            } else {
                self.last_frame = Some(frame.clone());
                Output::Frame(frame)
            }
        }
    }
}

impl Frame {
    pub(super) fn push_block(&mut self, message: String) {
        let mut slot = BlockSlot::default();
        self.emit(&mut slot, message, false);
    }
}

impl Default for SummaryState {
    fn default() -> Self {
        Self {
            diff: SUMMARY_ORDER
                .into_iter()
                .map(|kind| (diff_key(kind), HashMap::new()))
                .collect(),
            manifest_diffs: HashMap::new(),
            slot: BlockSlot::default(),
            seen: false,
            rendered: false,
        }
    }
}
