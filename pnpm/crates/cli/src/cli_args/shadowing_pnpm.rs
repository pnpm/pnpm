//! The `pnpm` a `PATH` lookup finds when it is not the one in the global bin
//! directory.
//!
//! `self-update` and `setup` install pnpm into the global bin directory. A
//! `pnpm` that another installer put ahead of that directory on `PATH` (npm,
//! Homebrew, Corepack, Volta, Scoop) keeps running after either command
//! reports success, so every version they install looks like it never took.
//! Both commands, and `doctor`, name that `pnpm` and the command that removes
//! it.

use super::self_update::homebrew_formula;
use pnpm_config::{
    Host,
    default_pnpm_home_dir,
};
use pnpm_reporter::{
    LogEvent,
    LogLevel,
    PnpmLog,
    Reporter,
};
use std::{
    ffi::OsStr,
    fs,
    path::{
        Path,
        PathBuf,
    },
};

/// Shim scripts (npm's `.cmd` files, Corepack's) name their target inside;
/// anything larger than this is a real executable, not a script.
const SHIM_SCRIPT_MAX_SIZE: u64 = 64 * 1024;

/// How a `pnpm` outside the global bin directory got onto `PATH`, as far as
/// its location and, for a shim script, its contents tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstallOrigin {
    /// `npm install -g pnpm`: the executable lives in a `node_modules/pnpm`
    /// package.
    NpmGlobal,
    /// A Homebrew formula (`pnpm`, `pnpm@11`, ...) whose keg holds the
    /// executable.
    Homebrew {
        formula: String,
    },
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
        if let Some(formula) = homebrew_formula(&real) {
            return Self::Homebrew { formula };
        }
        origin_from_path(&real)
            .or_else(|| origin_from_path(executable))
            .or_else(|| npm_global_package(&real))
            .or_else(|| origin_from_shim_script(executable))
            .unwrap_or(Self::Unknown)
    }

    /// The command that removes that installation, when the origin names one.
    fn removal_command(&self) -> Option<String> {
        let command = match self {
            Self::NpmGlobal => "npm uninstall -g pnpm",
            Self::Homebrew { formula } => return Some(format!("brew uninstall {formula}")),
            Self::Corepack => "corepack disable pnpm",
            Self::Volta => "volta uninstall pnpm",
            Self::Scoop => "scoop uninstall pnpm",
            Self::Unknown => return None,
        };
        Some(command.to_string())
    }

    fn description(&self) -> &'static str {
        match self {
            Self::NpmGlobal => "installed with npm",
            Self::Homebrew { .. } => "installed with Homebrew",
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
    let has = |name: &str| {
        components
            .iter()
            .any(|component| component == name)
    };
    if has("corepack") {
        return Some(InstallOrigin::Corepack);
    }
    if has(".volta") {
        return Some(InstallOrigin::Volta);
    }
    if has("scoop") {
        return Some(InstallOrigin::Scoop);
    }
    None
}

