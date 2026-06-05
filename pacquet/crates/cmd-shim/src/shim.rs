use crate::{capabilities::FsReadHead, path_util::lexical_normalize};
use std::{
    fmt::Write as _,
    io,
    path::{Path, PathBuf},
};

/// Detected runtime for a target script.
///
/// Mirrors the return shape of `searchScriptRuntime` in
/// <https://github.com/pnpm/cmd-shim/blob/0d79ca9534/src/index.ts>.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRuntime {
    /// The interpreter to invoke. `None` means "exec the file directly".
    pub prog: Option<String>,
    /// Extra arguments declared after the interpreter in the shebang. Empty
    /// when the runtime came from the extension fallback.
    pub args: String,
}

/// Map of file extensions to their default runtime when the script lacks a
/// shebang. Mirrors `extensionToProgramMap` in upstream cmd-shim.
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
/// Order, mirroring `searchScriptRuntime`:
///
/// 1. If the file exists and starts with a shebang, parse `prog` + `args` from
///    it.
/// 2. Otherwise look up a default runtime by file extension (e.g. `.js` →
///    `node`, `.cmd` → `cmd`).
/// 3. If neither yields a runtime, return `None`. [`generate_sh_shim`]
///    handles that by exec'ing the target directly.
///
/// `NotFound` reading the file degrades to `Ok(None)` so a missing-bin race
/// doesn't fail the whole install. Other IO errors propagate, since pacquet
/// has already verified the bin path resolves under the package root by
/// this point and a real failure deserves to surface.
pub fn search_script_runtime<Sys: FsReadHead>(path: &Path) -> io::Result<Option<ScriptRuntime>> {
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    let runtime_from_shebang = read_shebang::<Sys>(path)?;
    if let Some(rt) = runtime_from_shebang {
        return Ok(Some(rt));
    }

    if let Some(prog) = extension_program(extension) {
        return Ok(Some(ScriptRuntime { prog: Some(prog.to_string()), args: String::new() }));
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
/// kernel and upstream cmd-shim both treat `#!` as a shebang only when
/// it sits at byte 0 of the file; an earlier
/// `String::from_utf8_lossy(bytes).trim_start()` accepted inputs like
/// `" \n#!/usr/bin/env node"` as a valid shebang and could select the
/// wrong runtime for files that just happen to mention `#!` after some
/// whitespace. The first line is taken exactly as-is (`#!` is matched
/// at column 0 of that line via `strip_prefix` in `parse_shebang`).
#[must_use]
pub fn parse_shebang_from_bytes(bytes: &[u8]) -> Option<ScriptRuntime> {
    let head = String::from_utf8_lossy(bytes);
    let first_line = head.split('\n').next().unwrap_or("").trim_end_matches('\r');
    parse_shebang(first_line)
}

/// Mirrors the shebang regex in upstream cmd-shim:
/// `^#!\s*(?:/usr/bin/env(?:\s+-S\s*)?)?\s*([^ \t]+)(.*)$`.
///
/// Recognises `#!/usr/bin/env <prog>`, `#!/usr/bin/env -S <prog>`, and any
/// direct `#!/path/to/<prog>` shebang. `args` is captured **including the
/// leading whitespace** that separates it from `prog`. That matches
/// upstream's regex group 2 (`(.*)`), which captures everything from after
/// `prog`'s end-of-match to end of line. Preserving the leading whitespace
/// is what produces the byte-identical shim text upstream emits (e.g. the
/// double space between `$basedir/sh` and `-e` in the rendered exec line).
fn parse_shebang(line: &str) -> Option<ScriptRuntime> {
    let rest = line.strip_prefix("#!")?.trim_start();
    let (rest, _) = strip_env_prefix(rest);
    let rest = rest.trim_start();

    // Slice at the first space or tab; the args slice keeps the separator
    // so the rendered shim matches upstream byte-for-byte. Using `splitn`
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

/// Generate the Unix shell-shim contents for `target_path`, written to
/// `shim_path`. Mirrors `generateShShim` in upstream cmd-shim.
///
/// The shim is a pure `/bin/sh` script that:
///
/// 1. Resolves `basedir` to its own directory (with a `cygpath` fixup for
///    MSYS-style POSIX shells on Windows).
/// 2. If the runtime program is colocated at `$basedir/<prog>` (a rare case,
///    only true when the runtime was bundled alongside the shim), prefer that
///    binary; otherwise fall through to the system PATH.
/// 3. Forwards `"$@"` to the resolved interpreter, with the target script as
///    the first positional argument.
///
/// When [`search_script_runtime`] returned `None` (no shebang, unknown
/// extension), the shim execs the target directly via the second branch
/// upstream uses for that case.
#[must_use]
pub fn generate_sh_shim(
    target_path: &Path,
    shim_path: &Path,
    runtime: Option<&ScriptRuntime>,
) -> String {
    let mut sh = String::from(SH_SHIM_HEADER);

    let sh_target = relative_target(target_path, shim_path);
    let quoted_target = if Path::new(&sh_target).is_absolute() {
        format!("\"{sh_target}\"")
    } else {
        format!("\"$basedir/{sh_target}\"")
    };

    match runtime {
        Some(ScriptRuntime { prog: Some(prog), args }) => {
            // `sh_long_prog` is the `"$basedir/<prog>"` form upstream uses.
            // It always carries the leading `$basedir/` and quotes; never
            // just the program name on its own.
            let sh_long_prog = format!("\"$basedir/{prog}\"");
            writeln!(
                sh,
                "if [ -x {sh_long_prog} ]; then\n  exec {sh_long_prog} {args} {quoted_target} \"$@\"\nelse\n  exec {prog} {args} {quoted_target} \"$@\"\nfi",
            )
            .unwrap();
        }
        // No runtime detected, so exec the target directly. Upstream still
        // emits `exit $?` on this branch for parity with non-execve POSIX
        // shells.
        runtime_opt => {
            let args = runtime_opt.map_or("", |runtime| runtime.args.as_str());
            writeln!(sh, "{quoted_target} {args} \"$@\"\nexit $?").unwrap();
        }
    }

    writeln!(sh, "# {}", shim_target_marker(target_path)).unwrap();
    sh
}

/// Generate the Windows `.cmd` shim contents for `target_path`. Mirrors
/// `generateCmdShim` in upstream cmd-shim. Pacquet skips the
/// `nodePath`/`prependToPath`/`nodeExecPath`/`progArgs` features that
/// upstream supports; we only ever write a "plain" cmd shim.
///
/// CRLF line endings are part of the on-disk contract for `.cmd` files
/// on Windows, so the template uses literal `\r\n`.
#[must_use]
pub fn generate_cmd_shim(
    target_path: &Path,
    shim_path: &Path,
    runtime: Option<&ScriptRuntime>,
) -> String {
    let cmd_target_rel = relative_target_windows(target_path, shim_path);
    let quoted_target = if Path::new(&cmd_target_rel).is_absolute() {
        format!("\"{cmd_target_rel}\"")
    } else {
        format!("\"%~dp0\\{cmd_target_rel}\"")
    };

    let mut cmd = String::from("@SETLOCAL\r\n");

    match runtime {
        Some(ScriptRuntime { prog: Some(prog), args }) => {
            let long_prog = format!("\"%~dp0\\{prog}.exe\"");
            writeln!(
                cmd,
                "@IF EXIST {long_prog} (\r\n  {long_prog} {args} {quoted_target} %*\r\n) ELSE (\r\n  @SET PATHEXT=%PATHEXT:;.JS;=;%\r\n  {prog} {args} {quoted_target} %*\r\n)\r",
            )
            .unwrap();
        }
        runtime_opt => {
            let args = runtime_opt.map_or("", |runtime| runtime.args.as_str());
            // No runtime detected, so exec the target directly.
            writeln!(cmd, "@{quoted_target} {args} %*\r").unwrap();
        }
    }

    cmd
}

/// Generate the cross-shell PowerShell `.ps1` shim contents for
/// `target_path`. Mirrors `generatePwshShim` in upstream cmd-shim,
/// minus the `nodePath`/`prependToPath`/`nodeExecPath`/`progArgs`
/// branches we don't use. The shim self-detects Windows vs. POSIX-ish
/// pwsh and adjusts the executable suffix accordingly.
#[must_use]
pub fn generate_pwsh_shim(
    target_path: &Path,
    shim_path: &Path,
    runtime: Option<&ScriptRuntime>,
) -> String {
    let sh_target = relative_target(target_path, shim_path);
    let quoted_target = if Path::new(&sh_target).is_absolute() {
        format!("\"{sh_target}\"")
    } else {
        format!("\"$basedir/{sh_target}\"")
    };

    use std::fmt::Write;
    let mut pwsh = String::from(PWSH_SHIM_HEADER);

    match runtime {
        Some(ScriptRuntime { prog: Some(prog), args }) => {
            let long_prog = format!(r#""$basedir/{prog}$exe""#);
            let prog_quoted = format!(r#""{prog}$exe""#);
            writeln!(pwsh).unwrap();
            writeln!(pwsh, "$ret=0").unwrap();
            writeln!(pwsh, "if (Test-Path {long_prog}) {{").unwrap();
            writeln!(pwsh, "  # Support pipeline input").unwrap();
            writeln!(pwsh, "  if ($MyInvocation.ExpectingInput) {{").unwrap();
            writeln!(pwsh, "    $input | & {long_prog} {args} {quoted_target} $args").unwrap();
            writeln!(pwsh, "  }} else {{").unwrap();
            writeln!(pwsh, "    & {long_prog} {args} {quoted_target} $args").unwrap();
            writeln!(pwsh, "  }}").unwrap();
            writeln!(pwsh, "  $ret=$LASTEXITCODE").unwrap();
            writeln!(pwsh, "}} else {{").unwrap();
            writeln!(pwsh, "  # Support pipeline input").unwrap();
            writeln!(pwsh, "  if ($MyInvocation.ExpectingInput) {{").unwrap();
            writeln!(pwsh, "    $input | & {prog_quoted} {args} {quoted_target} $args").unwrap();
            writeln!(pwsh, "  }} else {{").unwrap();
            writeln!(pwsh, "    & {prog_quoted} {args} {quoted_target} $args").unwrap();
            writeln!(pwsh, "  }}").unwrap();
            writeln!(pwsh, "  $ret=$LASTEXITCODE").unwrap();
            writeln!(pwsh, "}}").unwrap();
            writeln!(pwsh, "exit $ret").unwrap();
        }
        runtime_opt => {
            let args = runtime_opt.map_or("", |runtime| runtime.args.as_str());
            writeln!(pwsh).unwrap();
            writeln!(pwsh, "# Support pipeline input").unwrap();
            writeln!(pwsh, "if ($MyInvocation.ExpectingInput) {{").unwrap();
            writeln!(pwsh, "  $input | & {quoted_target} {args} $args").unwrap();
            writeln!(pwsh, "}} else {{").unwrap();
            writeln!(pwsh, "  & {quoted_target} {args} $args").unwrap();
            writeln!(pwsh, "}}").unwrap();
            writeln!(pwsh, "exit $LASTEXITCODE").unwrap();
        }
    }

    pwsh
}

/// `.ps1` template prelude. Sets up `$basedir` and `$exe` exactly like
/// upstream's `generatePwshShim`.
const PWSH_SHIM_HEADER: &str = r#"#!/usr/bin/env pwsh
$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent

$exe=""
if ($PSVersionTable.PSVersion -lt "6.0" -or $IsWindows) {
  # Fix case when both the Windows and Linux builds of Node
  # are installed in the same directory
  $exe=".exe"
}"#;

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

const SH_SHIM_HEADER: &str = r#"#!/bin/sh
basedir=$(dirname "$(echo "$0" | sed -e 's,\\,/,g')")

case `uname` in
    *CYGWIN*|*MINGW*|*MSYS*)
        if command -v cygpath > /dev/null 2>&1; then
            basedir=`cygpath -w "$basedir"`
        fi
    ;;
esac

"#;

/// Trailing `# cmd-shim-target=<rel>` marker. Upstream uses it to detect
/// whether an existing shim already targets the same source without
/// re-parsing its body. Pacquet uses [`is_shim_pointing_at`] for the same
/// short-circuit on warm reinstalls.
fn shim_target_marker(target_path: &Path) -> String {
    format!("cmd-shim-target={}", target_path.to_string_lossy().replace('\\', "/"))
}

/// Whether an already-on-disk shim targets `target_path`. Mirrors
/// `isShimPointingAt`. The check looks for the trailing marker line so the
/// header text never has to be byte-identical between cmd-shim versions.
#[must_use]
pub fn is_shim_pointing_at(shim_content: &str, target_path: &Path) -> bool {
    let marker = format!("# {}", shim_target_marker(target_path));
    shim_content.lines().any(|line| line == marker)
}

/// Compute the relative path from `shim_path`'s parent directory to
/// `target_path`. Falls back to the absolute target path if the relative
/// computation fails. That matches the `path.isAbsolute(shTarget)` guard in
/// upstream's `generateShShim`.
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

    let common =
        from_components.iter().zip(to_components.iter()).take_while(|(a, b)| a == b).count();

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
