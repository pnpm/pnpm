use super::{OsString, Path, PathBuf, exec_program_with_path};

/// Run `program` with `bin_dirs` prepended to `PATH`. A JavaScript
/// package manager needs the Node.js it was provisioned with to be
/// reachable, and its own directory has to come first so a nested
/// invocation finds the same version.
pub(super) fn exec_program_with_bin_dirs(
    program: &Path,
    bin_dirs: &[PathBuf],
    args: &[OsString],
) -> i32 {
    match crate::path_env::prepend_dirs_to_path(bin_dirs) {
        // The `PATH` travels on the command rather than through this
        // process's own environment: an `exec` hands the child the
        // command's environment just the same, and nothing here has to
        // reason about which threads are running.
        Ok(path) => exec_program_with_path(program, args, Some(path.as_os_str())),
        Err(error) => {
            // Rendered as a report so the failure carries the same
            // `ERR_PNPM_BAD_PATH_DIR` code the commands report it under.
            eprintln!("pnpm: {:?}", miette::Report::new(error));
            1
        }
    }
}

/// Run `program` with `args`, replacing this process where the platform
/// allows. Exit codes follow the shell convention: 127 when the program
/// does not exist, 126 when it cannot be executed.
pub(super) fn exec_program(program: &Path, args: &[OsString]) -> i32 {
    exec_program_with_path(program, args, None)
}