/// `NpmGlobal` when `path` lies in a `node_modules/pnpm` package outside
/// any project. A project's own `node_modules`, which `node_modules/.bin`
/// puts on `PATH` in scripts, sits next to a `package.json`; npm's global
/// prefixes (`lib/` on POSIX, `%APPDATA%\npm` on Windows) do not. A
/// package in a pnpm virtual store (`.pnpm`) was installed by pnpm, not npm.
fn npm_global_package(path: &Path) -> Option<InstallOrigin> {
    if path
        .components()
        .any(|component| component.as_os_str() == ".pnpm")
    {
        return None;
    }
    let node_modules = path
        .ancestors()
        .filter(|dir| dir.file_name() == Some(OsStr::new("pnpm")))
        .filter_map(Path::parent)
        .find(|dir| dir.file_name() == Some(OsStr::new("node_modules")))?;
    let prefix = node_modules.parent()?;
    (!prefix.join("package.json").exists()).then_some(InstallOrigin::NpmGlobal)
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
    let in_project_bin = executable
        .parent()
        .is_some_and(|dir| dir.ends_with(Path::new("node_modules").join(".bin")));
    if !in_project_bin
        && (script.contains("node_modules/pnpm/") || script.contains(r"node_modules\pnpm\"))
    {
        return Some(InstallOrigin::NpmGlobal);
    }
    None
}

/// What `pnpm setup` does to the user's `PATH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SetupPathEntry {
    /// Writes the global bin directory first on `PATH`.
    Prepended,
    /// Prints the settings for the user to add, under `--no-modify-path` or
    /// when the shell configuration cannot be written.
    LeftAlone,
}

/// A `pnpm` that a `PATH` lookup finds ahead of the one in the global bin
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShadowingPnpm {
    pub(crate) executable: PathBuf,
    pub(crate) origin: InstallOrigin,
    /// Whether the global bin directory is on `PATH` at all, behind
    /// `executable`. When it is not, `PATH` has yet to be set up, and the
    /// right pnpm runs once the global bin directory is added ahead of
    /// `executable`.
    pub(crate) global_bin_on_path: bool,
}

impl ShadowingPnpm {
    /// The warning to print once a pnpm is linked into `global_bin` and this
    /// one still answers to `pnpm`.
    /// Whether the warning still applies after `pnpm setup` writes its PATH
    /// entry. Setup puts a global bin directory missing from `PATH` first,
    /// which is the fix the warning would ask for, unless it leaves `PATH`
    /// alone.
    pub(crate) fn outlives_setup(&self, path_entry: SetupPathEntry) -> bool {
        self.global_bin_on_path || path_entry == SetupPathEntry::LeftAlone
    }

    pub(crate) fn emit_warning<Logger: Reporter>(&self, global_bin: &Path, prefix: &str) {
        Logger::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: self.warning(global_bin),
            prefix: prefix.to_string(),
        }));
    }

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
                 When you add it, put it first: {fix}.",
            )
        }
    }
}

/// Warn about a `pnpm` ahead of `global_bin` that `pnpm setup` leaves in
/// place, given what it did to `PATH`.
pub(crate) fn warn_if_shadowed_after_setup<Logger: Reporter>(
    global_bin: &Path,
    path_entry: SetupPathEntry,
    prefix: &str,
) {
    if let Some(shadowing) = find_shadowing_pnpm_on_path(global_bin)
        && shadowing.outlives_setup(path_entry)
    {
        shadowing.emit_warning::<Logger>(global_bin, prefix);
    }
}

/// Where pnpm links its own `pnpm`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OwnDirs<'a> {
    pub(crate) global_bin: &'a Path,
    /// The pnpm home directory, which the v10 layout and CI actions link
    /// into directly.
    pub(crate) pnpm_home: Option<&'a Path>,
}

/// The `pnpm` the current `PATH` finds ahead of the one in `global_bin`, if
/// any.
pub(crate) fn find_shadowing_pnpm_on_path(global_bin: &Path) -> Option<ShadowingPnpm> {
    let pnpm_home = default_pnpm_home_dir::<Host>();
    let own = OwnDirs { global_bin, pnpm_home: pnpm_home.as_deref() };
    find_shadowing_pnpm(own, std::env::var_os("PATH").as_deref())
}

/// The `pnpm` that a lookup through `path_env` finds ahead of the one in
/// `own.global_bin`, if any.
///
/// A `pnpm` in one of `own`'s directories, or a symlink resolving into
/// one, is pnpm's own and never shadows.
pub(crate) fn find_shadowing_pnpm(
    own: OwnDirs<'_>,
    path_env: Option<&OsStr>,
) -> Option<ShadowingPnpm> {
    let path_env = path_env?;
    let executable = which::which_in_global("pnpm", Some(path_env)).ok()?.next()?;
    if is_own_pnpm(&executable, own) {
        return None;
    }
    let global_bin_on_path =
        pnpm_fs::split_paths(path_env).any(|dir| same_dir(&dir, own.global_bin));
    Some(ShadowingPnpm {
        origin: InstallOrigin::detect(&executable),
        executable,
        global_bin_on_path,
    })
}

fn is_own_pnpm(executable: &Path, own: OwnDirs<'_>) -> bool {
    let own_dirs = [Some(own.global_bin), own.pnpm_home];
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
