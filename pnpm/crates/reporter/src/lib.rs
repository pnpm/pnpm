//! User-facing log channels for pacquet.
//!
//! Pacquet's progress, lifecycle, summary, and similar output is shaped to
//! match pnpm's so that emitted NDJSON is consumable by
//! `@pnpm/cli.default-reporter`. The wire format matches what
//! `@pnpm/core-loggers` defines for each channel.
//!
//! # Adding a channel
//!
//! Only the variants pacquet currently emits live in [`LogEvent`]. New
//! channels are added incrementally as the surrounding code starts using
//! them.

pub use dependencies::{
    AddedRoot, DependencyType, PackageManifestLog, PackageManifestMessage, RemovedRoot, RootLog,
    RootMessage, SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalParent,
    SkippedOptionalReason,
};
pub use progress::{
    ContextLog, FetchingProgressLog, FetchingProgressMessage, PackageImportMethod,
    PackageImportMethodLog, ProgressLog, ProgressMessage, PromptAction, PromptLog, Stage, StageLog,
    StatsLog, StatsMessage, SummaryLog,
};

use serde::Serialize;
use std::{
    io::Write,
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};

/// One log channel from `@pnpm/core-loggers`.
///
/// Variants are added as pacquet starts emitting them. The `name` tag in
/// the serialized JSON identifies the channel; consumers (notably
/// `@pnpm/cli.default-reporter`) dispatch on this value.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "name")]
pub enum LogEvent {
    /// Install context: store directory, virtual-store directory, and
    /// whether a current lockfile (`node_modules/.pnpm/lock.yaml`) was
    /// loaded (`pnpm:context`).
    #[serde(rename = "pnpm:context")]
    Context(ContextLog),

    /// Coarse install-pipeline phase markers (`pnpm:stage`).
    #[serde(rename = "pnpm:stage")]
    Stage(StageLog),

    /// How many workspace projects the command runs over (`pnpm:scope`).
    /// Emitted once per run, before the command's own output. The
    /// default reporter renders it as the `Scope:` line for the commands
    /// pnpm reports scope for, and stays silent when a single project is
    /// selected.
    #[serde(rename = "pnpm:scope")]
    Scope(ScopeLog),

    /// Brackets an interactive terminal prompt (`pnpm:prompt`). The default
    /// reporter holds live redraws between `start` and `end` so they cannot
    /// overwrite the question while it is waiting for input.
    #[serde(rename = "pnpm:prompt")]
    Prompt(PromptLog),

    /// End-of-install marker (`pnpm:summary`). pnpm's reporter combines
    /// this with the accumulated `pnpm:root` events to render the final
    /// "+N -M" block.
    #[serde(rename = "pnpm:summary")]
    Summary(SummaryLog),

    /// The import method used to materialise files from the store
    /// (`pnpm:package-import-method`). Fires the first time each
    /// resolved method (`clone` / `hardlink` / `copy`) actually
    /// succeeds during an install — so for the `auto` and
    /// `clone-or-copy` config values, the wire value reflects the
    /// post-fallback method rather than the optimistic configured
    /// one. Up to three events per install (one per resolved method)
    /// gated by an install-scoped atomic in `pnpm-package-manager`.
    #[serde(rename = "pnpm:package-import-method")]
    PackageImportMethod(PackageImportMethodLog),

    /// Per-package status transitions (`pnpm:progress`). Together they
    /// drive the "X/Y resolved, X/Y fetched, X/Y imported" counters in
    /// the default reporter.
    #[serde(rename = "pnpm:progress")]
    Progress(ProgressLog),

    /// Per-tarball download progress (`pnpm:fetching-progress`). The
    /// `in_progress` events are throttled to ~200ms while the body
    /// streams.
    #[serde(rename = "pnpm:fetching-progress")]
    FetchingProgress(FetchingProgressLog),

