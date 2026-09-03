//! The on-disk shape of a context-aware global shim.
//!
//! A shim is the pnpm executable published under the bin's own name —
//! `<bin dir>/<name>` (`<name>.exe` on Windows), a hard link or copy of the
//! executable that wrote it — with the target it stands for recorded in
//! the sidecar `<bin dir>/.pnpm-shim-v1-<name>-target`. A launch under a
//! bin name reads the sidecar and dispatches; there is no shell between
//! the caller and the target, so the target inherits the caller's
//! environment untouched. The sidecar holds the raw target path (UTF-16 on
//! Windows), or `pkg:<package>` for a shim with nothing installed behind it.
//!
//! A legacy shim is a shell script carrying the marker line
//! `# pnpm-shim-style=context-aware` and a `# cmd-shim-target=` trailer,
//! dispatching through a `.pnpm-shim-v1` executable with
//! `--shim <name> <shim> <target> -- <args...>`. An executable installed in
//! that dispatcher slot continues to serve the protocol and migrates the bin
//! directory before dispatching the target.

use super::{dispatch_target, trusted_shim_settings};
use crate::cli_args::global_bin_lock::try_acquire_global_bin_lock;
use miette::{Context as _, IntoDiagnostic as _};
use pnpm_cmd_shim::is_safe_bin_name;
use pnpm_resolving_parse_wanted_dependency::is_valid_old_npm_package_name;
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

const TARGET_FILE_PREFIX: &str = ".pnpm-shim-v1-";
const TARGET_FILE_SUFFIX: &str = "-target";
const VIRTUAL_TARGET_PREFIX: &str = "pkg:";
const LEGACY_DISPATCHER_NAME: &str = ".pnpm-shim-v1";
const LEGACY_CONTEXT_AWARE_MARKER: &str = "# pnpm-shim-style=context-aware";
const LEGACY_TARGET_MARKER: &str = "# cmd-shim-target=";
const MAX_LEGACY_SHIM_BYTES: u64 = 64 * 1024;

/// What a shim runs when the project it is launched in provides nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShimTarget {
    /// The bin file of the globally installed package.
    Installed(PathBuf),
    /// Nothing is installed globally; the shim exists for `package` so a
    /// project pinning or depending on it decides what runs.
    Virtual(String),
}

impl ShimTarget {
    pub(crate) fn virtual_package(&self) -> Option<&str> {
        match self {
            ShimTarget::Virtual(package) => Some(package),
            ShimTarget::Installed(_) => None,
        }
    }

    fn encode(&self) -> Vec<u8> {
        match self {
            ShimTarget::Installed(path) => encode_os(path.as_os_str()),
            ShimTarget::Virtual(package) => {
                encode_os(OsStr::new(&format!("{VIRTUAL_TARGET_PREFIX}{package}")))
            }
        }
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let raw = decode_os(bytes)?;
        if let Some(package) = raw.to_str().and_then(|raw| raw.strip_prefix(VIRTUAL_TARGET_PREFIX))
        {
            return is_valid_old_npm_package_name(package)
                .then(|| ShimTarget::Virtual(package.to_string()));
        }
        Some(ShimTarget::Installed(PathBuf::from(raw)))
    }

    /// The target a legacy shell shim recorded in its trailer and passes
    /// in its `--shim` invocation.
    fn from_legacy_marker(value: &str) -> Option<Self> {
        if let Some(package) = value.strip_prefix(VIRTUAL_TARGET_PREFIX) {
            return is_valid_old_npm_package_name(package)
                .then(|| ShimTarget::Virtual(package.to_string()));
        }
        Some(ShimTarget::Installed(PathBuf::from(value)))
    }
}

pub(crate) fn install_native_shim(
    bin_dir: &Path,
    name: &str,
    target: &ShimTarget,
) -> io::Result<()> {
    install_native_shim_from(&std::env::current_exe()?, bin_dir, name, target)
}

