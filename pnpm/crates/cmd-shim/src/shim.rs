pub use pnpm_package_manifest::{
    ScriptRuntime, extension_program, parse_shebang, parse_shebang_from_bytes,
};
pub use powershell::generate_pwsh_shim;
pub use quoting::{cmd_escape, sh_single_quote};
pub(crate) use relocatable::{is_relocatable_shim, is_within_root};
pub use replaced_batch::end_replaced_cmd_shim_batch;
pub use sh::{
    generate_sh_shim, is_sh_shim_basedir_anchor_current, is_sh_shim_hardened, is_shim_pointing_at,
};
#[cfg(target_family = "wasm")]
pub(crate) use wasm::generate_wasm_shim;
#[cfg(any(all(test, unix), target_family = "wasm"))]
mod wasm;

use crate::{capabilities::FsReadHead, path_util::lexical_normalize};
use pnpm_package_manifest::infer_script_runtime_from_shebang;
use std::{
    fmt::Write as _,
    io,
    path::{Path, PathBuf},
};

/// Read up to 512 bytes of `path` and infer the runtime.
///
/// `NotFound` reading the file degrades to `Ok(None)` so a missing-bin race
/// doesn't fail the whole install. Other IO errors propagate, since pacquet
/// has already verified the bin path resolves under the package root by
/// this point and a real failure deserves to surface.
pub fn search_script_runtime<Sys: FsReadHead>(path: &Path) -> io::Result<Option<ScriptRuntime>> {
    let runtime_from_shebang = read_shebang::<Sys>(path)?;
    Ok(infer_script_runtime_from_shebang(path, runtime_from_shebang))
}

fn read_shebang<Sys: FsReadHead>(path: &Path) -> io::Result<Option<ScriptRuntime>> {
    let mut buffer = [0u8; 512];
    let read = match read_head_filled::<Sys>(path, &mut buffer) {
        Ok(read) => read,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(parse_shebang_from_bytes(&buffer[..read]))
}

/// Read up to `buf.len()` bytes from `path` into `buf`, looping over
/// the [`FsReadHead`] capability until either the buffer is full or
/// the underlying read returns 0 (EOF). Returns the number of bytes
/// actually filled (which can be `< buf.len()` for a short file).
///
/// [`FsReadHead::read_head`] mirrors a single `read(2)` syscall, which
/// POSIX permits to return short. This loop collects short reads so
/// the shebang parser sees a complete view of the head of the file
/// even on pseudo-fs paths (`/proc`, `/sys`, FUSE, ...) where short
/// reads are common. On regular files at offset 0 the underlying
/// `read` returns the whole prefix in one syscall, so the loop adds
/// no extra syscalls in the hot path. The cost is one extra branch.
pub fn read_head_filled<Sys: FsReadHead>(path: &Path, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match Sys::read_head(path, total as u64, &mut buf[total..])? {
            0 => break, // EOF
            n => total += n,
        }
    }
    Ok(total)
}

/// Render `node_path` entries into the platform variants cmd-shim's
/// `normalizePathEnvVar` produces: `win32` joins with `;` and
/// backslashes, `posix` joins with `:` and forward slashes. When the shim
/// is generated on Windows (`windows_host`), the posix form additionally
/// maps a drive prefix to WSL's mount (`C:` → `/mnt/c`). Shells under
/// Cygwin and MSYS read the `win32` form instead, which the shim picks at
/// run time, so the rendering doesn't depend on the installing shell. On
/// Unix the entries pass through unchanged.
struct NodePathEnvVar {
    win32: String,
    posix: String,
}

