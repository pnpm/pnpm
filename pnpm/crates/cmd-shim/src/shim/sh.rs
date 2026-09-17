use super::{
    Path, ScriptRuntime, normalize_node_path_env_var, relative_target,
    relocatable::{
        BASEDIR_ABS, BASEDIR_ABS_PRELUDE, marker_target, sh_node_path_entries, shim_target_markers,
    },
};
use std::fmt::Write as _;

/// Generate the Unix shell-shim contents for `target_path`, written to
/// `shim_path`. `node_path` entries (empty for a plain shim) become the
/// cmd-shim `NODE_PATH` export block.
///
/// A shim inside `relocatable_root` names its target marker and every
/// `NODE_PATH` entry inside that root relative to its own directory, so the
/// tree keeps working after the root moves. `None`, or any root on Windows,
/// writes the absolute paths `@zkochan/cmd-shim` writes.
#[must_use]
pub fn generate_sh_shim(
    target_path: &Path,
    shim_path: &Path,
    runtime: Option<&ScriptRuntime>,
    node_path: &[String],
    relocatable_root: Option<&Path>,
) -> String {
    let shim_dir = shim_path.parent().unwrap_or_else(|| Path::new(""));
    let mut sh = String::from(SH_SHIM_HEADER);
    write_sh_node_path(&mut sh, &sh_node_path_entries(node_path, shim_dir, relocatable_root));

    let sh_target = relative_target(target_path, shim_path);
    let absolute = Path::new(&sh_target).is_absolute();
    let quoted = QuotedTarget {
        posix: if absolute {
            format!(r#""{sh_target}""#)
        } else {
            format!(r#""$basedir/{sh_target}""#)
        },
        windows: if absolute {
            format!(r#""{sh_target}""#)
        } else {
            format!(r#""$basedir_win/{sh_target}""#)
        },
    };

    match runtime {
        Some(ScriptRuntime { prog: Some(prog), args }) => {
            write_sh_runtime_exec(&mut sh, prog, args, &quoted);
        }
        // The trailing `exit $?` is unreachable after the `exec`. It is
        // emitted anyway because upstream emits it, which is what keeps
        // shims outside a relocatable root byte-identical to upstream's.
        runtime_opt => {
            let args = runtime_opt.map_or("", |runtime| runtime.args.as_str());
            let quoted_target = &quoted.posix;
            writeln!(sh, "exec {quoted_target} {args} \"$@\"\nexit $?").unwrap();
        }
    }

    let marker = marker_target(target_path, &sh_target, shim_dir, relocatable_root);
    writeln!(sh, "# {}", shim_target_marker(&marker)).unwrap();
    sh
}

/// How the shim spells its target, in the two path flavors a shim under MSYS
/// has to choose between.
struct QuotedTarget {
    posix: String,
    windows: String,
}

/// Prepend the shim's own `node_modules` directories to `NODE_PATH`, when the
/// linker asked for any.
fn write_sh_node_path(sh: &mut String, node_path: &[String]) {
    let sh_node_path = normalize_node_path_env_var(node_path).posix;
    if sh_node_path.is_empty() {
        return;
    }
    if node_path
        .iter()
        .any(|entry| entry.starts_with(BASEDIR_ABS))
    {
        sh.push_str(BASEDIR_ABS_PRELUDE);
    }
    writeln!(
        sh,
        "if [ -z \"$NODE_PATH\" ]; then\n  export NODE_PATH=\"{sh_node_path}\"\nelse\n  export NODE_PATH=\"{sh_node_path}:$NODE_PATH\"\nfi",
    )
    .unwrap();
}

/// Emit the `exec` block for a target with a script runtime, wrapping it in an
/// MSYS branch when `cmd` switches need escaping there.
fn write_sh_runtime_exec(sh: &mut String, prog: &str, args: &str, quoted: &QuotedTarget) {
    let prog_base = strip_exe_suffix(prog).unwrap_or(prog);
    let prog_has_exe = prog_base.len() != prog.len();
    let prog_exe = if prog_has_exe { prog.to_string() } else { format!("{prog}.exe") };
    let exec = |exec_args: &str| {
        sh_exec_block(&ShExec { prog, prog_exe: &prog_exe, prog_has_exe, quoted }, exec_args)
    };
    let msys_args = prog_base
        .eq_ignore_ascii_case("cmd")
        .then(|| escape_msys_cmd_switches(args))
        .filter(|escaped_args| escaped_args != args);
    let Some(msys_args) = msys_args else {
        sh.push_str(&exec(args));
        return;
    };
    writeln!(
        sh,
        "if [ -n \"$msys\" ]; then\n{}else\n{}fi",
        indent_shell_block(&exec(&msys_args)),
        indent_shell_block(&exec(args)),
    )
    .unwrap();
}

/// What one `exec` block runs the target through.
struct ShExec<'a> {
    prog: &'a str,
    prog_exe: &'a str,
    /// Whether `prog` already carried the `.exe` suffix.
    prog_has_exe: bool,
    quoted: &'a QuotedTarget,
}

/// One `exec` block: a program that already names an executable runs directly,
/// while a bare program name is probed in the bin directory, then on `PATH`.
fn sh_exec_block(exec: &ShExec<'_>, exec_args: &str) -> String {
    let ShExec {
        prog,
        prog_exe,
        prog_has_exe,
        quoted,
    } = *exec;
    let quoted_target = &quoted.posix;
    let quoted_target_win = &quoted.windows;
    let sh_long_prog_exe = format!(r#""$basedir/{prog_exe}""#);
    let mut block = String::new();
    if prog_has_exe {
        writeln!(
            block,
            "if [ -x {sh_long_prog_exe} ]; then\n  exec {sh_long_prog_exe} {exec_args} {quoted_target_win} \"$@\"\nelse\n  exec {prog_exe} {exec_args} {quoted_target_win} \"$@\"\nfi",
        )
        .unwrap();
        return block;
    }
    let sh_long_prog = format!(r#""$basedir/{prog}""#);
    writeln!(
        block,
        "if [ -n \"$exe\" ] && [ -x {sh_long_prog_exe} ]; then\n  exec {sh_long_prog_exe} {exec_args} {quoted_target_win} \"$@\"\nelif [ -x {sh_long_prog} ]; then\n  exec {sh_long_prog} {exec_args} {quoted_target} \"$@\"\nelif command -v {prog} >/dev/null 2>&1; then\n  exec {prog} {exec_args} {quoted_target} \"$@\"\nelif [ -n \"$exe\" ] && command -v {prog_exe} >/dev/null 2>&1; then\n  exec {prog_exe} {exec_args} {quoted_target_win} \"$@\"\nelse\n  exec {prog} {exec_args} {quoted_target} \"$@\"\nfi",
    )
    .unwrap();
    block
}

const SH_SHIM_HEADER: &str = r#"#!/bin/sh
# Resolve $0 through symlinks so basedir is the shim's real directory.
# Cap hops at the kernel's ELOOP limit so a cycle cannot hang the shim.
#
# A shim runs with node_modules/.bin at the front of PATH, so readlink, sed,
# uname, and printf go through `command -p`, which searches the system default
# path instead. A dependency's bin cannot stand in for one of them and take over
# the shim before it reaches its target. Directories come from `${link%/*}`,
# which needs no helper at all.
link="$0"
# `${link%/*}` needs a separator to strip. A bare name came from a PATH lookup
# and stands for a file in the current directory.
case "$link" in
  */*|*\\*) ;;
  *) link="./$link" ;;
esac
hops=0
while [ -L "$link" ] && [ "$hops" -lt 40 ]; do
  hops=$((hops+1))
  target=$(command -p readlink "$link")
  case "$target" in
    /*) link="$target" ;;
    *)  link="${link%/*}/$target" ;;
  esac
done
basedir=$(command -p printf '%s\n' "$link" | command -p sed -e 's,\\,/,g')
basedir="${basedir%/*}"
basedir_win="$basedir"
exe=""
msys=""

case `command -p uname -a` in
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

pub(super) fn escape_msys_cmd_switches(args: &str) -> String {
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

pub(super) fn strip_exe_suffix(prog: &str) -> Option<&str> {
    let suffix_start = prog.len().checked_sub(4)?;
    prog.as_bytes()[suffix_start..]
        .eq_ignore_ascii_case(b".exe")
        .then(|| &prog[..suffix_start])
}

/// Trailing `# cmd-shim-target=<target>` marker, naming the target relative
/// to the shim inside a relocatable root and absolutely otherwise.
/// [`is_shim_pointing_at`] reads it to detect whether an existing shim
/// already targets the same source without re-parsing its body,
/// short-circuiting warm reinstalls.
fn shim_target_marker(target: &str) -> String {
    format!("cmd-shim-target={}", target.replace('\\', "/"))
}

/// Whether an already-on-disk shim at `shim_path` targets `target_path`. The
/// check looks for the trailing marker line so the header text never has to
/// be byte-identical between cmd-shim versions. A relative marker resolves
/// against the shim's directory.
#[must_use]
pub fn is_shim_pointing_at(shim_content: &str, shim_path: &Path, target_path: &Path) -> bool {
    let target = target_path.to_string_lossy().replace('\\', "/");
    shim_target_markers(shim_content)
        .any(|marker| marker == target || marker == relative_target(target_path, shim_path))
}

/// The line the header resolves `readlink` through. Taken verbatim from
/// [`SH_SHIM_HEADER`], which
/// `generate_sh_shim_header_carries_the_hardened_helper_line` pins, so the
/// header cannot drift away from what [`is_sh_shim_hardened`] looks for.
pub(super) const SH_SHIM_HARDENED_HELPER_LINE: &str = r#"  target=$(command -p readlink "$link")"#;

/// The line the header prints `$link` through before converting backslashes.
/// Pinned the same way as [`SH_SHIM_HARDENED_HELPER_LINE`].
pub(super) const SH_SHIM_PATH_PRINTF_LINE: &str =
    r#"basedir=$(command -p printf '%s\n' "$link" | command -p sed -e 's,\\,/,g')"#;

/// Whether an already-on-disk POSIX shim has the header a warm reinstall can
/// leave in place.
///
/// The trailing target marker does not describe the header, so a shim whose
/// target has not moved can still be stale. This looks for the `command -p`
/// `readlink` lookup and the `printf` path conversion. Missing either means
/// the next install rewrites the shim: a helper taken from `PATH` can redirect
/// it, and a POSIX `echo` eats backslash escapes in a Windows-form `$0`.
#[must_use]
pub fn is_sh_shim_hardened(shim_content: &str) -> bool {
    shim_content
        .lines()
        .any(|line| line == SH_SHIM_HARDENED_HELPER_LINE)
        && shim_content
            .lines()
            .any(|line| line == SH_SHIM_PATH_PRINTF_LINE)
}