    /// Project manifest snapshots (`pnpm:package-manifest`). Two
    /// presence-tagged shapes per pnpm's union: `initial` (emitted
    /// once at install start with the on-disk manifest) and
    /// `updated` (emitted after the manifest is rewritten — e.g.
    /// `pacquet add` saves a new dependency entry).
    #[serde(rename = "pnpm:package-manifest")]
    PackageManifest(PackageManifestLog),

    /// Per-direct-dependency add / remove events (`pnpm:root`). pnpm's
    /// reporter accumulates these and renders the "+N -M" block at
    /// `pnpm:summary` time.
    #[serde(rename = "pnpm:root")]
    Root(RootLog),

    /// Aggregate add / remove counts emitted once per project after
    /// the link phase (`pnpm:stats`). Pnpm emits `added` and
    /// `removed` from separate sites; pacquet currently emits both
    /// together because pruning hasn't landed yet — see
    /// [`StatsMessage::Removed`].
    #[serde(rename = "pnpm:stats")]
    Stats(StatsLog),

    /// One per failed-and-being-retried HTTP request
    /// (`pnpm:request-retry`). Pnpm's default reporter surfaces these
    /// as `Will retry in <ms>. <N> retries left.` warnings; the
    /// `error` payload is what the JS reporter dispatches on
    /// (`httpStatusCode` / `status` / `errno` / `code`) to render the
    /// reason.
    #[serde(rename = "pnpm:request-retry")]
    RequestRetry(RequestRetryLog),

    /// Per-script lifecycle output (`pnpm:lifecycle`). `Script` fires
    /// once before the script spawns, `Stdio` fires per stdout/stderr
    /// line, and `Exit` fires once after the script returns.
    #[serde(rename = "pnpm:lifecycle")]
    Lifecycle(LifecycleLog),

    /// One per install run, listing every package whose lifecycle
    /// scripts were skipped because the package was not in
    /// `allowBuilds` (`pnpm:ignored-scripts`). pnpm's reporter renders
    /// the list to remind the user they can opt in.
    #[serde(rename = "pnpm:ignored-scripts")]
    IgnoredScripts(IgnoredScriptsLog),

    /// The latest pnpm the registry offers, next to the running one
    /// (`pnpm:update-check`). Emitted at most once a day by the
    /// install-family commands the update notifier covers; the default
    /// reporter prints a notice only when the latest version is newer.
    #[serde(rename = "pnpm:update-check")]
    UpdateCheck(UpdateCheckLog),

    /// One per optional-dependency pacquet decided to skip rather
    /// than fail the install over. Reason discriminates the cause —
    /// pacquet currently only emits `build_failure` (from
    /// `BuildModules` when a postinstall fails on an optional dep);
    /// the `unsupported_engine` / `unsupported_platform` /
    /// `resolution_failure` reasons come from earlier phases that
    /// haven't landed in pacquet yet.
    #[serde(rename = "pnpm:skipped-optional-dependency")]
    SkippedOptionalDependency(SkippedOptionalDependencyLog),

    /// Bracketing events for the configurational-dependency install
    /// (`pnpm:installing-config-deps`): a `started` before any
    /// fetch/link work, then a single `done` carrying the installed
    /// `{ name, version }` list. Both are suppressed entirely when an
    /// install finds every config dependency already materialized, so a
    /// no-op install emits nothing on this channel.
    #[serde(rename = "pnpm:installing-config-deps")]
    InstallingConfigDeps(InstallingConfigDepsLog),

    /// One per snapshot whose `<virtual_store_dir>/...` directory
    /// has gone missing on disk even though the current lockfile
    /// records it as installed (`pnpm:_broken_node_modules`). The
    /// frozen-lockfile path emits one of these per missing slot
    /// before falling through to a full re-install of that snapshot.
    #[serde(rename = "pnpm:_broken_node_modules")]
    BrokenModules(BrokenModulesLog),

    /// Lockfile-verification gate progress (`pnpm:lockfile-verification`).
    /// One `started` event before the fan-out, followed by exactly one
    /// terminal `done` (success) or `failed` (policy violation or
    /// unexpected throw). Fires only when the candidate set is
    /// non-empty — a lockfile whose snapshots all fail name/version
    /// extraction produces no events.
    #[serde(rename = "pnpm:lockfile-verification")]
    LockfileVerification(LockfileVerificationLog),

