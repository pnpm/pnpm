use crate::ScriptsPrependNodePath;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub struct ScriptEnvironment<'a> {
    pub init_cwd: &'a Path,
    /// Path to a `node` binary for `npm_node_execpath` / `NODE`. When
    /// `None`, [`crate::build_env`] falls back to looking `node` up
    /// on `PATH`. Required for native postinstalls that shell out
    /// via `$NODE`.
    pub node_execpath: Option<&'a Path>,
    /// Path written into `npm_execpath` so postinstalls can re-invoke
    /// the package manager. When `None`, `std::env::current_exe()`
    /// is used.
    pub npm_execpath: Option<&'a Path>,
    /// `node-gyp` entry point written into `npm_config_node_gyp`.
    /// `None` leaves the variable unset, which is what pnpm does: the
    /// wrapper found through the `node_gyp_bin` execution option reads
    /// this variable and falls back to the shipped copy when it is
    /// unset, so setting it here would override a user's own choice.
    pub node_gyp_path: Option<&'a Path>,
    /// Value written into `npm_config_user_agent`. Caller-supplied
    /// (typically `"pnpm/<version>"`); `None` skips the stamp.
    pub user_agent: Option<&'a str>,
    pub extra_env: &'a HashMap<String, String>,
}

#[derive(Clone, Copy)]
pub struct ScriptExecutionOptions<'a> {
    pub extra_bin_paths: &'a [PathBuf],
    /// Directory holding the shipped `node-gyp` wrapper, prepended to
    /// `PATH` so install scripts that shell out to `node-gyp` resolve
    /// it. Supplied by [`crate::bundled_node_gyp_bin`]; `None` when
    /// nothing was shipped beside the executable.
    pub node_gyp_bin: Option<&'a Path>,
    /// Tri-state from `scriptsPrependNodePath` config. `Never` is the
    /// safe default; `Always` appends `dirname(node)` to `PATH`.
    pub prepend_node_path: ScriptsPrependNodePath,
    /// Custom shell from `scriptShell` config (e.g. `bash`,
    /// `/usr/local/bin/bash`). `None` means use the platform default
    /// (`sh -c` on POSIX, `cmd /d /s /c` on Windows).
    pub shell: Option<&'a Path>,
    /// The `shellEmulator` config: run the script through pacquet's
    /// built-in shell rather than the platform's. Callers that mirror a
    /// pnpm call site which does not thread the setting — publishing,
    /// packing, patching, git package preparation — pass `false`.
    pub shell_emulator: bool,
    /// The `.bin` holding the script directory's own executables, when
    /// `modulesDir` puts them somewhere other than
    /// `<dir>/node_modules/.bin`. `None` keeps `<dir>/node_modules/.bin`.
    pub wd_bin_dir: Option<&'a Path>,
}

pub struct ScriptInvocation<'a> {
    pub stage: &'a str,
    pub script: &'a str,
    pub args: &'a [String],
}
