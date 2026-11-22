// Copyright 2018-2025 the Deno authors. MIT license.

use std::{
    borrow::Cow,
    ffi::{
        OsStr,
        OsString,
    },
    path::{
        Path,
        PathBuf,
    },
};

use thiserror::Error;

use super::ShellState;

/// Error when a command path could not be resolved.
#[derive(Error, Debug, PartialEq)]
pub enum CommandPathResolutionError {
    #[error("{}: command not found", .0.to_string_lossy())]
    CommandNotFound(OsString),
    #[error("{}: failed canonicalizing", .0.to_string_lossy())]
    FailedCanonicalizing(OsString),
    #[error("command name was empty")]
    CommandEmpty,
}

impl CommandPathResolutionError {
    pub fn exit_code(&self) -> i32 {
        match self {
            // Use the Exit status that is used in bash: https://www.gnu.org/software/bash/manual/bash.html#Exit-Status
            CommandPathResolutionError::CommandNotFound(_) => 127,
            CommandPathResolutionError::CommandEmpty
            | CommandPathResolutionError::FailedCanonicalizing(_) => 1,
        }
    }
}

/// Resolves a command name to an absolute path.
pub fn resolve_command_path(
    command_name: &OsStr,
    base_dir: &Path,
    state: &ShellState,
) -> Result<PathBuf, CommandPathResolutionError> {
    if command_name.is_empty() {
        return Err(CommandPathResolutionError::CommandEmpty);
    }

    // check for absolute or relative path
    let path = Path::new(command_name);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    } else if path.components().count() > 1 {
        return Ok(base_dir.join(command_name));
    }

    let result = which::WhichConfig::new_with_sys(state.clone())
        .binary_name(command_name.to_os_string())
        .custom_cwd(base_dir.to_path_buf())
        .first_result();
    result.map_err(|err| match err {
        which::Error::CannotFindBinaryPath | which::Error::CannotGetCurrentDirAndPathListEmpty => {
            CommandPathResolutionError::CommandNotFound(command_name.into())
        }
        which::Error::CannotCanonicalize => {
            CommandPathResolutionError::FailedCanonicalizing(command_name.into())
        }
    })
}

impl which::sys::Sys for ShellState {
    type ReadDirEntry = std::fs::DirEntry;

    type Metadata = std::fs::Metadata;

    fn is_windows(&self) -> bool {
        cfg!(windows)
    }

    fn current_dir(&self) -> std::io::Result<PathBuf> {
        Ok(self.cwd().to_path_buf())
    }

    fn home_dir(&self) -> Option<PathBuf> {
        // disable tilde expansion because that's handled by the shell
        None
    }

    fn env_split_paths(&self, paths: &OsStr) -> Vec<PathBuf> {
        std::env::split_paths(paths).collect()
    }

    fn env_path(&self) -> Option<OsString> {
        self.get_var(OsStr::new("PATH"))
            .cloned()
    }

    fn env_path_ext(&self) -> Option<OsString> {
        self.get_var(OsStr::new("PATHEXT"))
            .cloned()
    }

    fn env_windows_path_ext(&self) -> Cow<'static, [String]> {
        Cow::Owned(
            self.get_var(OsStr::new("PATHEXT"))
                .and_then(|pathext| {
                    Some(
                        pathext
                            .to_str()?
                            .split(';')
                            .filter_map(|s| {
                                if s.as_bytes().first() == Some(&b'.') {
                                    Some(s.to_owned())
                                } else {
                                    // Invalid segment; just ignore it.
                                    None
                                }
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .unwrap_or_else(|| {
                    vec![
                        ".EXE".to_string(),
                        ".CMD".to_string(),
                        ".BAT".to_string(),
                        ".COM".to_string(),
                    ]
                }),
        )
    }

    fn metadata(&self, path: &Path) -> std::io::Result<Self::Metadata> {
        std::fs::metadata(path)
    }

    fn symlink_metadata(&self, path: &Path) -> std::io::Result<Self::Metadata> {
        std::fs::symlink_metadata(path)
    }

    fn read_dir(
        &self,
        path: &Path,
    ) -> std::io::Result<Box<dyn Iterator<Item = std::io::Result<Self::ReadDirEntry>>>> {
        let iter = std::fs::read_dir(path)?;
        Ok(Box::new(iter))
    }

    #[cfg(unix)]
    fn is_valid_executable(&self, path: &std::path::Path) -> std::io::Result<bool> {
        use nix::unistd::{
            AccessFlags,
            access,
        };

        match access(path, AccessFlags::X_OK) {
            Ok(()) => Ok(true),
            Err(nix::errno::Errno::ENOENT) => Ok(false),
            Err(e) => Err(std::io::Error::from_raw_os_error(e as i32)),
        }
    }

    #[cfg(windows)]
    fn is_valid_executable(&self, path: &std::path::Path) -> std::io::Result<bool> {
        use std::os::windows::ffi::OsStrExt;

        let name = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>();
        let mut bt: u32 = 0;
        // SAFETY: winapi call
        unsafe {
            Ok(windows_sys::Win32::Storage::FileSystem::GetBinaryTypeW(name.as_ptr(), &mut bt) != 0)
        }
    }
}