    /// Generic global-logger message (`name: "pnpm"`). Carries a
    /// `{ message, prefix }` payload — for example, the "Lockfile is
    /// up to date, resolution step is skipped" line the frozen-install
    /// short-circuit prints. `@pnpm/cli.default-reporter` routes these
    /// into the "other" log stream.
    #[serde(rename = "pnpm")]
    Pnpm(PnpmLog),

    /// The `ERR_PNPM_DEDUPE_CHECK_ISSUES` error (`name: "pnpm"`).
    ///
    /// This keeps the structured diff on the wire for NDJSON consumers
    /// while carrying the terminal rendering used by the in-process default
    /// reporter.
    #[serde(rename = "pnpm")]
    DedupeCheck(DedupeCheckLog),

    /// Global-logger message (`name: "pnpm:global"`). Written to a
    /// `bole('pnpm:global')` logger with just a message string — no
    /// `prefix`, unlike [`LogEvent::Pnpm`]. The interactive
    /// web-authentication flow (`pnpm-network-web-auth`) emits on this
    /// channel to surface the auth URL / QR code and the browser-open
    /// prompts. `@pnpm/cli.default-reporter` routes these into the "other"
    /// log stream.
    #[serde(rename = "pnpm:global")]
    Global(GlobalLog),

    /// One per `context.log(...)` call a pnpmfile hook makes while it
    /// runs (`pnpm:hook`). `readPackage` and `afterAllResolved` hooks
    /// receive a `context` whose `log` forwards here, so a pnpmfile can
    /// surface why it rewrote a manifest or lockfile. `@pnpm/cli.default-reporter`
    /// routes these into the "other" log stream.
    #[serde(rename = "pnpm:hook")]
    Hook(HookLog),

    /// Total command wall-clock time (`pnpm:execution-time`). Emitted once
    /// per CLI run after the command finishes; the default reporter renders
    /// it as the `Done in <time> using <pkg> v<version>` footer.
    #[serde(rename = "pnpm:execution-time")]
    ExecutionTime(ExecutionTimeLog),

    /// Deprecated-package warning (`pnpm:deprecation`). Emitted once per
    /// newly-resolved package whose registry manifest carries a `deprecated`
    /// field and whose name/version is not covered by
    /// `allowedDeprecatedVersions`. Direct dependencies (`depth == 0`) are
    /// rendered immediately; transitive ones are buffered and summarized at
    /// `pnpm:stage` time.
    #[serde(rename = "pnpm:deprecation")]
    Deprecation(DeprecationLog),

    /// Unmet peer dependencies left behind by a resolving install
    /// (`pnpm:peer-dependency-issues`). Emitted once per install that
    /// resolved, and only when at least one issue survives the
    /// project's `peerDependencyRules`; the default reporter renders a
    /// single line pointing at `pnpm peers check`. Under
    /// `strictPeerDependencies` the install fails instead of emitting
    /// this, matching pnpm.
    #[serde(rename = "pnpm:peer-dependency-issues")]
    PeerDependencyIssues(PeerDependencyIssuesLog),
}

/// `pnpm:request-retry` payload. `attempt` is one-indexed (the failed
/// attempt that triggered the retry) and `timeout` is the
/// milliseconds the retry loop will sleep before the next attempt;
/// pnpm's default reporter renders both directly.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRetryLog {
    pub level: LogLevel,
    pub attempt: u32,
    pub error: RequestRetryError,
    pub max_retries: u32,
    pub method: String,
    pub timeout: u64,
    pub url: String,
}