/// Publish `source` as the shim `name`, recording `target` beside it. The
/// sidecar lands first so a shim is never launched without its target;
/// the executable is swapped in atomically over whatever held the slot.
pub(crate) fn install_native_shim_from(
    source: &Path,
    bin_dir: &Path,
    name: &str,
    target: &ShimTarget,
) -> io::Result<()> {
    fs::create_dir_all(bin_dir)?;
    // A legacy shim, or a direct shim being turned context-aware, may hold
    // the slot in the text flavors. On Unix the executable replaces the
    // single flavor in place.
    if cfg!(windows) {
        for flavor in ["", ".cmd", ".ps1"] {
            remove_if_exists(&bin_dir.join(format!("{name}{flavor}")))?;
        }
    }
    let target_file = target_file_path(bin_dir, name);
    let executable = executable_path(bin_dir, name);
    pnpm_fs::write_atomic(&target_file, &target.encode())?;
    crate::executable_link::replace_executable(source, &executable).inspect_err(|_| {
        // A sidecar without an executable would list as a shim; a sidecar
        // beside an older executable is a live shim with its new target.
        if !executable.exists() {
            let _ = fs::remove_file(&target_file);
        }
    })
}

/// Remove the shim `name` and its sidecar. A missing shim is not an error.
pub(crate) fn remove_native_shim(bin_dir: &Path, name: &str) -> io::Result<()> {
    remove_if_exists(&executable_path(bin_dir, name))?;
    remove_if_exists(&target_file_path(bin_dir, name))
}

/// The recorded target of the shim `name`, `None` when no shim is
/// installed under that name.
pub(crate) fn native_shim_target(bin_dir: &Path, name: &str) -> io::Result<Option<ShimTarget>> {
    let target_file = target_file_path(bin_dir, name);
    let bytes = match fs::read(&target_file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    ShimTarget::decode(&bytes).map(Some).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} does not hold a shim target", target_file.display()),
        )
    })
}

pub(crate) fn native_shim_is_installed(bin_dir: &Path, name: &str) -> bool {
    target_file_path(bin_dir, name).is_file() && executable_path(bin_dir, name).is_file()
}

