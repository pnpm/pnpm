use super::{LogLevel, Serialize};

/// `pnpm:context` payload.
///
/// Emitted once per install when the install context has been
/// constructed. Field names match pnpm's wire shape (camelCase) so
/// `@pnpm/cli.default-reporter` accepts the record unchanged.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextLog {
    pub level: LogLevel,
    pub current_lockfile_exists: bool,
    pub store_dir: String,
    pub virtual_store_dir: String,
}

/// `pnpm:stage` payload.
///
/// `prefix` is the project root path the stage applies to, matching pnpm's
/// usage. `stage` is the phase marker; see [`Stage`].
#[derive(Debug, Clone, Serialize)]
pub struct StageLog {
    pub level: LogLevel,
    pub prefix: String,
    pub stage: Stage,
}

/// `pnpm:prompt` payload.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PromptLog {
    pub level: LogLevel,
    pub action: PromptAction,
}

/// Whether an interactive prompt is acquiring or releasing the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptAction {
    Start,
    End,
}

/// `pnpm:stage` phase marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    ResolutionStarted,
    ResolutionDone,
    ImportingStarted,
    ImportingDone,
}

/// `pnpm:summary` payload. `prefix` identifies the importer; pnpm's
/// reporter uses it to look up the matching `pnpm:root` history and
/// render its "+N -M" diff. `level` is the [bunyan]-envelope severity,
/// common to every channel.
///
/// [bunyan]: https://github.com/trentm/node-bunyan
#[derive(Debug, Clone, Serialize)]
pub struct SummaryLog {
    pub level: LogLevel,
    pub prefix: String,
}

/// `pnpm:package-import-method` payload. The method names match pnpm's
/// wire shape exactly — anything else would silently fail to render
/// even though the JSON parses.
#[derive(Debug, Clone, Serialize)]
pub struct PackageImportMethodLog {
    pub level: LogLevel,
    pub method: PackageImportMethod,
}

/// Wire-format import method. pnpm only knows three values; pacquet's
/// config enum (`pnpm_config::PackageImportMethod`) carries `Auto`
/// and `CloneOrCopy` on top of those, but those are dispatched-on by
/// the auto-importer's fallback chain, not emitted. The wire value is
/// the resolved method `link_file` actually used — `Clone` /
/// `Hardlink` / `Copy` — so an `auto` install that falls back to
/// hardlink emits `hardlink`, not the optimistic `clone`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageImportMethod {
    Clone,
    Hardlink,
    Copy,
}

/// `pnpm:progress` payload. The bunyan-envelope `level` is a fixed
/// outer field; the rest of the record is a status-tagged union via
/// `#[serde(flatten)]` so the wire shape stays flat (matching pnpm's
/// [`ProgressMessage`] discriminator on `status`).
#[derive(Debug, Clone, Serialize)]
pub struct ProgressLog {
    pub level: LogLevel,
    #[serde(flatten)]
    pub message: ProgressMessage,
}

/// `pnpm:progress` discriminated payload.
///
/// `requester` is the install root — same value as the
/// [`StageLog::prefix`] threaded through `Install::run`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProgressMessage {
    Resolved {
        #[serde(rename = "packageId")]
        package_id: String,
        requester: String,
    },
    Fetched {
        #[serde(rename = "packageId")]
        package_id: String,
        requester: String,
    },
    FoundInStore {
        #[serde(rename = "packageId")]
        package_id: String,
        requester: String,
    },
    Imported {
        method: PackageImportMethod,
        requester: String,
        to: String,
    },
}

/// `pnpm:fetching-progress` payload. Same flatten-on-status pattern as
/// [`ProgressLog`].
#[derive(Debug, Clone, Serialize)]
pub struct FetchingProgressLog {
    pub level: LogLevel,
    #[serde(flatten)]
    pub message: FetchingProgressMessage,
}

/// `pnpm:fetching-progress` discriminated payload. `size` is derived
/// from the response's `Content-Length`, and is unknown when the
/// response is chunked. pacquet throttles `InProgress` events to ~200ms
/// per package, mirroring pnpm's reporter coalescing window.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FetchingProgressMessage {
    Started {
        attempt: u32,
        #[serde(rename = "packageId")]
        package_id: String,
        size: Option<u64>,
    },
    InProgress {
        downloaded: u64,
        #[serde(rename = "packageId")]
        package_id: String,
    },
}

/// `pnpm:stats` payload. Same flatten-on-presence pattern as
/// [`PackageManifestLog`](crate::PackageManifestLog) / [`RootLog`](crate::RootLog).
#[derive(Debug, Clone, Serialize)]
pub struct StatsLog {
    pub level: LogLevel,
    #[serde(flatten)]
    pub message: StatsMessage,
}

/// `pnpm:stats` discriminated payload. pnpm's reporter dispatches on
/// presence: an event carries either `added` *or* `removed`, never
/// both, because pnpm emits them from two separate sites.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum StatsMessage {
    Added { prefix: String, added: u64 },
    Removed { prefix: String, removed: u64 },
}