/// JS-shaped error object the default-reporter dispatches on:
/// `error.httpStatusCode ?? error.status ?? error.errno ?? error.code`
/// is what gets rendered as the reason. pacquet populates whichever
/// field its `pnpm_tarball::TarballError` variant maps to (HTTP
/// status → `http_status_code`, decode / IO failures → `code`) and
/// always carries the rendered `message` so consumers that read
/// `err.message` directly still work.
///
/// Plain backticks (not an intra-doc link) because `pnpm-reporter`
/// cannot depend on `pnpm-tarball` — the dependency runs the
/// other way.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRetryError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errno: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// `pnpm:lifecycle` payload. Same flatten-on-presence pattern as
/// [`PackageManifestLog`] / [`RootLog`].
#[derive(Debug, Clone, Serialize)]
pub struct LifecycleLog {
    pub level: LogLevel,
    #[serde(flatten)]
    pub message: LifecycleMessage,
}

/// `pnpm:lifecycle` discriminated payload. A union of three shapes
/// that pnpm's reporter dispatches on by presence of `script`,
/// `line`, or `exitCode`. `#[serde(untagged)]` matches that shape so
/// consumers (notably `@pnpm/cli.default-reporter`) accept the record
/// unchanged.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum LifecycleMessage {
    /// Emitted once before each hook spawns.
    Script {
        #[serde(rename = "depPath")]
        dep_path: String,
        optional: bool,
        script: String,
        stage: String,
        wd: String,
    },
    /// One event per stdout/stderr line read from the spawned script.
    /// `line` is the raw text of the output line.
    Stdio {
        #[serde(rename = "depPath")]
        dep_path: String,
        line: String,
        stage: String,
        stdio: LifecycleStdio,
        wd: String,
    },
    /// Emitted once after the script exits with the resolved exit
    /// code.
    Exit {
        #[serde(rename = "depPath")]
        dep_path: String,
        #[serde(rename = "exitCode")]
        exit_code: i32,
        optional: bool,
        stage: String,
        wd: String,
    },
}

/// Stdio channel discriminator on a [`LifecycleMessage::Stdio`] event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleStdio {
    Stdout,
    Stderr,
}

/// `pnpm:ignored-scripts` payload. Emitted once per install with the
/// names of every package whose lifecycle scripts were skipped because
/// the package was not in `allowBuilds`. Names are deduplicated and in
/// `name@version` form.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IgnoredScriptsLog {
    pub level: LogLevel,
    pub package_names: Vec<String>,
    /// `strictDepBuilds` at emit time. Carried in-memory only —
    /// `#[serde(skip)]` keeps it out of the `pnpm:ignored-scripts` NDJSON
    /// wire shape — so the default reporter can suppress the warning box
    /// under strict mode (where the install fails with
    /// `ERR_PNPM_IGNORED_BUILDS` instead) without relying on a stale
    /// global flag. The structured event itself is always emitted with
    /// the package names, matching pnpm's `ignoredScriptsLogger.debug`.
    #[serde(skip)]
    pub strict_dep_builds: bool,
}

/// `pnpm:update-check` payload: the running pnpm version and the latest
/// one the registry resolved for the `latest` tag.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckLog {
    pub level: LogLevel,
    pub current_version: String,
    pub latest_version: String,
}

/// `pnpm:installing-config-deps` payload. `status` is `started` (no
/// `deps`) or `done` (with the installed list).
#[derive(Debug, Clone, Serialize)]
pub struct InstallingConfigDepsLog {
    pub level: LogLevel,
    pub status: InstallingConfigDepsStatus,
    /// Empty (and omitted from the wire shape) on `started`; the
    /// installed packages on `done`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<InstalledConfigDep>,
}

/// `status` discriminator on a [`InstallingConfigDepsLog`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallingConfigDepsStatus {
    Started,
    Done,
}

/// One installed config dependency in a `done`
/// [`InstallingConfigDepsLog`].
#[derive(Debug, Clone, Serialize)]
pub struct InstalledConfigDep {
    pub name: String,
    pub version: String,
}

/// `pnpm:_broken_node_modules` payload. `missing` is the absolute
/// path to the snapshot's `node_modules/<pkg>` slot that the current-
/// lockfile lookup expected on disk but didn't find.
#[derive(Debug, Clone, Serialize)]
pub struct BrokenModulesLog {
    pub level: LogLevel,
    pub missing: String,
}

