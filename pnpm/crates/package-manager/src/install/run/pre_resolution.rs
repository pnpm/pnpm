//! The pnpmfile's `preResolution` hook.

use super::{
    super::{Arc, InstallError, Lockfile, Path, Reporter},
    InstallView,
    lockfile_load::Loaded,
    wanted::WantedLockfile,
    workspace::InstallWorkspace,
};
use pnpm_reporter::{HookLog, LogEvent, LogLevel};
use serde_json::Value;

/// Run the `preResolution` hook on the settled lockfiles, before the install
/// decides whether the wanted lockfile is up to date. The install continues
/// from whatever the hook changed in either lockfile.
///
/// Skipped under `ignore_manifest_check`, for the reason
/// `pnpm:devPreinstall` is: a delegating CLI already ran it.
pub(super) async fn run_pre_resolution_hook<Reporter: self::Reporter>(
    install: InstallView<'_>,
    workspace: &InstallWorkspace<'_>,
    loaded: &mut Loaded<'_>,
    wanted: &mut WantedLockfile<'_>,
) -> Result<(), InstallError> {
    let Some(hook) = loaded.pnpmfile_hook.clone() else { return Ok(()) };
    if install.lockfile_policy.ignore_manifest_check
        || !hook.has_pre_resolution().await.map_err(InstallError::PreResolutionHook)?
    {
        return Ok(());
    }
    let config = install.context.config;
    let lockfile_dir = &workspace.dirs.workspace_root;
    let wanted_json = lockfile_json(wanted.get())?;
    let current_json = lockfile_json(loaded.current.as_ref())?;
    let ctx = pnpm_hooks::PreResolutionHookContext {
        wanted_lockfile: wanted_json.clone(),
        current_lockfile: current_json.clone(),
        exists_current_lockfile: loaded.current.is_some(),
        exists_non_empty_wanted_lockfile: wanted.loaded.is_some()
            && wanted
                .get()
                .is_some_and(|lockfile| {
                    lockfile.snapshots
                        .as_ref()
                        .is_some_and(|snapshots| !snapshots.is_empty())
                }),
        lockfile_dir: lockfile_dir.to_string_lossy().into_owned(),
        store_dir: config.store_dir.display().to_string(),
        registries: serde_json::json!(config.resolved_registries()),
    };
    let logger = pnpm_hooks::PreResolutionHookLogger {
        info: pre_resolution_log_fn::<Reporter>(lockfile_dir, LogLevel::Info),
        warn: pre_resolution_log_fn::<Reporter>(lockfile_dir, LogLevel::Warn),
    };
    let hooked = hook.pre_resolution(ctx, logger).await.map_err(InstallError::PreResolutionHook)?;
    if hooked.wanted_lockfile != wanted_json {
        wanted.set_hooked(parse_hooked_lockfile("wantedLockfile", hooked.wanted_lockfile)?);
    }
    if hooked.current_lockfile != current_json {
        loaded.current = Some(parse_hooked_lockfile("currentLockfile", hooked.current_lockfile)?);
    }
    Ok(())
}

/// The hook sees a missing lockfile as `{}`.
fn lockfile_json(lockfile: Option<&Lockfile>) -> Result<Value, InstallError> {
    lockfile.map_or_else(
        || Ok(Value::Object(serde_json::Map::new())),
        |lockfile| serde_json::to_value(lockfile).map_err(InstallError::PreResolutionSerialize),
    )
}

fn parse_hooked_lockfile(field: &'static str, value: Value) -> Result<Lockfile, InstallError> {
    serde_json::from_value(value)
        .map_err(|error| InstallError::BadPreResolutionHookResult { field, error })
}

/// Build one side of the `preResolution` hook's `logger`: each
/// `logger.info(...)` / `logger.warn(...)` call emits a `pnpm:hook` event at
/// the given level. `from` is the literal `"pnpmfile"`, as pnpm's
/// `createPreResolutionHookLogger` hardcodes it.
fn pre_resolution_log_fn<Reporter: self::Reporter>(
    prefix: &Path,
    level: LogLevel,
) -> pnpm_hooks::LogFn {
    let prefix = prefix.to_string_lossy().into_owned();
    Arc::new(move |message: String| {
        Reporter::emit(&LogEvent::Hook(HookLog {
            level,
            from: "pnpmfile".to_string(),
            hook: "preResolution".to_string(),
            message,
            prefix: prefix.clone(),
        }));
    })
}
