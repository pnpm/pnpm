pub use powershell::generate_pwsh_shim;
pub use quoting::{cmd_escape, sh_single_quote};
pub(crate) use relocatable::{is_relocatable_shim, is_within_root};
pub use sh::{generate_sh_shim, is_sh_shim_hardened, is_shim_pointing_at};

use crate::{capabilities::FsReadHead, path_util::lexical_normalize};
use std::{
    fmt::Write as _,
    io,
    path::{Path, PathBuf},
};

/// Detected runtime for a target script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRuntime {
    /// The interpreter to invoke. `None` means "exec the file directly".
    pub prog: Option<String>,
    /// Extra arguments declared after the interpreter in the shebang. Empty
    /// when the runtime came from the extension fallback.
    pub args: String,
}

/// Map of file extensions to their default runtime when the script lacks a
/// shebang.
fn extension_program(extension: &str) -> Option<&'static str> {
    match extension {
        "js" | "cjs" | "mjs" => Some("node"),
        "cmd" | "bat" => Some("cmd"),
        "ps1" => Some("pwsh"),
        "sh" => Some("sh"),
        _ => None,
    }
}

/// Read up to 512 bytes of `path` and infer the runtime.
///
/// `NotFound` reading the file degrades to `Ok(None)` so a missing-bin race
/// doesn't fail the whole install. Other IO errors propagate, since pacquet
/// has already verified the bin path resolves under the package root by
/// this point and a real failure deserves to surface.
pub fn search_script_runtime<Sys: FsReadHead>(path: &Path) -> io::Result<Option<ScriptRuntime>> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let runtime_from_shebang = read_shebang::<Sys>(path)?;
    if let Some(rt) = runtime_from_shebang {
        return Ok(Some(rt));
    }

    if let Some(prog) = extension_program(extension) {
        let args = if prog == "cmd" { "/C" } else { "" };
        return Ok(Some(ScriptRuntime { prog: Some(prog.to_string()), args: args.to_string() }));
    }

    Ok(None)
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
///
/// Kept generic over [`FsReadHead`] so tests can plug in a fake that
/// deliberately returns short and verify the loop accumulates
/// correctly.
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

/// Parse the runtime out of the first line of a script's content. Pure
/// function over bytes so the caller can plug in any I/O strategy.
///
/// Does **not** trim leading whitespace before looking for `#!`. The
/// kernel treats `#!` as a shebang only when it sits at byte 0 of the
/// file; trimming would accept inputs like
/// `" \n#!/usr/bin/env node"` as a valid shebang and could select the
/// wrong runtime for files that just happen to mention `#!` after some
/// whitespace. The first line is taken exactly as-is (`#!` is matched
/// at column 0 of that line via `strip_prefix` in `parse_shebang`).
#[must_use]
pub fn parse_shebang_from_bytes(bytes: &[u8]) -> Option<ScriptRuntime> {
    let head = String::from_utf8_lossy(bytes);
    let first_line = head
        .split('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r');
    parse_shebang(first_line)
}

/// Parses the shebang against the grammar
/// `^#!\s*(?:/usr/bin/env(?:\s+-S\s*)?)?\s*([^ \t]+)(.*)$`.
///
/// `args` is captured **including the leading whitespace** that
/// separates it from `prog` — everything from after `prog`'s end of
/// match to end of line. Preserving the leading whitespace is what
/// produces byte-identical shim text (e.g. the double space between
/// `$basedir/sh` and `-e` in the rendered exec line).
fn parse_shebang(line: &str) -> Option<ScriptRuntime> {
    let rest = line.strip_prefix("#!")?.trim_start();
    let (rest, _) = strip_env_prefix(rest);
    let rest = rest.trim_start();

    // Slice at the first space or tab; the args slice keeps the separator
    // so the rendered shim stays byte-for-byte stable. Using `splitn`
    // would discard the separator and silently drop one space from the
    // `exec` line.
    let (prog, args) = match rest.find([' ', '\t']) {
        Some(idx) => rest.split_at(idx),
        None => (rest, ""),
    };

    if prog.is_empty() {
        return None;
    }

    Some(ScriptRuntime { prog: Some(prog.to_string()), args: args.to_string() })
}

/// Strip a leading `/usr/bin/env`, optionally followed by `-S`, from the
/// shebang body. Returns the remainder and whether `env` was present.
fn strip_env_prefix(input: &str) -> (&str, bool) {
    let Some(rest) = input.strip_prefix("/usr/bin/env") else {
        return (input, false);
    };
    let trimmed = rest.trim_start();
    if let Some(after_dash_s) = trimmed.strip_prefix("-S") {
        return (after_dash_s, true);
    }
    (trimmed, true)
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
) -> String {
    let cmd_target_rel = relative_target_windows(target_path, shim_path);
    let quoted_target = if Path::new(&cmd_target_rel).is_absolute() {
        format!(r#""{cmd_target_rel}""#)
    } else {
        format!(r#""%~dp0\{cmd_target_rel}""#)
    };

    let mut cmd = String::from("@SETLOCAL\r\n");

    let cmd_node_path = normalize_node_path_env_var(node_path, cfg!(windows)).win32;
    if !cmd_node_path.is_empty() {
        write!(
            cmd,
            "@IF NOT DEFINED NODE_PATH (\r\n  @SET \"NODE_PATH={cmd_node_path}\"\r\n) ELSE (\r\n  @SET \"NODE_PATH={cmd_node_path};%NODE_PATH%\"\r\n)\r\n",
        )
        .unwrap();
    }

    match runtime {
        Some(ScriptRuntime { prog: Some(prog), args }) => {
            let long_prog = format!(r#""%~dp0\{prog}.exe""#);
            writeln!(
                cmd,
                "@IF EXIST {long_prog} (\r\n  {long_prog} {args} {quoted_target} %*\r\n) ELSE (\r\n  @SET PATHEXT=%PATHEXT:;.JS;=;%\r\n  {prog} {args} {quoted_target} %*\r\n)\r",
            )
            .unwrap();
        }
        runtime_opt => {
            let args = runtime_opt.map_or("", |runtime| runtime.args.as_str());
            writeln!(cmd, "@{quoted_target} {args} %*\r").unwrap();
        }
    }

    cmd
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

mod sh;

mod relocatable;