/// `pnpm:lockfile-verification` payload. The [bunyan]-envelope `level`
/// is a fixed outer field; the rest of the record is a status-tagged
/// union via `#[serde(flatten)]` so the wire shape stays flat (the
/// [`LockfileVerificationMessage`] discriminator on `status`).
///
/// [bunyan]: https://github.com/trentm/node-bunyan
#[derive(Debug, Clone, Serialize)]
pub struct LockfileVerificationLog {
    pub level: LogLevel,
    #[serde(flatten)]
    pub message: LockfileVerificationMessage,
}

/// `pnpm:lockfile-verification` discriminated payload. `Started`
/// fires once before the per-candidate fan-out begins; throttled
/// `Progress` events may fire while it runs; exactly one terminal
/// `Done` or `Failed` fires after, with `elapsed_ms` measured
/// against the matching `Started`. `Cached` fires instead
/// of the pair when the verification cache short-circuits the gate;
/// it carries no `entries` count because the short-circuit happens
/// before candidates are collected.
///
/// `lockfile_path` is the absolute path of the lockfile being
/// verified. It's `Option` because the runner is invoked without a
/// path in unit tests that skip the cache wiring; production code
/// paths always supply it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LockfileVerificationMessage {
    Started {
        entries: u64,
        #[serde(rename = "lockfilePath", skip_serializing_if = "Option::is_none")]
        lockfile_path: Option<String>,
    },
    Progress {
        entries: u64,
        /// Number of entries that have completed verification so far.
        checked: u64,
        #[serde(rename = "lockfilePath", skip_serializing_if = "Option::is_none")]
        lockfile_path: Option<String>,
    },
    Done {
        entries: u64,
        /// Number of entries that were checked before finishing.
        /// On success this equals `entries` — all entries were verified.
        checked: u64,
        #[serde(rename = "elapsedMs")]
        elapsed_ms: u64,
        #[serde(rename = "lockfilePath", skip_serializing_if = "Option::is_none")]
        lockfile_path: Option<String>,
    },
    Failed {
        entries: u64,
        /// Number of entries that were checked before the failure.
        /// Zero only on the paths where the fan-out never ran to
        /// completion (panic, registry-fetch abort) and the count is
        /// unknown.
        checked: u64,
        #[serde(rename = "elapsedMs")]
        elapsed_ms: u64,
        #[serde(rename = "lockfilePath", skip_serializing_if = "Option::is_none")]
        lockfile_path: Option<String>,
    },
    Cached {
        /// ISO-8601 timestamp of the verification run the cached
        /// verdict was recorded by. Omitted when the cache record
        /// predates the field.
        #[serde(rename = "verifiedAt", skip_serializing_if = "Option::is_none")]
        verified_at: Option<String>,
        #[serde(rename = "lockfilePath", skip_serializing_if = "Option::is_none")]
        lockfile_path: Option<String>,
    },
}

/// Generic-channel (`name: "pnpm"`) payload, used for `logger.info`-style
/// emits with no dedicated channel. `prefix` carries the install root the
/// message applies to, matching pnpm's wire shape.
#[derive(Debug, Clone, Serialize)]
pub struct PnpmLog {
    pub level: LogLevel,
    pub message: String,
    pub prefix: String,
}

/// The error payload bole serializes under `err`.
#[derive(Debug, Clone, Serialize)]
pub struct PnpmErrorLog {
    pub code: String,
    pub message: String,
}

/// Keeps the structured dedupe diff on the wire while retaining a
/// terminal-only rendering for the in-process default reporter.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupeCheckLog {
    pub level: LogLevel,
    pub message: String,
    pub err: PnpmErrorLog,
    pub dedupe_check_issues: serde_json::Value,
    #[serde(skip)]
    pub rendered: String,
}

