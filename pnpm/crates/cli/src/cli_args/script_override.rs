//! Script overrides for built-in commands, per
//! <https://pnpm.io/scripts#built-in-command-and-script-conflicts>.
//! The `pm` prefix forces the built-in command instead (see
//! [`crate::pm_prefix`]).

use super::{dispatch::RunCtx, recursive::RecursiveExecutionArgs, run::RunArgs};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_fs::lexical_normalize;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::safe_read_project_manifest_only;

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
/// `script_args` are the command's positionals, forwarded to the script —
/// `pnpm deploy <dir>` runs the `deploy` script with `<dir>` as its
/// argument, like pnpm 11's redirect.
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
    if overriding_script(safe_read_project_manifest_only(dir)?, command_name).is_some() {
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
        && overriding_script(safe_read_project_manifest_only(workspace_dir)?, command_name).is_some()
    {
        return Err(ScriptOverrideInWorkspaceRoot { command: command_name.to_string() }.into());
    }
    Ok(None)
}

/// The `<command_name>` script of an optional manifest, mirroring pnpm's
/// `safeReadProjectManifestOnly` tolerance for a missing `package.json`.
fn overriding_script(manifest: Option<PackageManifest>, command_name: &str) -> Option<String> {
    manifest
        .and_then(|manifest| {
            manifest
                .script(command_name, true)
                .ok()
                .flatten()
                .map(str::to_string)
        })
        .filter(|script| !script.is_empty())
}
