use crate::{
    MaxLogLevel,
    SummaryScope,
};

/// Rendering settings that cannot be recovered from the event stream.
#[derive(Debug, Clone)]
pub struct ReporterOptions {
    /// Emit each update as a new line instead of replacing the current frame.
    pub append_only: bool,
    /// Verbosity ceiling from pnpm's `--loglevel` setting.
    pub max_log_level: MaxLogLevel,
    /// Replaces the second line of the ignored-builds box — the one that
    /// tells the user how to approve a build. pnpm's
    /// `approveBuildsInstructionText`, for embedders whose users approve
    /// builds through the embedder's own configuration rather than
    /// `pnpm approve-builds`.
    pub ignored_builds_instruction_text: Option<String>,
    /// Package-name patterns whose *linked* entries are left out of the
    /// packages-diff summary — an entry is linked when it carries a
    /// `from`, i.e. it is symlinked in rather than materialized from the
    /// store. The Rust counterpart of the TypeScript reporter's
    /// `filterPkgsDiff` callback: an embedder that links its own runtime
    /// into every project (Bit's core aspects) silences that noise
    /// without silencing the same packages when they are really
    /// installed.
    pub hide_linked_pkgs_diff: Vec<String>,
    pub lifecycle: LifecycleOptions,
    pub progress: ProgressOptions,
    pub scope: ScopeOptions,
}

#[derive(Debug, Default, Clone)]
pub struct LifecycleOptions {
    /// Keep lifecycle script output in its collapsed block instead of
    /// streaming each line, even in append-only mode. pnpm's
    /// `hideLifecycleOutput`, which the TypeScript reporter applies by
    /// forcing the lifecycle stream's own `appendOnly` off.
    pub hide_output: bool,
    /// Stream lifecycle script output line by line even when the rest of
    /// the frame renders in place. pnpm's `streamLifecycleOutput`, which
    /// its reporter implements by turning on the lifecycle stream's own
    /// `appendOnly`.
    pub stream_output: bool,
    /// Hold each script's streamed lines until it exits, then print the
    /// whole run as one block. pnpm's `aggregateOutput`.
    pub aggregate_output: bool,
    /// Drop the project prefix from streamed script output lines. pnpm's
    /// `hideLifecyclePrefix` — the `$ <script>` and `Done` / `Failed`
    /// lines keep theirs.
    pub hide_prefix: bool,
}

#[derive(Debug, Default, Clone)]
pub struct ProgressOptions {
    /// Omit the `added` counter from dependency progress lines.
    pub hide_added_pkgs: bool,
    /// Omit the workspace-project prefix from progress lines.
    pub hide_prefix: bool,
}

#[derive(Debug, Clone)]
pub struct ScopeOptions {
    /// Select which project prefixes contribute to the package summary.
    pub summary: SummaryScope,
    /// Whether the running command reports the workspace scope it
    /// selected. Mirrors pnpm's `COMMANDS_THAT_REPORT_SCOPE` gate, which
    /// lives in the reporter because the `pnpm:scope` event itself is
    /// command-agnostic.
    pub reports_scope: bool,
    /// Whether direct dependency warnings use workspace-relative prefixes.
    pub recursive: bool,
}

impl Default for ReporterOptions {
    fn default() -> Self {
        Self {
            append_only: false,
            max_log_level: MaxLogLevel::Info,
            ignored_builds_instruction_text: None,
            hide_linked_pkgs_diff: Vec::new(),
            lifecycle: crate::state::LifecycleOptions {
                hide_output: false,
                stream_output: false,
                aggregate_output: false,
                hide_prefix: false,
            },
            progress: crate::state::ProgressOptions { hide_added_pkgs: false, hide_prefix: false },
            scope: crate::state::ScopeOptions {
                summary: SummaryScope::CurrentPrefix,
                reports_scope: false,
                recursive: false,
            },
        }
    }
}

impl Default for ScopeOptions {
    fn default() -> Self {
        Self { summary: SummaryScope::CurrentPrefix, reports_scope: false, recursive: false }
    }
}
