//! Script overrides for built-in commands, per
//! <https://pnpm.io/scripts#built-in-command-and-script-name-conflicts>.

use super::{dispatch::RunCtx, recursive::RecursiveExecutionArgs, run::RunArgs};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_fs::lexical_normalize;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::safe_read_project_manifest_only;
use std::sync::atomic::Ordering;

/// `pnpm <command>` was invoked from a subdirectory of a workspace whose
/// root `package.json` declares a `<command>` script. pnpm refuses to run
/// the built-in from the subdirectory in that case (it would shadow the
/// root script), and directs the user to `pnpm run <script>` at the root.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "The workspace root has a \"{command}\" script, so the built-in \"pnpm {command}\" command cannot run from a subdirectory"
)]
#[diagnostic(
    code(ERR_PNPM_SCRIPT_OVERRIDE_IN_WORKSPACE_ROOT),
    help("Run \"pnpm run {command}\" from the workspace root to execute the script")
)]
struct ScriptOverrideInWorkspaceRoot {
    command: String,
}

/// Resolve a same-named script that replaces an overridable built-in
/// command, returning the `pnpm run <command>` invocation to dispatch.
/// `None` means the built-in command should run.
///
/// `command_name` is the name as typed, so each alias looks up its own
/// script: `pnpm rb` runs an `rb` script, not a `rebuild` one.
/// `script_args` are the command's positionals, forwarded to the script,
/// so `pnpm deploy <dir>` passes `<dir>` on to the `deploy` script.
///
/// A caller that has no other use for `Config` can keep the load behind
/// its own [`RunCtx::builtin_command_forced`] test.
pub(crate) fn resolve(
    ctx: &RunCtx<'_>,
    config: &Config,
    command_name: &str,
    script_args: Vec<String>,
) -> miette::Result<Option<RunArgs>> {
    if ctx.builtin_command_forced {
        return Ok(None);
    }
    let dir = ctx.locations.dir;
    if declares_script(safe_read_project_manifest_only(dir)?.as_ref(), command_name) {
        ctx.builtin_replaced_by_script.store(true, Ordering::Relaxed);
        return Ok(Some(RunArgs {
            script: RunArgs::script(command_name, script_args),
            if_present: false,
            sequential: false,
            dry_run: false,
            json: false,
            workspace: RecursiveExecutionArgs {
                resume_from: None,
                report_summary: false,
                no_bail: false,
                sort: true,
                reverse: false,
                parallel: false,
            },
        }));
    }
    if let Some(workspace_dir) = config.workspace_dir.as_deref()
        && lexical_normalize(workspace_dir) != lexical_normalize(dir)
        && declares_script(safe_read_project_manifest_only(workspace_dir)?.as_ref(), command_name)
    {
        return Err(ScriptOverrideInWorkspaceRoot { command: command_name.to_string() }.into());
    }
    Ok(None)
}

/// Whether `manifest` declares a `command_name` script with a body to run.
///
/// An empty script runs nothing, and pnpm reads it as no script at all, so
/// it leaves the built-in command in place.
fn declares_script(manifest: Option<&PackageManifest>, command_name: &str) -> bool {
    let Some(manifest) = manifest else { return false };
    matches!(manifest.script(command_name, true), Ok(Some(script)) if !script.is_empty())
}
