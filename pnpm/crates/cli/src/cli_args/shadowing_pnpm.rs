//! The `pnpm` a `PATH` lookup finds when it is not the one in the global bin
//! directory.
//!
//! `self-update` and `setup` install pnpm into the global bin directory. A
//! `pnpm` that another installer put ahead of that directory on `PATH` (npm,
//! Homebrew, Corepack, Volta, Scoop) keeps running after either command
//! reports success, so every version they install looks like it never took.
//! Both commands, and `doctor`, name that `pnpm` and the command that removes
//! it.

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

/// Shim scripts (npm's `.cmd` files, Corepack's) name their target inside;
/// anything larger than this is a real executable, not a script.
const SHIM_SCRIPT_MAX_SIZE: u64 = 64 * 1024;

/// How a `pnpm` outside the global bin directory got onto `PATH`, as far as
/// its location and, for a shim script, its contents tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallOrigin {
    /// `npm install -g pnpm`: the executable lives in a `node_modules/pnpm`
    /// package.
    NpmGlobal,
    /// The Homebrew formula, under a `Cellar/pnpm` keg.
    Homebrew,
    /// A Corepack shim.
    Corepack,
    /// A Volta shim.
    Volta,
    /// A Scoop shim.
    Scoop,
    Unknown,
}

impl InstallOrigin {
    pub(crate) fn detect(executable: &Path) -> Self {
        let real = fs::canonicalize(executable).unwrap_or_else(|_| executable.to_path_buf());
        origin_from_path(&real)
            .or_else(|| origin_from_path(executable))
            .or_else(|| origin_from_shim_script(executable))
            .unwrap_or(Self::Unknown)
    }

    /// The command that removes that installation, when the origin names one.
    pub(crate) fn removal_command(self) -> Option<&'static str> {
        match self {
            Self::NpmGlobal => Some("npm uninstall -g pnpm"),
            Self::Homebrew => Some("brew uninstall pnpm"),
            Self::Corepack => Some("corepack disable pnpm"),
            Self::Volta => Some("volta uninstall pnpm"),
            Self::Scoop => Some("scoop uninstall pnpm"),
            Self::Unknown => None,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::NpmGlobal => "installed with npm",
            Self::Homebrew => "installed with Homebrew",
            Self::Corepack => "a Corepack shim",
            Self::Volta => "installed with Volta",
            Self::Scoop => "installed with Scoop",
            Self::Unknown => "not installed by pnpm",
        }
    }
}

fn origin_from_path(path: &Path) -> Option<InstallOrigin> {
    let components: Vec<String> = path
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .to_ascii_lowercase()
        })
        .collect();
    let follows = |first: &str, second: &str| {
        components
            .windows(2)
            .any(|pair| pair[0] == first && pair[1] == second)
    };
    let has = |name: &str| {
        components
            .iter()
            .any(|component| component == name)
    };
    if follows("cellar", "pnpm") {
        return Some(InstallOrigin::Homebrew);
    }
    if has("corepack") {
        return Some(InstallOrigin::Corepack);
    }
    if has(".volta") {
        return Some(InstallOrigin::Volta);
    }
    if has("scoop") {
        return Some(InstallOrigin::Scoop);
    }
    if follows("node_modules", "pnpm") {
        return Some(InstallOrigin::NpmGlobal);
    }
    None
}

fn origin_from_shim_script(executable: &Path) -> Option<InstallOrigin> {
    let size = fs::metadata(executable).ok()?.len();
    if size > SHIM_SCRIPT_MAX_SIZE {
        return None;
    }
    let script = fs::read_to_string(executable).ok()?;
    if script.contains("corepack") {
        return Some(InstallOrigin::Corepack);
    }
    if script.contains("node_modules/pnpm/") || script.contains(r"node_modules\pnpm\") {
        return Some(InstallOrigin::NpmGlobal);
    }
    None
}

/// A `pnpm` that a `PATH` lookup finds ahead of the one in the global bin
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShadowingPnpm {
    pub(crate) executable: PathBuf,
    pub(crate) origin: InstallOrigin,
    /// Whether the global bin directory is on `PATH` at all, behind
    /// `executable`. When it is not, `PATH` has yet to be set up, and the
    /// shell will find the right pnpm once the global bin directory is
    /// added ahead of `executable`.
    pub(crate) global_bin_on_path: bool,
}

impl ShadowingPnpm {
    /// The warning to print once a pnpm is linked into `global_bin` and this
    /// one still answers to `pnpm`.
    pub(crate) fn warning(&self, global_bin: &Path) -> String {
        let executable = self.executable.display();
        let global_bin = global_bin.display();
        let origin = self.origin.description();
        let reorder = match self.executable.parent() {
            Some(dir) => format!("move {global_bin} ahead of {} in PATH", dir.display()),
            None => format!("move {global_bin} ahead of it in PATH"),
        };
        let fix = match self.origin.removal_command() {
            Some(command) => format!(r#"run "{command}" or {reorder}"#),
            None => reorder,
        };
        if self.global_bin_on_path {
            format!(
                "\"pnpm\" on PATH is {executable} ({origin}), which comes before {global_bin}. \
                 Your shell keeps running that pnpm, not the one pnpm installed to {global_bin}. \
                 To finish switching, {fix}.",
            )
        } else {
            format!(
                "\"pnpm\" on PATH is {executable} ({origin}), and {global_bin} is not on PATH yet. \
                 Once a new shell adds it, it has to come first: {fix}.",
            )
        }
    }
}

/// The `pnpm` that a lookup through `path_env` finds ahead of the one in
/// `global_bin`, if any.
///
/// A `pnpm` in `global_bin` itself, in its parent directory (the pnpm home
/// directory, which older layouts and CI actions link into directly), or a
/// symlink resolving into either, is pnpm's own and never shadows.
pub(crate) fn find_shadowing_pnpm(
    global_bin: &Path,
    path_env: Option<&OsStr>,
) -> Option<ShadowingPnpm> {
    let path_env = path_env?;
    // A bare name is never resolved against the working directory, which
    // only matters for a name with a separator in it.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut candidates = which::which_in_all("pnpm", Some(path_env), cwd).ok()?;
    let executable = candidates.next()?;
    if is_own_pnpm(&executable, global_bin) {
        return None;
    }
    let global_bin_on_path = candidates.any(|candidate| is_own_pnpm(&candidate, global_bin));
    Some(ShadowingPnpm {
        origin: InstallOrigin::detect(&executable),
        executable,
        global_bin_on_path,
    })
}

fn is_own_pnpm(executable: &Path, global_bin: &Path) -> bool {
    let own_dirs = [Some(global_bin), global_bin.parent()];
    let real_executable = fs::canonicalize(executable).ok();
    [Some(executable), real_executable.as_deref()]
        .into_iter()
        .flatten()
        .filter_map(Path::parent)
        .any(|dir| {
            own_dirs
                .into_iter()
                .flatten()
                .any(|own| same_dir(dir, own))
        })
}

fn same_dir(left: &Path, right: &Path) -> bool {
    left == right || same_file::is_same_file(left, right).unwrap_or(false)
}

#[cfg(test)]
mod tests;
