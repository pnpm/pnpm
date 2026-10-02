use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};
use which::{Result, WhichConfig, sys::Sys};

pub fn which<Name: AsRef<OsStr>>(name: Name) -> Result<PathBuf> {
    config(name.as_ref(), std::env::var_os("PATH")).first_result()
}

pub fn which_all<Name: AsRef<OsStr>>(name: Name) -> Result<impl Iterator<Item = PathBuf>> {
    config(name.as_ref(), std::env::var_os("PATH")).all_results()
}

pub fn which_in<Name, Paths, Directory>(
    name: Name,
    paths: Option<Paths>,
    cwd: Directory,
) -> Result<PathBuf>
where
    Name: AsRef<OsStr>,
    Paths: AsRef<OsStr>,
    Directory: AsRef<Path>,
{
    config(name.as_ref(), paths.map(|paths| paths.as_ref().to_owned()))
        .custom_cwd(cwd.as_ref().to_owned())
        .first_result()
}

pub fn which_in_global<Name: AsRef<OsStr>, Paths: AsRef<OsStr>>(
    name: Name,
    paths: Option<Paths>,
) -> Result<impl Iterator<Item = PathBuf>> {
    config(name.as_ref(), paths.map(|paths| paths.as_ref().to_owned()))
        .system_cwd(false)
        .all_results()
}

fn config(name: &OsStr, path: Option<OsString>) -> WhichConfig<HostSys> {
    WhichConfig::new_with_sys(HostSys { path }).binary_name(name.to_owned())
}

struct HostSys {
    path: Option<OsString>,
}

impl Sys for HostSys {
    type ReadDirEntry = fs::DirEntry;
    type Metadata = fs::Metadata;

    fn is_windows(&self) -> bool {
        false
    }
    fn current_dir(&self) -> io::Result<PathBuf> {
        std::env::current_dir()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        pnpm_fs::home_dir()
    }
    fn env_split_paths(&self, paths: &OsStr) -> Vec<PathBuf> {
        pnpm_fs::split_paths(paths).collect()
    }
    fn env_path(&self) -> Option<OsString> {
        self.path.clone()
    }
    fn env_path_ext(&self) -> Option<OsString> {
        None
    }
    fn metadata(&self, path: &Path) -> io::Result<Self::Metadata> {
        fs::metadata(path)
    }
    fn symlink_metadata(&self, path: &Path) -> io::Result<Self::Metadata> {
        fs::symlink_metadata(path)
    }
    fn read_dir(
        &self,
        path: &Path,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<Self::ReadDirEntry>>>> {
        fs::read_dir(path).map(|entries| Box::new(entries) as _)
    }
    fn is_valid_executable(&self, path: &Path) -> io::Result<bool> {
        pnpm_fs::executable_access(path).map(|()| true)
    }
}