/// `pnpm:peer-dependency-issues` payload.
///
/// `issues_by_projects` is the same `importerId -> issues` map pnpm's
/// `peerDependencyIssuesLogger` carries, already filtered through
/// `peerDependencyRules`, so an NDJSON consumer sees the detail the
/// one-line terminal rendering leaves out.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerDependencyIssuesLog {
    pub level: LogLevel,
    pub issues_by_projects: serde_json::Value,
}

/// `pnpm:scope` payload: how many workspace projects the command
/// selected, out of how many the workspace has.
///
/// `total` accompanies a workspace-wide run — including a `--filter` that
/// narrowed it to one project — and is absent from the single-project
/// shape a command targeting only the project it was run in reports.
/// `workspace_prefix` is absent outside a workspace, which is what makes
/// the reporter say "projects" rather than "workspace projects". Both are
/// the shapes pnpm's `ScopeMessage` distinguishes.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeLog {
    pub level: LogLevel,
    pub selected: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(rename = "workspacePrefix", skip_serializing_if = "Option::is_none")]
    pub workspace_prefix: Option<String>,
}

/// Global-channel (`name: "pnpm:global"`) payload. Carries only a
/// severity and a message — pnpm's `bole('pnpm:global')` logger takes a
/// bare string, with no `prefix`, so this struct has none either.
#[derive(Debug, Clone, Serialize)]
pub struct GlobalLog {
    pub level: LogLevel,
    pub message: String,
}

/// `pnpm:hook` payload. Field names match pnpm's `HookMessage` so
/// `@pnpm/cli.default-reporter` accepts the record unchanged. `from`
/// is the pnpmfile that defined the hook, `hook` is the hook name
/// (`readPackage` / `afterAllResolved`), `prefix` is the project the
/// hook ran for, and `message` is the string passed to `context.log`.
/// The hook context logger emits at `debug`.
#[derive(Debug, Clone, Serialize)]
pub struct HookLog {
    pub level: LogLevel,
    pub from: String,
    pub hook: String,
    pub message: String,
    pub prefix: String,
}

/// `pnpm:execution-time` payload. `started_at` / `ended_at` are
/// Unix-epoch milliseconds; the reporter renders their difference. Field
/// names match pnpm's wire shape (`startedAt` / `endedAt`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTimeLog {
    pub level: LogLevel,
    pub started_at: u128,
    pub ended_at: u128,
}

/// `pnpm:deprecation` payload. Field names match the `DeprecationMessage`
/// wire shape in `pnpm11/core/core-loggers/src/deprecationLogger.ts` so
/// `@pnpm/cli.default-reporter` dispatches on them unchanged.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeprecationLog {
    pub level: LogLevel,
    pub pkg_name: String,
    pub pkg_version: String,
    pub pkg_id: String,
    pub prefix: String,
    pub deprecated: String,
    pub depth: i32,
}

/// Severity level on the [bunyan]-shaped envelope.
///
/// pnpm's logger uses the [bole] library, which writes one of these strings
/// for every record. Each channel pins the level pnpm itself uses (e.g.
/// `pnpm:stage` is always emitted at `debug`).
///
/// [bunyan]: https://github.com/trentm/node-bunyan
/// [bole]: https://github.com/rvagg/bole
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Capability for emitting log events.
///
/// Implementations are unit structs; any implementation-internal state
/// lives in module-level `static`s. Emitting code is generic over
/// `R: Reporter` and calls `R::emit(...)`; the production entry point
/// monomorphises with the chosen sink.
///
/// [`Reporter::emit`] must not panic. A serialization or I/O failure is
/// swallowed so a reporter problem can never crash an install.
///
/// **Thread safety.** `emit` may be invoked concurrently from
/// arbitrary threads — pacquet's import path runs `link_file` from a
/// rayon `par_iter`, and tarball download / store-index work runs
/// across tokio workers, all of which can fire reporter events at
/// once. Implementations must therefore guard any shared state they
/// touch (`Mutex`, atomic, or write-once initialization). Both
/// production sinks satisfy this: [`SilentReporter`] is a no-op, and
/// [`NdjsonReporter`] serializes per-event then writes under
/// `std::io::stderr().lock()`.
///
/// The `Send + Sync + 'static` supertraits state the same contract in
/// the type system, so emitting code can hand `R` to a spawned task
/// (the concurrent lockfile-verification gate) without re-declaring the
/// bounds at every generic hop. Implementations are unit structs, which
/// satisfy them automatically.
pub trait Reporter: Send + Sync + 'static {
    fn emit(event: &LogEvent);
}

