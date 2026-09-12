use std::sync::Mutex;

use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use serde_json::Value;

use crate::{
    AddedRoot, BrokenModulesLog, ContextLog, DedupeCheckLog, DependencyType, DeprecationLog,
    Envelope, FetchingProgressLog, FetchingProgressMessage, GetHostName, GlobalLog, HookLog, Host,
    IgnoredScriptsLog, LifecycleLog, LifecycleMessage, LifecycleStdio, LockfileVerificationLog,
    LockfileVerificationMessage, LogEvent, LogLevel, PackageImportMethod, PackageImportMethodLog,
    PackageManifestLog, PackageManifestMessage, PeerDependencyIssuesLog, PnpmErrorLog, PnpmLog,
    ProgressLog, ProgressMessage, PromptAction, PromptLog, RemovedRoot, Reporter,
    RequestRetryError, RequestRetryLog, RootLog, RootMessage, SilentReporter,
    SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalParent,
    SkippedOptionalReason, Stage, StageLog, StatsLog, StatsMessage, SummaryLog,
};

mod behavior;

mod reporting;

mod manifests;

mod files;

mod dependencies;

mod lockfile;
