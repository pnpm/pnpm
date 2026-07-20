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
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");

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
    let first_line = head.split('\n').next().unwrap_or("").trim_end_matches('\r');
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
/// backslashes, `posix` joins with `:` and forward slashes. On a
/// Windows host the posix form additionally rewrites the drive prefix
/// (`C:` → `/proc/cygdrive/c` under Cygwin/MSYS, `/mnt/c` otherwise),
/// matching cmd-shim. On Unix the entries pass through unchanged.
struct NodePathEnvVar {
    win32: String,
    posix: String,
}

fn normalize_node_path_env_var(node_path: &[String]) -> NodePathEnvVar {
    let mut win32 = String::new();
    let mut posix = String::new();
    for entry in node_path {
        let entry_win32 = entry.replace('/', r"\");
        let entry_posix = if cfg!(windows) { windows_entry_to_posix(entry) } else { entry.clone() };
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
/// leading drive letter to the Cygwin/WSL mount prefix. Cygwin/MSYS is
/// detected the way cmd-shim does — `TERM=CYGWIN` or a set `MSYSTEM`.
fn windows_entry_to_posix(entry: &str) -> String {
    let flipped = entry.replace('\\', "/");
    let Some((drive, rest)) = flipped.split_once(':') else {
        return flipped;
    };
    if drive.is_empty() || drive.contains('/') {
        return flipped;
    }
    let is_cygwin = std::env::var("TERM").is_ok_and(|term| term == "CYGWIN")
        || std::env::var_os("MSYSTEM").is_some();
    let mount = if is_cygwin { "/proc/cygdrive" } else { "/mnt" };
    format!("{mount}/{}{rest}", drive.to_lowercase())
}

/// Generate the Unix shell-shim contents for `target_path`, written to
/// `shim_path`. `node_path` entries (empty for a plain shim) become the
/// cmd-shim `NODE_PATH` export block.
#[must_use]
pub fn generate_sh_shim(
    target_path: &Path,
    shim_path: &Path,
    runtime: Option<&ScriptRuntime>,
    node_path: &[String],
) -> String {
    let mut sh = String::from(SH_SHIM_HEADER);

    let sh_node_path = normalize_node_path_env_var(node_path).posix;
    if !sh_node_path.is_empty() {
        writeln!(
            sh,
            "if [ -z \"$NODE_PATH\" ]; then\n  export NODE_PATH=\"{sh_node_path}\"\nelse\n  export NODE_PATH=\"{sh_node_path}:$NODE_PATH\"\nfi",
        )
        .unwrap();
    }

    let sh_target = relative_target(target_path, shim_path);
    let quoted_target = if Path::new(&sh_target).is_absolute() {
        format!(r#""{sh_target}""#)
    } else {
        format!(r#""$basedir/{sh_target}""#)
    };
    let quoted_target_win = if Path::new(&sh_target).is_absolute() {
        format!(r#""{sh_target}""#)
    } else {
        format!(r#""$basedir_win/{sh_target}""#)
    };

    match runtime {
        Some(ScriptRuntime { prog: Some(prog), args }) => {
            let prog_base = strip_exe_suffix(prog).unwrap_or(prog);
            let prog_has_exe = prog_base.len() != prog.len();
            let prog_exe = if prog_has_exe { prog.clone() } else { format!("{prog}.exe") };
            let sh_long_prog_exe = format!(r#""$basedir/{prog_exe}""#);
            let exec_block = |exec_args: &str| {
                let mut block = String::new();
                if prog_has_exe {
                    writeln!(
                        block,
                        "if [ -x {sh_long_prog_exe} ]; then\n  exec {sh_long_prog_exe} {exec_args} {quoted_target_win} \"$@\"\nelse\n  exec {prog_exe} {exec_args} {quoted_target_win} \"$@\"\nfi",
                    )
                    .unwrap();
                } else {
                    let sh_long_prog = format!(r#""$basedir/{prog}""#);
                    writeln!(
                        block,
                        "if [ -n \"$exe\" ] && [ -x {sh_long_prog_exe} ]; then\n  exec {sh_long_prog_exe} {exec_args} {quoted_target_win} \"$@\"\nelif [ -x {sh_long_prog} ]; then\n  exec {sh_long_prog} {exec_args} {quoted_target} \"$@\"\nelif command -v {prog} >/dev/null 2>&1; then\n  exec {prog} {exec_args} {quoted_target} \"$@\"\nelif [ -n \"$exe\" ] && command -v {prog_exe} >/dev/null 2>&1; then\n  exec {prog_exe} {exec_args} {quoted_target_win} \"$@\"\nelse\n  exec {prog} {exec_args} {quoted_target} \"$@\"\nfi",
                    )
                    .unwrap();
                }
                block
            };

            let msys_args = prog_base
                .eq_ignore_ascii_case("cmd")
                .then(|| escape_msys_cmd_switches(args))
                .filter(|escaped_args| escaped_args != args);
            if let Some(msys_args) = msys_args {
                writeln!(
                    sh,
                    "if [ -n \"$msys\" ]; then\n{}else\n{}fi",
                    indent_shell_block(&exec_block(&msys_args)),
                    indent_shell_block(&exec_block(args)),
                )
                .unwrap();
            } else {
                sh.push_str(&exec_block(args));
            }
        }
        // Emit `exit $?` on this branch for parity with non-execve
        // POSIX shells.
        runtime_opt => {
            let args = runtime_opt.map_or("", |runtime| runtime.args.as_str());
            writeln!(sh, "{quoted_target} {args} \"$@\"\nexit $?").unwrap();
        }
    }

    writeln!(sh, "# {}", shim_target_marker(target_path)).unwrap();
    sh
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

    let cmd_node_path = normalize_node_path_env_var(node_path).win32;
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

/// Generate the cross-shell PowerShell `.ps1` shim contents for
/// `target_path`, minus the `prependToPath`/`nodeExecPath`/`progArgs`
/// branches we don't use. `node_path` entries (empty for a plain shim)
/// become the cmd-shim `NODE_PATH` set/restore blocks. The shim
/// self-detects Windows vs. POSIX-ish pwsh and adjusts the executable
/// suffix (and `NODE_PATH` flavor) accordingly.
#[must_use]
pub fn generate_pwsh_shim(
    target_path: &Path,
    shim_path: &Path,
    runtime: Option<&ScriptRuntime>,
    node_path: &[String],
) -> String {
    let sh_target = relative_target(target_path, shim_path);
    let quoted_target = if Path::new(&sh_target).is_absolute() {
        format!(r#""{sh_target}""#)
    } else {
        format!(r#""$basedir/{sh_target}""#)
    };

    use std::fmt::Write;
    let NodePathEnvVar { win32: win32_node_path, posix: posix_node_path } =
        normalize_node_path_env_var(node_path);
    let has_node_path = !win32_node_path.is_empty();
    let mut pwsh = if has_node_path {
        format!(
            "#!/usr/bin/env pwsh\n$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent\n\n$exe=\"\"\n$pathsep=\":\"\n$env_node_path=$env:NODE_PATH\n$new_node_path=\"{win32_node_path}\"\nif ($PSVersionTable.PSVersion -lt \"6.0\" -or $IsWindows) {{\n  # Fix case when both the Windows and Linux builds of Node\n  # are installed in the same directory\n  $exe=\".exe\"\n  $pathsep=\";\"\n}} else {{\n  $new_node_path=\"{posix_node_path}\"\n}}\nif ([string]::IsNullOrEmpty($env_node_path)) {{\n  $env:NODE_PATH=$new_node_path\n}} else {{\n  $env:NODE_PATH=\"$new_node_path$pathsep$env_node_path\"\n}}",
        )
    } else {
        String::from(PWSH_SHIM_HEADER)
    };
    let restore_node_path = has_node_path.then_some("$env:NODE_PATH=$env_node_path");

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
            if let Some(restore) = restore_node_path {
                writeln!(pwsh, "{restore}").unwrap();
            }
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
            if let Some(restore) = restore_node_path {
                writeln!(pwsh, "{restore}").unwrap();
            }
            writeln!(pwsh, "exit $LASTEXITCODE").unwrap();
        }
    }

    pwsh
}

/// `.ps1` template prelude. Sets up `$basedir` and `$exe`.
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
basedir_win="$basedir"
exe=""
msys=""

case `uname -a` in
  *CYGWIN*|*MINGW*|*MSYS*)
    if command -v cygpath > /dev/null 2>&1; then
      basedir_win=`cygpath -w "$basedir"`
    fi
    exe=".exe"
    msys="true"
  ;;
  *WSL2*)
    if command -v wslpath > /dev/null 2>&1; then
      basedir_win="$(wslpath -w "$basedir" 2> /dev/null)"
      if [ $? -ne 0 ] || [ -z "$basedir_win" ]; then
        basedir_win="$basedir"
      else
        exe=".exe"
      fi
    fi
  ;;
esac

"#;

fn indent_shell_block(script: &str) -> String {
    script
        .split('\n')
        .map(|line| if line.is_empty() { String::new() } else { format!("  {line}") })
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape_msys_cmd_switches(args: &str) -> String {
    let mut escaped = String::with_capacity(args.len());
    let mut chars = args.char_indices();
    let mut at_boundary = true;

    while let Some((_, ch)) = chars.next() {
        if ch == '/' && at_boundary {
            let mut lookahead = chars.clone();
            if let Some((_, switch @ ('C' | 'c' | 'K' | 'k'))) = lookahead.next()
                && lookahead.next().is_none_or(|(_, next)| next.is_whitespace())
            {
                escaped.push('/');
                escaped.push('/');
                escaped.push(switch);
                chars.next();
                at_boundary = false;
                continue;
            }
        }

        escaped.push(ch);
        at_boundary = ch.is_whitespace();
    }

    escaped
}

fn strip_exe_suffix(prog: &str) -> Option<&str> {
    let suffix_start = prog.len().checked_sub(4)?;
    prog.as_bytes()[suffix_start..].eq_ignore_ascii_case(b".exe").then(|| &prog[..suffix_start])
}

/// Trailing `# cmd-shim-target=<rel>` marker. [`is_shim_pointing_at`]
/// reads it to detect whether an existing shim already targets the same
/// source without re-parsing its body, short-circuiting warm reinstalls.
fn shim_target_marker(target_path: &Path) -> String {
    format!("cmd-shim-target={}", target_path.to_string_lossy().replace('\\', "/"))
}

/// Whether an already-on-disk shim targets `target_path`. The check looks
/// for the trailing marker line so the header text never has to be
/// byte-identical between cmd-shim versions.
#[must_use]
pub fn is_shim_pointing_at(shim_content: &str, target_path: &Path) -> bool {
    let marker = format!("# {}", shim_target_marker(target_path));
    shim_content.lines().any(|line| line == marker)
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