/// Adapt a [`Reporter`] into the warning callback used by the network client.
pub fn emit_global_warning<Sink: Reporter>(message: &str) {
    Sink::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Warn,
        message: message.to_string(),
    }));
}

/// `--reporter=silent`: every event is dropped.
pub struct SilentReporter;

impl Reporter for SilentReporter {
    fn emit(_event: &LogEvent) {}
}

/// `--reporter=ndjson`: writes one [bunyan]-shaped JSON record per event to
/// stderr, terminated by `\n`. The wire format matches what pnpm itself
/// produces under `--reporter=ndjson`, so the same consumers work
/// unmodified.
///
/// Today this writes synchronously under the stderr lock. When the volume
/// of emit sites grows past coarse start/end markers, the writer should
/// move behind an MPSC channel.
///
/// [bunyan]: https://github.com/trentm/node-bunyan
pub struct NdjsonReporter;

impl Reporter for NdjsonReporter {
    fn emit(event: &LogEvent) {
        let mut buf = Vec::with_capacity(256);
        if write_record(&mut buf, event).is_err() {
            return;
        }
        buf.push(b'\n');
        let _ = std::io::stderr().lock().write_all(&buf);
    }
}

fn write_record(buf: &mut Vec<u8>, event: &LogEvent) -> serde_json::Result<()> {
    let envelope =
        Envelope { time: now_millis(), hostname: &HOSTNAME, pid: std::process::id(), event };
    serde_json::to_writer(buf, &envelope)
}

// Wraps a [`LogEvent`] with the bunyan envelope fields pnpm's logger adds.
// `#[serde(flatten)]` merges the channel-specific tag and payload fields up
// to the top level of the JSON object so the wire format is one flat record
// per line.
#[derive(Serialize)]
struct Envelope<'a> {
    time: u128,
    hostname: &'a str,
    pid: u32,
    #[serde(flatten)]
    event: &'a LogEvent,
}

fn now_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// Capability for obtaining the host name written into the [bunyan]-shaped
/// envelope.
///
/// Backed by a real syscall in production via [`Host`]. The envelope itself
/// reads from a process-cached `HOSTNAME` `static` initialized by
/// [`Host::get_host_name`], so the value is fixed for the lifetime of the
/// process and the envelope path is **not** currently generic over this
/// trait. The trait therefore exists for two narrow reasons: to keep the
/// `gethostname` syscall behind a named seam (so the production call site
/// is consistent with the rest of `Host`'s capability surface), and so the
/// capability can be exercised in isolation by unit tests. Substituting a
/// hostname per-test in the rendered envelope would require plumbing a
/// `Sys: GetHostName` generic through the emission site, which has not
/// been done.
///
/// [bunyan]: https://github.com/trentm/node-bunyan
pub trait GetHostName {
    fn get_host_name() -> String;
}

/// Production implementation of the capability traits in this crate.
///
/// Each trait method calls into the real underlying system facility (for
/// [`GetHostName`], the `gethostname` syscall via the [`gethostname`] crate).
pub struct Host;

impl GetHostName for Host {
    fn get_host_name() -> String {
        gethostname::gethostname().to_string_lossy().into_owned()
    }
}

// Process-wide cache of the host name. The value cannot change at runtime,
// and `gethostname` is one syscall we'd otherwise repeat on every emit.
// Initialized lazily through `Host::get_host_name` so tests that exercise
// the capability trait directly can do so without paying for the syscall.
static HOSTNAME: LazyLock<String> = LazyLock::new(Host::get_host_name);

#[cfg(test)]
mod tests;

mod progress;

mod dependencies;
