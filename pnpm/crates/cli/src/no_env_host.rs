use pnpm_config::{
    EnvVar,
    EnvVarOs,
    GetCurrentDir,
    GetHomeDir,
    LinkProbe,
};
use std::{
    ffi::OsString,
    path::{
        Path,
        PathBuf,
    },
};

/// A config host with no environment and no home directory, so only a
/// fixture's files and the command line reach the loaded config. The
/// developer's own `.npmrc` and global config stay out of the test.
pub(crate) struct NoEnv;

impl EnvVar for NoEnv {
    fn var(_: &str) -> Option<String> {
        None
    }
}

impl EnvVarOs for NoEnv {
    fn var_os(_: &str) -> Option<OsString> {
        None
    }
}

impl GetHomeDir for NoEnv {
    fn home_dir() -> Option<PathBuf> {
        None
    }
}

impl GetCurrentDir for NoEnv {
    fn current_dir() -> std::io::Result<PathBuf> {
        std::env::current_dir()
    }
}

impl LinkProbe for NoEnv {
    fn can_link_between_dirs(_: &Path, _: &Path) -> bool {
        true
    }
}
