use crate::_utils;

/// Helpers for editing the `pnpm-workspace.yaml` that
/// [`CommandTempCwd::add_mocked_registry`] wrote. Both edit in place
/// rather than replacing the file: the harness's `storeDir`, `cacheDir`,
/// and `enableGlobalVirtualStore: false` keys have to survive, or the
/// test installs into the real store and the global virtual store moves
/// every package out of `node_modules/.pnpm`.
mod workspace_yaml;

mod dependency_build_scripts;

/// `.modules.yaml`'s `pendingBuilds` — the record of builds
/// `--ignore-scripts` deferred, which `pacquet rebuild --pending`
/// later drains.
mod pending_builds;

/// Project (workspace/root) lifecycle scripts run during
/// `pacquet install` — preinstall, install, postinstall, preprepare,
/// prepare, postprepare — as opposed to the dependency build scripts
/// the [`dependency_build_scripts`] module above exercises.
mod project_scripts;

/// Which *workspace* projects run their own lifecycle scripts, which
/// pnpm decides from the mutated-importer list its command layer builds:
/// the projects the command was pointed at, plus the workspace root,
/// which its recursive dispatch pushes in as a full install whenever the
/// selection leaves it out.
mod project_scripts_in_a_workspace;

/// `scriptShell` selects the shell every lifecycle script is spawned
/// under, not only the one `pnpm run` uses. Unix-only: the probe is a
/// shell shim, and `select_shell` rejects `.cmd`/`.bat` shims on
/// Windows anyway.
#[cfg(unix)]
mod script_shell;

/// `shellEmulator` extends to every lifecycle script an install runs,
/// not only to `pnpm run`. Each test points `scriptShell` at a path that
/// could never be spawned, so an install that still succeeds proves the
/// built-in shell took over.
mod shell_emulator;