fn normalize_node_path_env_var(node_path: &[String], windows_host: bool) -> NodePathEnvVar {
    let mut win32 = String::new();
    let mut posix = String::new();
    for entry in node_path {
        let entry_win32 = entry.replace('/', r"\");
        let entry_posix = if windows_host { windows_entry_to_posix(entry) } else { entry.clone() };
        if !win32.is_empty() {
            win32.push(';');
        }
        win32.push_str(&entry_win32);
        if !posix.is_empty() {
            posix.push(':');
        }
        posix.push_str(&entry_posix);
    }
    NodePathEnvVar { win32, posix }
}

/// cmd-shim's Windows-host posix rendering: flip backslashes and map a
/// leading drive letter under WSL's `/mnt`.
fn windows_entry_to_posix(entry: &str) -> String {
    let flipped = entry.replace('\\', "/");
    let Some((drive, rest)) = flipped.split_once(':') else {
        return flipped;
    };
    if drive.is_empty() || drive.contains('/') {
        return flipped;
    }
    format!("/mnt/{}{rest}", drive.to_lowercase())
}

/// Generate the Windows `.cmd` shim contents for `target_path`. Pacquet
/// skips the `prependToPath`/`nodeExecPath`/`progArgs` features; only
/// `nodePath` (the `NODE_PATH` block) is supported beyond the "plain"
/// cmd shim.
///
/// CRLF line endings are part of the on-disk contract for `.cmd` files
/// on Windows, so the template uses literal `\r\n`.
#[must_use]
pub fn generate_cmd_shim(
    target_path: &Path,
    shim_path: &Path,
    runtime: Option<&ScriptRuntime>,
    node_path: &[String],
    batch: CmdShimBatch,
) -> String {
    let cmd_target_rel = cmd_escape(&relative_target_windows(target_path, shim_path));
    let quoted_target = if Path::new(&cmd_target_rel).is_absolute() {
        format!(r#""{cmd_target_rel}""#)
    } else {
        format!(r#""%~dp0\{cmd_target_rel}""#)
    };

    let runs_target_directly = runtime.is_none_or(|runtime| runtime.prog.is_none());
    if batch == CmdShimBatch::EndedBeforeTarget && runs_target_directly {
        let args = runtime.map_or(String::new(), |runtime| cmd_escape(&runtime.args));
        return generate_batchless_cmd_shim(&quoted_target, &args);
    }

    let mut cmd = String::from("@SETLOCAL\r\n");

    let cmd_node_path = cmd_escape(&normalize_node_path_env_var(node_path, cfg!(windows)).win32);
    if !cmd_node_path.is_empty() {
        write!(
            cmd,
            "@IF NOT DEFINED NODE_PATH (\r\n  @SET \"NODE_PATH={cmd_node_path}\"\r\n) ELSE (\r\n  @SET \"NODE_PATH={cmd_node_path};%NODE_PATH%\"\r\n)\r\n",
        )
        .unwrap();
    }

    match runtime {
        Some(ScriptRuntime { prog: Some(prog), args }) => {
            let prog = cmd_escape(prog);
            let args = cmd_escape(args);
            let long_prog = format!(r#""%~dp0\{prog}.exe""#);
            writeln!(
                cmd,
                "@IF EXIST {long_prog} (\r\n  {long_prog} {args} {quoted_target} %*\r\n) ELSE (\r\n  @SET PATHEXT=%PATHEXT:;.JS;=;%\r\n  {prog} {args} {quoted_target} %*\r\n)\r",
            )
            .unwrap();
        }
        runtime_opt => {
            let args = runtime_opt.map_or(String::new(), |runtime| cmd_escape(&runtime.args));
            writeln!(cmd, "@{quoted_target} {args} %*\r").unwrap();
        }
    }

    with_utf8_codepage(cmd)
}

/// Whether a `.cmd` shim's batch context is still active while its target
/// runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdShimBatch {
    /// The target runs as a command of the batch file, as in cmd-shim.
    Kept,
    /// The shim ends its batch context before the target starts, so when
    /// Ctrl+C stops the target, cmd.exe has no batch job left to ask
    /// `Terminate batch job (Y/N)?` about.
    ///
    /// The `SETLOCAL` scope ends with the batch context, so such a shim sets no
    /// `NODE_PATH`. A target run through an interpreter keeps
    /// [`Kept`](Self::Kept), because its interpreter lookup sets `PATHEXT`,
    /// which would then leak into an interactive cmd.exe session.
    EndedBeforeTarget,
}

/// The [`CmdShimBatch::EndedBeforeTarget`] form of a shim that runs
/// `quoted_target` directly.
///
/// A `GOTO` to a missing label ends the batch context, yet cmd.exe still runs
/// the rest of the line it has already parsed, in command-line context. Percent
/// expansion happened during that parse, so the target path and `%*` are in
/// place.
///
/// An interactive cmd.exe adds the batch command to the window title and
/// restores the title when the batch ends. This exit skips the restore, so the
/// title would grow with every call. The shim resets it to `%COMSPEC%` when
/// cmd.exe is interactive, that is, when its command line has no `/c`. Under
/// `/c`, as when PowerShell runs a `.cmd`, cmd.exe leaves the title alone, and
/// so does the shim. `=` ends an `IF` operand, so the substitution goes through
/// a variable before the comparison.
///
/// A non-ASCII target needs the UTF-8 code page while cmd.exe reads the line,
/// and the line restores the caller's code page itself, since nothing after it
/// runs.
fn generate_batchless_cmd_shim(quoted_target: &str, args: &str) -> String {
    let target_command = format!("{quoted_target} {args} %*");
    let restore_codepage = if target_command.is_ascii() {
        ""
    } else {
        r#"(IF NOT "%_PNPM_CODEPAGE%"=="" "%SystemRoot%\System32\chcp.com" %_PNPM_CODEPAGE% >NUL) & "#
    };
    let mut cmd = format!(
        "@SETLOCAL\r\n\
         @SET \"_PNPM_RESTORE_TITLE=\"\r\n\
         @SETLOCAL EnableDelayedExpansion\r\n\
         @SET \"_PNPM_CMDLINE=!CMDCMDLINE:/c=!\"\r\n\
         @IF \"!_PNPM_CMDLINE!\"==\"!CMDCMDLINE!\" (ENDLOCAL & SET \"_PNPM_RESTORE_TITLE=1\") ELSE ENDLOCAL\r\n\
         {BATCH_END}(IF \"%_PNPM_RESTORE_TITLE%\"==\"1\" TITLE %COMSPEC%) & {restore_codepage}{target_command}\r\n",
    );
    if !cmd.is_ascii() {
        insert_utf8_codepage_switch(&mut cmd);
    }
    cmd
}

/// The start of the line that ends a [`CmdShimBatch::EndedBeforeTarget`]
/// shim's batch and runs its target.
const BATCH_END: &str = "@GOTO #_undefined_# 2>NUL || ";

/// What a [`CmdShimBatch::Kept`] shim runs after its target when it switched
/// the code page: the restore, with the target's exit code carried past it.
const CODEPAGE_RESTORE_TRAILER: &str = "@SET \"_PNPM_EXIT_CODE=%ERRORLEVEL%\"\r\n\
    @IF DEFINED _PNPM_CODEPAGE @\"%SystemRoot%\\System32\\chcp.com\" %_PNPM_CODEPAGE% >NUL\r\n\
    @EXIT /B %_PNPM_EXIT_CODE%\r\n";

fn with_utf8_codepage(mut cmd: String) -> String {
    if !cmd.is_ascii() {
        insert_utf8_codepage_switch(&mut cmd);
        cmd.push_str(CODEPAGE_RESTORE_TRAILER);
    }
    cmd
}

/// Save the caller's code page and switch to UTF-8, right after the shim's
/// leading `@SETLOCAL`, so cmd.exe reads the non-ASCII lines that follow
/// correctly.
fn insert_utf8_codepage_switch(cmd: &mut String) {
    cmd.insert_str(
        "@SETLOCAL\r\n".len(),
        "@SET \"_PNPM_CODEPAGE=\"\r\n\
         @FOR /F \"tokens=2 delims=:\" %%a IN ('\"%SystemRoot%\\System32\\chcp.com\"') DO @SET \"_PNPM_CODEPAGE=%%a\"\r\n\
         @\"%SystemRoot%\\System32\\chcp.com\" 65001 >NUL\r\n\
         @SET \"ERRORLEVEL=\"\r\n",
    );
}

/// Compute the Windows-style relative path from `shim_path`'s parent
/// directory to `target_path`. The `.cmd` shim uses backslashes, so we
/// convert the lexical-relative result. Falls back to the absolute path
/// if the relative computation fails. Same shape as
/// [`relative_target`] but with the slash direction flipped.
fn relative_target_windows(target_path: &Path, shim_path: &Path) -> String {
    let shim_dir = shim_path.parent().unwrap_or_else(|| Path::new(""));
    let rel = relative_path_from(shim_dir, target_path);
    rel.to_string_lossy().replace('/', r"\")
}

/// Compute the relative path from `shim_path`'s parent directory to
/// `target_path`. Falls back to the absolute target path if the relative
/// computation fails, which the sh-shim generator handles via its
/// `is_absolute` guard on the result.
fn relative_target(target_path: &Path, shim_path: &Path) -> String {
    let shim_dir = shim_path.parent().unwrap_or_else(|| Path::new(""));
    let rel = relative_path_from(shim_dir, target_path);
    rel.to_string_lossy().replace('\\', "/")
}

fn relative_path_from(from: &Path, to: &Path) -> PathBuf {
    let from = lexical_normalize(from);
    let to = lexical_normalize(to);

    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();

    let common = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut result = PathBuf::new();
    for _ in &from_components[common..] {
        result.push("..");
    }
    for component in &to_components[common..] {
        result.push(component.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

#[cfg(test)]
mod tests;

mod powershell;

mod quoting;

mod relocatable;

mod replaced_batch;

mod sh;