/// The names of every shim in `bin_dir`, in name order.
pub(crate) fn native_shims(bin_dir: &Path) -> io::Result<Vec<String>> {
    let entries = match fs::read_dir(bin_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut names = Vec::new();
    for entry in entries {
        let file_name = entry?.file_name();
        let Some(name) = file_name
            .to_str()
            .and_then(|file_name| file_name.strip_prefix(TARGET_FILE_PREFIX))
            .and_then(|rest| rest.strip_suffix(TARGET_FILE_SUFFIX))
        else {
            continue;
        };
        if is_safe_bin_name(name) {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

/// The files a shim `name` occupies: its executable and its sidecar.
pub(crate) fn native_shim_paths(bin_dir: &Path, name: &str) -> [PathBuf; 2] {
    [executable_path(bin_dir, name), target_file_path(bin_dir, name)]
}

/// Republish every shim in `bin_dir` from `source`, migrating legacy
/// shims first. Self-update uses this so the shims carry the newly
/// installed engine; a bin dir without shims stays without them.
pub(crate) fn refresh_native_shims(source: &Path, bin_dir: &Path) -> io::Result<()> {
    migrate_legacy_shims_from(source, bin_dir)?;
    for name in native_shims(bin_dir)? {
        crate::executable_link::replace_executable(source, &executable_path(bin_dir, &name))?;
    }
    Ok(())
}

pub(crate) fn migrate_legacy_shims(bin_dir: &Path) -> io::Result<()> {
    migrate_legacy_shims_from(&std::env::current_exe()?, bin_dir)
}

/// Turn every legacy shim into a native shim, then drop the
/// `.pnpm-shim-v1` executable the legacy shims dispatch through.
pub(crate) fn migrate_legacy_shims_from(source: &Path, bin_dir: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(bin_dir) {
        Ok(entries) => entries.collect::<io::Result<Vec<_>>>()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str().filter(|name| is_safe_bin_name(name)) else {
            continue;
        };
        // Migrating one shim removes its Windows siblings, which the
        // listing may still name.
        let Some(target) = legacy_shim_target(&entry.path())? else {
            continue;
        };
        install_native_shim_from(source, bin_dir, name, &target)?;
    }
    let dispatcher =
        bin_dir.join(format!("{LEGACY_DISPATCHER_NAME}{}", std::env::consts::EXE_SUFFIX));
    // The dispatcher may still be executing on behalf of a shim launched
    // before the migration, which Windows reports as a sharing violation;
    // the next migration pass removes it.
    let _ = fs::remove_file(dispatcher);
    Ok(())
}

pub(crate) fn is_legacy_context_aware_shim(path: &Path) -> bool {
    legacy_shim_target(path).is_ok_and(|target| target.is_some())
}

/// The target of the legacy shim at `path`, `None` for anything that is
/// not one. Only the shell flavor carries the markers; the
/// Windows `.cmd` and `.ps1` siblings are removed with it.
fn legacy_shim_target(path: &Path) -> io::Result<Option<ShimTarget>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    // A shell shim is a few hundred bytes; the native shims and other
    // executables in the bin dir are ruled out by size without a read.
    if !metadata.is_file() || metadata.len() > MAX_LEGACY_SHIM_BYTES {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(MAX_LEGACY_SHIM_BYTES).read_to_end(&mut bytes)?;
    if !bytes.starts_with(b"#!") {
        return Ok(None);
    }
    let body = String::from_utf8_lossy(&bytes);
    if !body.lines().any(|line| line == LEGACY_CONTEXT_AWARE_MARKER) {
        return Ok(None);
    }
    let target = body
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(LEGACY_TARGET_MARKER))
        .and_then(ShimTarget::from_legacy_marker);
    Ok(target)
}

/// Serve a legacy shim's `--shim` invocation. A valid invocation identifies a
/// shim beside the executing dispatcher. Migration is best-effort and does not
/// prevent dispatch.
pub(super) fn dispatch_legacy_shim(rest: &[OsString]) -> i32 {
    let Some((name, shim, target, args)) = parse_legacy_shim_argv(rest) else {
        report_shim_error(&miette::miette!(
            "malformed --shim invocation. Usage: pnpm --shim <name> <shim> <target> -- [args...]",
        ));
        return 1;
    };
    let Some(bin_dir) = executing_dispatcher_bin_dir(shim) else {
        let shim_display = shim.display();
        report_shim_error(&miette::miette!(
            "the legacy shim path {} is not beside the executing dispatcher",
            shim_display,
        ));
        return 1;
    };
    try_migrate_legacy_shims(&bin_dir);
    let settings = trusted_shim_settings();
    let invocation = super::ShimInvocation { name, bin_dir: &bin_dir, target: &target };
    dispatch_target(&invocation, args, &settings.shims, &settings.state_dir)
}

fn executing_dispatcher_bin_dir(shim: &Path) -> Option<PathBuf> {
    let supplied_bin_dir = shim.parent().filter(|dir| !dir.as_os_str().is_empty())?;
    let dispatcher = std::env::current_exe().ok()?;
    let dispatcher_name = format!("{LEGACY_DISPATCHER_NAME}{}", std::env::consts::EXE_SUFFIX);
    if dispatcher.file_name() != Some(OsStr::new(&dispatcher_name)) {
        return None;
    }
    let bin_dir = dispatcher.parent()?.to_path_buf();
    same_file::is_same_file(supplied_bin_dir, &bin_dir).unwrap_or(false).then_some(bin_dir)
}

fn try_migrate_legacy_shims(bin_dir: &Path) {
    let lock = match try_acquire_global_bin_lock(bin_dir) {
        Ok(Some(lock)) => lock,
        Ok(None) => return,
        Err(error) => {
            report_shim_error(&error);
            return;
        }
    };
    if let Err(error) = migrate_legacy_shims(bin_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("cannot migrate the global shims in {}", bin_dir.display()))
    {
        report_shim_error(&error);
    }
    drop(lock);
}

fn report_shim_error(error: &miette::Report) {
    eprintln!("pnpm: {error:?}");
}

/// Split the machine-generated tail of a `--shim` invocation into the bin
/// name, the shim's own path, its global target, and the forwarded
/// arguments. The target slot carries what the shim's trailer would:
/// a path, or `pkg:<package>` for a target-less shim.
fn parse_legacy_shim_argv(rest: &[OsString]) -> Option<(&str, &Path, ShimTarget, &[OsString])> {
    let [name, shim, target, separator, args @ ..] = rest else {
        return None;
    };
    if separator.to_str() != Some("--") {
        return None;
    }
    let name = name.to_str().filter(|name| is_safe_bin_name(name))?;
    let target = match target.to_str() {
        Some(target) => ShimTarget::from_legacy_marker(target)?,
        None => ShimTarget::Installed(PathBuf::from(target)),
    };
    Some((name, Path::new(shim), target, args))
}

/// Intercept a launch under a shim name. `None` means this is pnpm
/// itself and the regular CLI should proceed.
pub(super) fn try_native_dispatch(argv: &[OsString]) -> Option<i32> {
    let executable = std::env::current_exe().ok()?;
    let name = shim_name(executable.file_name()?)?;
    let bin_dir = executable.parent()?;
    let target = match native_shim_target(bin_dir, &name) {
        Ok(Some(target)) => target,
        Ok(None) => return None,
        Err(error) => {
            eprintln!("pnpm: cannot read the global target of the {name} shim: {error}");
            return Some(1);
        }
    };
    if let ShimTarget::Installed(path) = &target
        && same_file::is_same_file(path, &executable).unwrap_or(false)
    {
        eprintln!("pnpm: the global target of the {name} shim points back at the shim");
        return Some(1);
    }
    let settings = trusted_shim_settings();
    let invocation = super::ShimInvocation { name: &name, bin_dir, target: &target };
    let args = argv.get(1..).unwrap_or_default();
    Some(dispatch_target(&invocation, args, &settings.shims, &settings.state_dir))
}

fn executable_path(bin_dir: &Path, name: &str) -> PathBuf {
    bin_dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn target_file_path(bin_dir: &Path, name: &str) -> PathBuf {
    bin_dir.join(format!("{TARGET_FILE_PREFIX}{name}{TARGET_FILE_SUFFIX}"))
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn shim_name(file_name: &OsStr) -> Option<String> {
    let file_name = file_name.to_str()?;
    let name = file_name.get(..file_name.len().checked_sub(4)?)?;
    file_name[name.len()..].eq_ignore_ascii_case(".exe").then(|| name.to_string())
}

#[cfg(not(windows))]
fn shim_name(file_name: &OsStr) -> Option<String> {
    Some(file_name.to_str()?.to_string())
}

#[cfg(unix)]
fn encode_os(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    value.as_bytes().to_vec()
}

#[cfg(unix)]
fn decode_os(bytes: &[u8]) -> Option<OsString> {
    use std::os::unix::ffi::OsStringExt as _;
    Some(OsString::from_vec(bytes.to_vec()))
}

#[cfg(windows)]
fn encode_os(value: &OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;
    value.encode_wide().flat_map(u16::to_le_bytes).collect()
}

#[cfg(windows)]
fn decode_os(bytes: &[u8]) -> Option<OsString> {
    use std::os::windows::ffi::OsStringExt as _;
    let mut chunks = bytes.chunks_exact(2);
    let value = OsString::from_wide(
        &chunks.by_ref().map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]])).collect::<Vec<_>>(),
    );
    chunks.remainder().is_empty().then_some(value)
}
