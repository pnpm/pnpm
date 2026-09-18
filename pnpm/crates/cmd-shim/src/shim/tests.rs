use super::{
    ScriptRuntime, extension_program, generate_cmd_shim, generate_pwsh_shim, generate_sh_shim,
    is_sh_shim_hardened, is_shim_pointing_at, parse_shebang, parse_shebang_from_bytes,
    read_head_filled, relative_target, search_script_runtime,
    sh::{
        SH_SHIM_CYGPATH_LINE, SH_SHIM_HARDENED_HELPER_LINE, SH_SHIM_PATH_PRINTF_LINE,
        SH_SHIM_WSLPATH_LINE, escape_msys_cmd_switches, strip_exe_suffix,
    },
};
use crate::{
    capabilities::{FsReadHead, Host},
    path_util::lexical_normalize,
};
use std::{
    io,
    path::{Path, PathBuf},
};

#[test]
fn parses_env_node_shebang() {
    let rt = parse_shebang("#!/usr/bin/env node").unwrap();
    assert_eq!(rt.prog.as_deref(), Some("node"));
    assert_eq!(rt.args, "");
}

#[test]
fn parses_env_dash_s_shebang() {
    let rt = parse_shebang("#!/usr/bin/env -S node --experimental").unwrap();
    assert_eq!(rt.prog.as_deref(), Some("node"));
    // Leading space is preserved because upstream's regex group 2
    // captures the separator. Pinning `" --experimental"` (with the
    // space) is what makes the rendered shim's `exec` line match
    // upstream byte-for-byte.
    assert_eq!(rt.args, " --experimental");
}

#[test]
fn parses_direct_shebang() {
    let rt = parse_shebang("#!/bin/sh -e").unwrap();
    assert_eq!(rt.prog.as_deref(), Some("/bin/sh"));
    // Leading space preserved (see `parses_env_dash_s_shebang`).
    assert_eq!(rt.args, " -e");
}

#[test]
fn rejects_non_shebang_lines() {
    assert!(parse_shebang("just text").is_none());
    assert!(parse_shebang("#! ").is_none());
}

#[test]
fn extension_fallback_picks_node_for_js() {
    assert_eq!(extension_program("js"), Some("node"));
    assert_eq!(extension_program("cjs"), Some("node"));
    assert_eq!(extension_program("mjs"), Some("node"));
}

#[test]
fn relative_target_traverses_into_sibling_package() {
    let target = Path::new("/proj/node_modules/foo/bin/cli.js");
    let shim = Path::new("/proj/node_modules/.bin/cli");
    assert_eq!(relative_target(target, shim), "../foo/bin/cli.js");
}

/// `is_sh_shim_hardened` decides whether a warm reinstall replaces a shim an
/// older pacquet wrote, by looking for exact lines of the header. Reformat
/// those lines and every existing shim starts looking unhardened, so pin them
/// together.
#[test]
fn generate_sh_shim_header_carries_the_hardened_helper_line() {
    let target = Path::new("/proj/node_modules/typescript/bin/tsc");
    let shim = Path::new("/proj/node_modules/.bin/tsc");
    let body = generate_sh_shim(target, shim, None, &[]);

    assert!(
        is_sh_shim_hardened(&body),
        "a freshly generated shim must count as hardened, body was:\n{body}",
    );
    assert!(
        !is_sh_shim_hardened(&body.replace(SH_SHIM_HARDENED_HELPER_LINE, "  target=$(readlink)")),
        "a shim that looks up readlink on PATH must not count as hardened",
    );
    assert!(
        !is_sh_shim_hardened(&body.replace(
            SH_SHIM_PATH_PRINTF_LINE,
            r#"basedir=$(echo "$link" | command -p sed -e 's,\\,/,g')"#,
        )),
        "a shim that pipes $link through echo must not count as hardened",
    );
    assert!(
        !is_sh_shim_hardened(&body.replace(
            SH_SHIM_CYGPATH_LINE,
            r"    if command -v cygpath > /dev/null 2>&1; then"
        )),
        "a shim that looks up cygpath on PATH must not count as hardened",
    );
    assert!(
        !is_sh_shim_hardened(&body.replace(
            SH_SHIM_WSLPATH_LINE,
            r"    if command -v wslpath > /dev/null 2>&1; then"
        )),
        "a shim that looks up wslpath on PATH must not count as hardened",
    );
}

#[test]
fn generate_sh_shim_matches_pnpm_typical_case() {
    let target = Path::new("/proj/node_modules/typescript/bin/tsc");
    let shim = Path::new("/proj/node_modules/.bin/tsc");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    let body = generate_sh_shim(target, shim, Some(&runtime), &[]);

    assert!(body.starts_with("#!/bin/sh\n"), "shebang must come first");
    assert!(
        body.contains(
            r#"basedir_win="$basedir"
exe=""
msys=""

case `command -p uname -a` in"#
        ),
        "header must track a Windows-form basedir for WSL2/Cygwin, body was:\n{body}",
    );
    // No test host reports itself as Cygwin, MSYS, or WSL2, so
    // `shim_execution_ignores_helpers_from_the_callers_path` can only decoy the
    // helpers outside the platform branch. This is what pins the rest.
    for helper in [
        "command -p readlink",
        "command -p sed",
        "command -p uname",
        "command -p cygpath",
        "command -p wslpath",
    ] {
        assert!(body.contains(helper), "the header must reach {helper}, body was:\n{body}");
    }
    assert!(!body.contains("dirname"), "the header must not fork dirname, body was:\n{body}");
    // POSIX echo processes `\n` / `\t` before sed can convert the backslashes.
    assert!(
        body.contains(
            r#"basedir=$(command -p printf '%s\n' "$link" | command -p sed -e 's,\\,/,g')"#
        ),
        "header must print $link with printf so a Windows-form path keeps its backslashes, body was:\n{body}",
    );
    assert!(
        !body.contains(r#"basedir=$(echo "$link""#),
        "header must not pipe $link through echo, body was:\n{body}",
    );
    // The header converts backslashes to slashes, so a Windows-form $0 is already
    // absolute and must not be prefixed with `./`, which would reroot it on the
    // working directory. Only a name with no separator at all came from a PATH
    // lookup. `@zkochan/cmd-shim` carries this same guard for pnpm 11's shims.
    assert!(
        body.contains(r"  */*|*\\*) ;;"),
        "the bare-name guard must count a backslash as a separator, body was:\n{body}",
    );
    assert!(
        body.contains(r#"basedir_win="$(wslpath -w "$basedir" 2> /dev/null)""#),
        "header must convert WSL2 basedir with wslpath, body was:\n{body}",
    );
    assert!(
        body.contains(r#"basedir_win=`cygpath -w "$basedir"`"#),
        "MSYS branch must only update the Windows-form basedir, body was:\n{body}",
    );
    assert!(
        !body.contains("basedir=`cygpath"),
        "MSYS branch must keep the POSIX basedir unchanged, body was:\n{body}",
    );
    assert!(
        body.contains("else\n        exe=\".exe\"\n      fi"),
        "WSL2 branch must enable .exe fallback only after wslpath succeeds, body was:\n{body}",
    );
    assert!(
        body.contains("if [ -n \"$exe\" ] && [ -x \"$basedir/node.exe\" ]; then\n  exec \"$basedir/node.exe\"  \"$basedir_win/../typescript/bin/tsc\" \"$@\"\nelif [ -x \"$basedir/node\" ]; then\n  exec \"$basedir/node\"  \"$basedir/../typescript/bin/tsc\" \"$@\"\nelif command -v node >/dev/null 2>&1; then\n  exec node  \"$basedir/../typescript/bin/tsc\" \"$@\"\nelif [ -n \"$exe\" ] && command -v node.exe >/dev/null 2>&1; then\n  exec node.exe  \"$basedir_win/../typescript/bin/tsc\" \"$@\"\nelse\n  exec node  \"$basedir/../typescript/bin/tsc\" \"$@\"\nfi\n"),
        "exec block must preserve the generated sh shim fallback order, body was:\n{body}",
    );
    assert!(
        body.ends_with("# cmd-shim-target=/proj/node_modules/typescript/bin/tsc\n"),
        "trailing target marker is required for is_shim_pointing_at parity",
    );
}

/// POSIX `echo` turns `\n` and `\t` into a newline and a tab, so a Windows-form
/// `$0` such as `C:\node_modules\.bin\tsc` is corrupted before `sed` runs.
#[test]
#[cfg_attr(not(unix), ignore = "the shim shebang is /bin/sh")]
fn posix_shim_header_normalizes_windows_backslash_paths_without_echo_escapes() {
    let target = Path::new("/proj/node_modules/typescript/bin/tsc");
    let shim = Path::new("/proj/node_modules/.bin/tsc");
    let body = generate_sh_shim(target, shim, None, &[]);
    let conversion = body
        .lines()
        .find(|line| line.starts_with("basedir=$("))
        .expect("header must assign basedir from the shim path");
    assert_eq!(conversion, SH_SHIM_PATH_PRINTF_LINE);

    let script =
        format!("link='C:\\node_modules\\.bin\\tsc'\n{conversion}\nprintf '%s' \"$basedir\"");
    let output = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .output()
        .expect("run the header's basedir conversion");
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let basedir = String::from_utf8_lossy(&output.stdout);
    assert_eq!(basedir, "C:/node_modules/.bin/tsc");
}

#[test]
fn is_shim_pointing_at_round_trips_through_marker() {
    let target = Path::new("/p/node_modules/typescript/bin/tsc");
    let shim = Path::new("/p/node_modules/.bin/tsc");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    let body = generate_sh_shim(target, shim, Some(&runtime), &[]);
    assert!(is_shim_pointing_at(&body, target));
    assert!(!is_shim_pointing_at(&body, Path::new("/elsewhere")));
}

#[test]
fn extension_program_covers_every_known_extension() {
    assert_eq!(extension_program("js"), Some("node"));
    assert_eq!(extension_program("cjs"), Some("node"));
    assert_eq!(extension_program("mjs"), Some("node"));
    assert_eq!(extension_program("cmd"), Some("cmd"));
    assert_eq!(extension_program("bat"), Some("cmd"));
    assert_eq!(extension_program("ps1"), Some("pwsh"));
    assert_eq!(extension_program("sh"), Some("sh"));
    assert_eq!(extension_program("unknown"), None);
    assert_eq!(extension_program(""), None);
}

#[test]
fn parse_shebang_returns_none_for_empty_prog() {
    assert!(parse_shebang("#!\t").is_none());
    assert!(parse_shebang("#!").is_none(), "empty line after #! must yield None");
    assert!(parse_shebang("not a shebang").is_none());
}

#[test]
fn parse_shebang_from_bytes_handles_crlf_and_lossy_utf8() {
    let bytes = b"#!/usr/bin/env node\r\nconsole.log('hi')\n";
    let rt = parse_shebang_from_bytes(bytes).expect("CRLF first line");
    assert_eq!(rt.prog.as_deref(), Some("node"));

    let mut bytes = Vec::from(*b"#!/usr/bin/env node\n");
    bytes.extend_from_slice(&[0xff, 0xfe, 0xfd]);
    let rt = parse_shebang_from_bytes(&bytes).expect("non-UTF-8 tail tolerated");
    assert_eq!(rt.prog.as_deref(), Some("node"));
}

#[test]
fn generate_sh_shim_emits_direct_exec_when_no_runtime() {
    let target = Path::new("/proj/node_modules/foo/bin/cli");
    let shim = Path::new("/proj/node_modules/.bin/cli");
    let body = generate_sh_shim(target, shim, None, &[]);
    assert!(
        body.contains("exec \"$basedir/../foo/bin/cli\"  \"$@\"\nexit $?\n"),
        "no-runtime arm must exec the target directly, body:\n{body}",
    );
    assert!(body.ends_with("# cmd-shim-target=/proj/node_modules/foo/bin/cli\n"));
}

#[test]
fn generate_sh_shim_threads_args_when_prog_is_none() {
    let target = Path::new("/p/cli");
    let shim = Path::new("/p/.bin/cli");
    let runtime = ScriptRuntime { prog: None, args: "--flag".to_string() };
    let body = generate_sh_shim(target, shim, Some(&runtime), &[]);
    assert!(
        body.contains("exec \"$basedir/../cli\" --flag \"$@\"\nexit $?\n"),
        "args must be threaded into the no-prog arm, body:\n{body}",
    );
}

/// Unix-only: a path like `/abs/elsewhere/cli` is "absolute" only on Unix.
/// On Windows, `Path::is_absolute()` requires a drive letter (e.g.
/// `C:\abs\...`), so the same input takes the relative branch. The shim
/// produced by pacquet is a `/bin/sh` script regardless of host platform,
/// but the absolute-vs-relative classification of bin paths is itself
/// platform-dependent. This test pins behavior on Unix only.
#[cfg(unix)]
#[test]
fn generate_sh_shim_uses_absolute_target_when_no_common_prefix() {
    // `relative_path_from` of two paths with no common root produces an
    // absolute-ish path that still starts with `/` once joined; force the
    // absolute branch by constructing a target that's absolute and a shim
    // whose parent is empty.
    let target = Path::new("/abs/elsewhere/cli");
    let shim = Path::new("local-shim");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    let body = generate_sh_shim(target, shim, Some(&runtime), &[]);
    assert!(
        body.contains(r#""/abs/elsewhere/cli""#),
        "absolute-target branch must skip $basedir prefix, body:\n{body}",
    );
}

#[test]
fn relative_target_collapses_to_dot_when_paths_share_dir() {
    let target = Path::new("/proj/.bin/cli");
    let shim = Path::new("/proj/.bin/wrapper");
    assert_eq!(relative_target(target, shim), "cli");
}

/// [`super::relative_path_from`] preserves a single leading `..` in the
/// target (the `out.push("..")` fallback fires when `out.pop()` returns
/// false on an empty buffer). Multiple consecutive leading `..`s aren't
/// tested because [`super::lexical_normalize`] collapses them. `PathBuf::pop`
/// does not treat a trailing `..` component as a parent reference, so the
/// second `..` pops the first. That edge case doesn't occur in pacquet's
/// production paths (which are always absolute under `<modules_dir>` or
/// `<virtual_store_dir>`), so we test only the single-`..` case where
/// the result is unambiguous.
///
/// Asserting the exact value catches a regression that returns the raw
/// target unchanged (`../shared/cli`). A weaker substring assertion
/// would pass for both correct and broken outputs.
#[test]
fn lexical_normalize_keeps_leading_parent_segments() {
    let target = Path::new("../shared/cli");
    let shim = Path::new("project/.bin/cli");
    let result = relative_target(target, shim);
    assert_eq!(result, "../../../shared/cli", "leading `..` must propagate");
}

/// [`lexical_normalize`] drops `.` (`CurDir`) components. This is a direct
/// test on the helper itself. The indirect test below pins the same
/// behavior at the `relative_target` level, but a direct assertion makes
/// the `CurDir` arm visible to coverage tooling that can't see through
/// inlined call chains.
#[test]
fn lexical_normalize_drops_curdir_segments_directly() {
    assert_eq!(lexical_normalize(Path::new("a/./b")), PathBuf::from("a/b"));
    assert_eq!(lexical_normalize(Path::new("./a/b")), PathBuf::from("a/b"));
    assert_eq!(lexical_normalize(Path::new("a/b/.")), PathBuf::from("a/b"));
    assert_eq!(lexical_normalize(Path::new("./.")), PathBuf::new());
}

#[test]
fn lexical_normalize_drops_curdir_components() {
    let with_dot = relative_target(Path::new("/p/foo/./cli"), Path::new("/p/.bin/x"));
    let without_dot = relative_target(Path::new("/p/foo/cli"), Path::new("/p/.bin/x"));
    assert_eq!(with_dot, without_dot);
}

#[test]
fn search_script_runtime_reads_shebang_from_real_file() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("script");
    std::fs::write(&path, "#!/usr/bin/env node\nbody\n").unwrap();
    let rt = search_script_runtime::<Host>(&path).unwrap().expect("runtime detected");
    assert_eq!(rt.prog.as_deref(), Some("node"));
}

#[test]
fn search_script_runtime_returns_none_for_missing_file() {
    let nonexistent = Path::new("/definitely/not/a/real/path/cli");
    assert_eq!(search_script_runtime::<Host>(nonexistent).unwrap(), None);
}

#[test]
fn search_script_runtime_falls_back_to_extension() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("script.js");
    std::fs::write(&path, "console.log('no shebang')\n").unwrap();
    let rt = search_script_runtime::<Host>(&path).unwrap().expect("extension fallback");
    assert_eq!(rt.prog.as_deref(), Some("node"));
}

#[test]
fn search_script_runtime_falls_back_to_cmd_with_c_switch() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();

    for filename in ["script.cmd", "script.bat"] {
        let path = tmp.path().join(filename);
        std::fs::write(&path, "echo off\r\n").unwrap();

        let rt = search_script_runtime::<Host>(&path).unwrap().expect("extension fallback");
        assert_eq!(rt.prog.as_deref(), Some("cmd"));
        assert_eq!(rt.args, "/C");
    }
}

#[test]
fn escape_msys_cmd_switches_escapes_only_standalone_cmd_switches() {
    assert_eq!(escape_msys_cmd_switches("/C"), "//C");
    assert_eq!(escape_msys_cmd_switches(" /c\t/K "), " //c\t//K ");
    assert_eq!(
        escape_msys_cmd_switches("--flag /Config path/C /C:bad"),
        "--flag /Config path/C /C:bad",
    );
}

#[test]
fn strip_exe_suffix_is_case_insensitive() {
    assert_eq!(strip_exe_suffix("cmd.exe"), Some("cmd"));
    assert_eq!(strip_exe_suffix("cmd.EXE"), Some("cmd"));
    assert_eq!(strip_exe_suffix("\u{e5}.exe"), Some("\u{e5}"));
    assert_eq!(strip_exe_suffix("node"), None);
    assert_eq!(strip_exe_suffix("\u{e5}\u{e5}x"), None);
}

#[test]
fn generate_sh_shim_uses_windows_target_only_for_exe_branches() {
    let target = Path::new("/proj/node_modules/foo/src.bat");
    let shim = Path::new("/proj/node_modules/.bin/foo");
    let runtime = ScriptRuntime { prog: Some("cmd".into()), args: "/C".into() };
    let body = generate_sh_shim(target, shim, Some(&runtime), &[]);

    assert!(
        body.contains("if [ -n \"$msys\" ]; then\n  if [ -n \"$exe\" ] && [ -x \"$basedir/cmd.exe\" ]; then\n    exec \"$basedir/cmd.exe\" //C \"$basedir_win/../foo/src.bat\" \"$@\"\n  elif [ -x \"$basedir/cmd\" ]; then\n    exec \"$basedir/cmd\" //C \"$basedir/../foo/src.bat\" \"$@\"\n  elif command -v cmd >/dev/null 2>&1; then\n    exec cmd //C \"$basedir/../foo/src.bat\" \"$@\"\n  elif [ -n \"$exe\" ] && command -v cmd.exe >/dev/null 2>&1; then\n    exec cmd.exe //C \"$basedir_win/../foo/src.bat\" \"$@\"\n  else\n    exec cmd //C \"$basedir/../foo/src.bat\" \"$@\"\n  fi\nelse\n  if [ -n \"$exe\" ] && [ -x \"$basedir/cmd.exe\" ]; then\n    exec \"$basedir/cmd.exe\" /C \"$basedir_win/../foo/src.bat\" \"$@\"\n  elif [ -x \"$basedir/cmd\" ]; then\n    exec \"$basedir/cmd\" /C \"$basedir/../foo/src.bat\" \"$@\"\n  elif command -v cmd >/dev/null 2>&1; then\n    exec cmd /C \"$basedir/../foo/src.bat\" \"$@\"\n  elif [ -n \"$exe\" ] && command -v cmd.exe >/dev/null 2>&1; then\n    exec cmd.exe /C \"$basedir_win/../foo/src.bat\" \"$@\"\n  else\n    exec cmd /C \"$basedir/../foo/src.bat\" \"$@\"\n  fi\nfi\n"),
        "cmd sh shim must escape switches only for MSYS and use Windows-form targets only for .exe execution branches, body was:\n{body}",
    );
}

#[test]
fn generate_sh_shim_checks_path_before_exe_fallback() {
    let target = Path::new("/proj/node_modules/foo/src.sh");
    let shim = Path::new("/proj/node_modules/.bin/foo");
    let runtime = ScriptRuntime { prog: Some("sh".into()), args: String::new() };
    let body = generate_sh_shim(target, shim, Some(&runtime), &[]);

    assert!(
        body.contains("elif command -v sh >/dev/null 2>&1; then\n  exec sh  \"$basedir/../foo/src.sh\" \"$@\"\nelif [ -n \"$exe\" ] && command -v sh.exe >/dev/null 2>&1; then\n  exec sh.exe  \"$basedir_win/../foo/src.sh\" \"$@\"\nelse\n  exec sh  \"$basedir/../foo/src.sh\" \"$@\"\nfi\n"),
        "PATH fallback must prefer POSIX runtimes and gate .exe fallback, body was:\n{body}",
    );
}

#[test]
fn generate_sh_shim_does_not_append_exe_twice() {
    let target = Path::new("/proj/node_modules/foo/src.bat");
    let shim = Path::new("/proj/node_modules/.bin/foo");
    let runtime = ScriptRuntime { prog: Some("cmd.exe".into()), args: "/C".into() };
    let body = generate_sh_shim(target, shim, Some(&runtime), &[]);

    assert!(!body.contains("cmd.exe.exe"), "explicit .exe runtime must not double suffix:\n{body}");
    assert!(
        body.contains("if [ -n \"$msys\" ]; then\n  if [ -x \"$basedir/cmd.exe\" ]; then\n    exec \"$basedir/cmd.exe\" //C \"$basedir_win/../foo/src.bat\" \"$@\"\n  else\n    exec cmd.exe //C \"$basedir_win/../foo/src.bat\" \"$@\"\n  fi\nelse\n  if [ -x \"$basedir/cmd.exe\" ]; then\n    exec \"$basedir/cmd.exe\" /C \"$basedir_win/../foo/src.bat\" \"$@\"\n  else\n    exec cmd.exe /C \"$basedir_win/../foo/src.bat\" \"$@\"\n  fi\nfi\n"),
        "explicit .exe runtime must use Windows-form targets and escape switches only for MSYS, body was:\n{body}",
    );
}

#[test]
fn search_script_runtime_returns_none_when_runtime_unknown() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("script.unknown_ext");
    std::fs::write(&path, "no shebang here\n").unwrap();
    assert_eq!(search_script_runtime::<Host>(&path).unwrap(), None);
}

/// Real-fs can't trigger e.g. `PermissionDenied` portably, so plug a
/// fake [`FsReadHead`] per the DI principles in
/// <https://github.com/pnpm/pacquet/pull/332#issuecomment-4345054524>.
#[test]
fn search_script_runtime_propagates_non_not_found_io_errors() {
    struct PermissionDenied;
    impl FsReadHead for PermissionDenied {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }
    let err = search_script_runtime::<PermissionDenied>(Path::new("any"))
        .expect_err("non-NotFound IO error must propagate");
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn search_script_runtime_reads_zero_bytes_then_falls_through() {
    struct EmptyRead;
    impl FsReadHead for EmptyRead {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }
    let rt = search_script_runtime::<EmptyRead>(Path::new("/x.js")).unwrap().expect("ext fallback");
    assert_eq!(rt.prog.as_deref(), Some("node"));

    let rt = search_script_runtime::<EmptyRead>(Path::new("/x")).unwrap();
    assert_eq!(rt, None);
}

/// [`Host::read_head`](Host) is the production capability. Tests
/// that exercise it indirectly cover most paths; this one pins the
/// contract directly.
#[test]
fn real_fs_read_head_reads_up_to_buffer_size() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("data");
    std::fs::write(&path, "hello world").unwrap();
    let mut buf = [0u8; 1024];
    let read = Host::read_head(&path, 0, &mut buf).unwrap();
    assert_eq!(read, 11);
    assert_eq!(&buf[..read], b"hello world");
}

/// [`Host::read_head`](Host) propagates `NotFound` so the shebang reader can
/// distinguish a missing file from a real IO error and degrade to
/// `Ok(None)`.
#[test]
fn real_fs_read_head_propagates_not_found() {
    let mut buf = [0u8; 16];
    let err = Host::read_head(Path::new("/no/such/file"), 0, &mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

#[test]
fn read_head_filled_real_fs_long_file_fills_buffer() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("long");
    let payload: Vec<u8> = (0..1024)
        .map(|index| (index % 251) as u8)
        .collect();
    std::fs::write(&path, &payload).unwrap();

    let mut buf = [0u8; 256];
    let read = read_head_filled::<Host>(&path, &mut buf).unwrap();
    assert_eq!(read, 256);
    assert_eq!(&buf[..], &payload[..256]);
}

#[test]
fn read_head_filled_real_fs_short_file_returns_partial() {
    use tempfile::tempdir;
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("short");
    std::fs::write(&path, "#!/bin/sh\n").unwrap();

    let mut buf = [0u8; 256];
    let read = read_head_filled::<Host>(&path, &mut buf).unwrap();
    assert_eq!(read, 10);
    assert_eq!(&buf[..read], b"#!/bin/sh\n");
}

/// Pinning this with a fake is the only way to verify the loop
/// without a pseudo-fs to test against: real filesystems essentially
/// never return short reads at offset 0.
#[test]
fn read_head_filled_accumulates_short_reads_from_fake() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Tracks the offsets each call sees, plus how many bytes the
    /// fake produces per call. We deliver the input slice to the
    /// caller `chunk_size` bytes at a time so the loop must run
    /// multiple iterations to fill its buffer.
    static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);
    static LAST_OFFSETS: [AtomicUsize; 4] = [
        AtomicUsize::new(usize::MAX),
        AtomicUsize::new(usize::MAX),
        AtomicUsize::new(usize::MAX),
        AtomicUsize::new(usize::MAX),
    ];
    const PAYLOAD: &[u8] = b"abcdefghij"; // 10 bytes
    const CHUNK_SIZE: usize = 3;

    struct ShortReader;
    impl FsReadHead for ShortReader {
        fn read_head(_: &Path, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
            let call_index = CALL_COUNT.fetch_add(1, Ordering::Relaxed);
            if call_index < LAST_OFFSETS.len() {
                LAST_OFFSETS[call_index].store(offset as usize, Ordering::Relaxed);
            }
            let off = offset as usize;
            if off >= PAYLOAD.len() {
                return Ok(0); // EOF
            }
            let remaining = &PAYLOAD[off..];
            let take = remaining
                .len()
                .min(buf.len())
                .min(CHUNK_SIZE);
            buf[..take].copy_from_slice(&remaining[..take]);
            Ok(take)
        }
    }

    let mut buf = [0u8; 8];
    let read = read_head_filled::<ShortReader>(Path::new("any"), &mut buf).unwrap();
    assert_eq!(read, 8, "loop must accumulate short reads to fill the buffer");
    assert_eq!(&buf[..], b"abcdefgh");

    assert_eq!(CALL_COUNT.load(Ordering::Relaxed), 3);
    assert_eq!(LAST_OFFSETS[0].load(Ordering::Relaxed), 0);
    assert_eq!(LAST_OFFSETS[1].load(Ordering::Relaxed), 3);
    assert_eq!(LAST_OFFSETS[2].load(Ordering::Relaxed), 6);
}

#[test]
fn read_head_filled_terminates_on_zero_byte_read_from_fake() {
    struct EofAfterOne;
    impl FsReadHead for EofAfterOne {
        fn read_head(_: &Path, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
            if offset == 0 && !buf.is_empty() {
                buf[0] = b'X';
                Ok(1)
            } else {
                Ok(0) // EOF on subsequent calls
            }
        }
    }

    let mut buf = [0u8; 16];
    let read = read_head_filled::<EofAfterOne>(Path::new("any"), &mut buf).unwrap();
    assert_eq!(read, 1, "loop must stop on EOF, returning the partial count");
    assert_eq!(buf[0], b'X');
}

#[test]
fn read_head_filled_propagates_io_error_from_fake() {
    struct AlwaysErrors;
    impl FsReadHead for AlwaysErrors {
        fn read_head(_: &Path, _: u64, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
    }

    let mut buf = [0u8; 16];
    let err = read_head_filled::<AlwaysErrors>(Path::new("any"), &mut buf).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn generate_cmd_shim_matches_pnpm_template() {
    let target = Path::new("/proj/node_modules/typescript/bin/tsc");
    let shim = Path::new("/proj/node_modules/.bin/tsc.cmd");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    let body = generate_cmd_shim(target, shim, Some(&runtime), &[]);

    assert!(body.starts_with("@SETLOCAL\r\n"), "must start with @SETLOCAL CRLF");
    assert!(
        body.contains("@IF EXIST \"%~dp0\\node.exe\" (\r\n  \"%~dp0\\node.exe\"  \"%~dp0\\..\\typescript\\bin\\tsc\" %*\r\n) ELSE (\r\n  @SET PATHEXT=%PATHEXT:;.JS;=;%\r\n  node  \"%~dp0\\..\\typescript\\bin\\tsc\" %*\r\n)\r\n"),
        "exec block must match pnpm's generateCmdShim template, body was:\n{body}",
    );
}

#[test]
fn generate_cmd_shim_emits_direct_exec_when_no_runtime() {
    let target = Path::new("/p/cli");
    let shim = Path::new("/p/.bin/cli.cmd");
    let body = generate_cmd_shim(target, shim, None, &[]);
    assert!(
        body.contains(r#"@"%~dp0\..\cli""#),
        "no-runtime arm must exec the target directly, body:\n{body}",
    );
}

#[test]
fn generate_pwsh_shim_matches_pnpm_template() {
    let target = Path::new("/proj/node_modules/typescript/bin/tsc");
    let shim = Path::new("/proj/node_modules/.bin/tsc.ps1");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    let body = generate_pwsh_shim(target, shim, Some(&runtime), &[]);

    assert!(body.starts_with("#!/usr/bin/env pwsh\n"), "ps1 shim must start with pwsh shebang");
    assert!(
        body.contains("$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent"),
        "must declare $basedir from MyInvocation",
    );
    assert!(body.contains(r#"$exe=".exe""#), "Windows-detection branch must set $exe to .exe");
    assert!(
        body.contains(
            "if (Test-Path \"$basedir/node$exe\") {\n  # Support pipeline input\n  if ($MyInvocation.ExpectingInput) {\n    $input | & \"$basedir/node$exe\"  \"$basedir/../typescript/bin/tsc\" $args\n  } else {\n    & \"$basedir/node$exe\"  \"$basedir/../typescript/bin/tsc\" $args\n  }",
        ),
        "exec-with-basedir-prog block must match pnpm's generatePwshShim template, body was:\n{body}",
    );
    assert!(body.ends_with("exit $ret\n"));
}

#[test]
fn generate_pwsh_shim_emits_direct_exec_when_no_runtime() {
    let target = Path::new("/p/cli");
    let shim = Path::new("/p/.bin/cli.ps1");
    let body = generate_pwsh_shim(target, shim, None, &[]);
    assert!(
        body.contains(r#"& "$basedir/../cli""#),
        "no-runtime arm must exec the target directly, body:\n{body}",
    );
    assert!(body.ends_with("exit $LASTEXITCODE\n"));
}

/// The shell sets `$0` to the invoked symlink, not the shim it points at,
/// so a shim reached through external symlinks must follow the chain
/// before deriving `basedir` (<https://github.com/pnpm/pnpm/issues/13405>).
#[cfg(unix)]
#[test]
fn shim_execution_resolves_symlink_chain() {
    use std::{fs, os::unix::fs::symlink, process::Command};
    use tempfile::tempdir;

    let tmp = tempdir().unwrap();
    let tmp_path = tmp.path();

    let bin_dir = tmp_path.join("node_modules").join(".bin");
    let target_dir = tmp_path
        .join("node_modules")
        .join("typescript")
        .join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&target_dir).unwrap();

    let target_path = target_dir.join("tsc");
    write_executable(&target_path, "#!/bin/sh\necho \"tsc-output\"\n");

    let shim_path = bin_dir.join("tsc");
    write_executable(&shim_path, &generate_sh_shim(&target_path, &shim_path, None, &[]));

    // hop2's relative target exercises the shim's directory-composition
    // branch; hop1's absolute target exercises the other.
    let hop1 = tmp_path.join("symlink_hop_1");
    symlink(&shim_path, &hop1).unwrap();
    let hop2_dir = tmp_path.join("local").join("bin");
    fs::create_dir_all(&hop2_dir).unwrap();
    let hop2 = hop2_dir.join("tsc");
    symlink("../../symlink_hop_1", &hop2).unwrap();

    let output = Command::new(&hop2).output().expect("execute shim through symlink chain");
    assert!(
        output.status.success(),
        "Shim execution failed: {:?}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tsc-output"), "Unexpected stdout: {stdout}");
}

/// A shim runs with `node_modules/.bin` at the front of `PATH`, which is where a
/// dependency's own bins live, so a helper taken from there could report any
/// directory it liked and redirect what the shim finally execs.
#[cfg(unix)]
#[test]
fn shim_execution_ignores_helpers_from_the_callers_path() {
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = plant_shimmed_tool(tmp.path());
    let mut command = std::process::Command::new(bin_dir.join("tsc-link"));
    assert_shim_reaches_its_target(tmp.path(), &mut command);
}

/// The kernel and `execvp` hand the interpreter the path they resolved, so `$0`
/// is bare only when a shell is given the name itself.
#[cfg(unix)]
#[test]
fn shim_execution_normalizes_a_bare_name() {
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = plant_shimmed_tool(tmp.path());
    let mut command = std::process::Command::new("sh");
    command.arg("tsc-link").current_dir(&bin_dir);
    assert_shim_reaches_its_target(tmp.path(), &mut command);
}

/// A shimmed tool plus a relative symlink to it in the same directory, so the
/// walk composes a directory with the link target instead of taking one
/// straight from `readlink`. Returns the bin directory.
#[cfg(unix)]
fn plant_shimmed_tool(root: &Path) -> PathBuf {
    let bin_dir = root.join("node_modules").join(".bin");
    let target = root
        .join("node_modules")
        .join("typescript")
        .join("bin")
        .join("tsc.js");
    std::fs::create_dir_all(&bin_dir).unwrap();
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "console.log('tsc-output')\n").unwrap();
    // A dependency can declare a bin named `node.exe`, and the shim's basedir is
    // the directory those bins land in. Only a lying `uname` reaches it.
    write_executable(&bin_dir.join("node.exe"), "#!/bin/sh\necho hijacked\n");

    let shim = bin_dir.join("tsc");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    write_executable(&shim, &generate_sh_shim(&target, &shim, Some(&runtime), &[]));
    std::os::unix::fs::symlink("tsc", bin_dir.join("tsc-link")).unwrap();
    bin_dir
}

/// Run `command` with the decoys first on `PATH` and require the real target's
/// output.
#[cfg(unix)]
fn assert_shim_reaches_its_target(root: &Path, command: &mut std::process::Command) {
    let decoy_dir = plant_hijack_tree_and_decoys(root);
    let path = format!("{}:{}", decoy_dir.display(), std::env::var("PATH").unwrap_or_default());
    let output = command
        .env("PATH", path)
        .output()
        .expect("run the shim");

    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim_end(), "tsc-output", "the shim took a helper from the caller's PATH");
}

/// Write the tree the decoys point at, and the decoys, returning the directory to
/// put at the front of `PATH`. Each decoy answers with what its real counterpart
/// would be asked for, so any one of them alone is enough to redirect the shim.
#[cfg(unix)]
fn plant_hijack_tree_and_decoys(root: &Path) -> PathBuf {
    let hijack = root.join("hijack").join("node_modules");
    let hijack_bin = hijack.join(".bin");
    let hijack_target = hijack
        .join("typescript")
        .join("bin")
        .join("tsc.js");
    std::fs::create_dir_all(&hijack_bin).unwrap();
    std::fs::create_dir_all(hijack_target.parent().unwrap()).unwrap();
    std::fs::write(&hijack_target, "console.log('hijacked')\n").unwrap();

    let decoy_dir = root.join("decoy");
    std::fs::create_dir_all(&decoy_dir).unwrap();
    let answer = |path: &Path| format!("#!/bin/sh\necho '{}'\n", path.display());
    for helper in ["readlink", "sed", "printf"] {
        write_executable(&decoy_dir.join(helper), &answer(&hijack_bin.join("tsc")));
    }
    write_executable(&decoy_dir.join("dirname"), &answer(&hijack_bin));
    write_executable(&decoy_dir.join("uname"), "#!/bin/sh\necho MINGW64_NT-10.0\n");
    decoy_dir
}

/// Cygwin, MSYS, and WSL2 convert `$basedir` to a Windows path through a helper
/// a dependency can also ship. The system copy answers first, and when none
/// does the shim falls back to the caller's `PATH` instead of giving up.
#[cfg(unix)]
#[test]
fn the_platform_branch_prefers_the_system_path_converter_and_still_falls_back() {
    let tmp = tempfile::tempdir().unwrap();
    let target = Path::new("/proj/node_modules/typescript/bin/tsc");
    let shim = Path::new("/proj/node_modules/.bin/tsc");
    let body = generate_sh_shim(target, shim, None, &[]);

    let answering = tmp.path().join("answering");
    write_executable(&answering, "#!/bin/sh\necho '/system/win'\n");
    let silent = tmp.path().join("silent");
    write_executable(&silent, "#!/bin/sh\n");
    let absent = tmp.path().join("absent");
    let decoys = tmp.path().join("decoy");
    std::fs::create_dir_all(&decoys).unwrap();
    for helper in ["cygpath", "wslpath"] {
        write_executable(&decoys.join(helper), "#!/bin/sh\necho '/decoy/win'\n");
    }

    for uname in ["MINGW64_NT-10.0", "Linux 5.15.0 WSL2"] {
        assert_eq!(
            run_platform_branch(&body, uname, &answering, &decoys),
            ("/system/win".to_owned(), ".exe".to_owned()),
            "{uname}: the system converter must win over the one on PATH",
        );
        assert_eq!(
            run_platform_branch(&body, uname, &silent, &decoys),
            ("/decoy/win".to_owned(), ".exe".to_owned()),
            "{uname}: an empty answer from the system converter must fall back to PATH",
        );
        assert_eq!(
            run_platform_branch(&body, uname, &absent, &decoys),
            ("/decoy/win".to_owned(), ".exe".to_owned()),
            "{uname}: no system converter must fall back to PATH",
        );
    }
    assert_eq!(
        run_platform_branch(&body, "MINGW64_NT-10.0", &absent, Path::new("")),
        (BRANCH_BASEDIR.to_owned(), ".exe".to_owned()),
        "MSYS with no converter at all must keep the POSIX basedir instead of failing",
    );
    assert_eq!(
        run_platform_branch(&body, "Linux 5.15.0 WSL2", &absent, Path::new("")),
        (BRANCH_BASEDIR.to_owned(), String::new()),
        "WSL2 with no converter at all must not claim a Windows exe",
    );
}

/// The POSIX directory the platform branch under test converts.
#[cfg(unix)]
const BRANCH_BASEDIR: &str = "/proj/node_modules/.bin";

/// Runs the header's platform branch, lifted out of `body` so the test drives
/// the text pnpm writes, and reports the `basedir_win` and `exe` it leaves
/// behind. No test host reports itself as Cygwin or WSL2, and `command -p`
/// searches the system default path, which a test cannot plant into, so the
/// `uname` and the two converters are the one thing rewritten here:
/// `system_converter` stands in for what `command -p` would reach and
/// `callers_path` for what the fallback finds. That the real header reaches
/// them through `command -p` is what
/// [`generate_sh_shim_matches_pnpm_typical_case`] pins.
#[cfg(unix)]
fn run_platform_branch(
    body: &str,
    uname: &str,
    system_converter: &Path,
    callers_path: &Path,
) -> (String, String) {
    const CASE_HEAD: &str = "case `command -p uname -a` in";
    let start = body.find(CASE_HEAD).expect("the header must select a platform");
    let end = start
        + body[start..].find("\nesac\n").expect("the platform branch must close")
        + "\nesac\n".len();
    let branch = body[start..end]
        .replace("`command -p uname -a`", r#""$fake_uname""#)
        .replace("command -p cygpath", r#""$system_converter""#)
        .replace("command -p wslpath", r#""$system_converter""#);
    let script = format!(
        "basedir={BRANCH_BASEDIR}\nbasedir_win=\"$basedir\"\nexe=\"\"\nmsys=\"\"\n{branch}\nprintf '%s\\n%s' \"$basedir_win\" \"$exe\"\n",
    );

    let output = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .env("fake_uname", uname)
        .env("system_converter", system_converter)
        .env("PATH", callers_path)
        .output()
        .expect("run the header's platform branch");
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (basedir_win, exe) = stdout
        .split_once('\n')
        .expect("the branch must report a Windows-form basedir and an exe suffix");
    (basedir_win.to_owned(), exe.to_owned())
}

#[cfg(unix)]
fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A waiting shell would surface the death as exit code 128+N, which the
/// caller cannot tell from an ordinary exit.
#[cfg(unix)]
#[test]
fn a_shim_lets_the_targets_signal_death_reach_the_caller() {
    use std::{
        os::unix::{fs::PermissionsExt, process::ExitStatusExt},
        process::Command,
    };

    // A real executable, so the shim takes the no-interpreter arm the way
    // a managed runtime binary does. A script would carry a shebang and be
    // launched through its interpreter instead.
    let dir = tempfile::tempdir().expect("create a temporary directory");
    let target = dir.path().join("target");
    std::fs::copy("/bin/sh", &target).expect("copy /bin/sh");

    let shim = dir.path().join("shim");
    let body = generate_sh_shim(&target, &shim, None, &[]);
    std::fs::write(&shim, body).expect("write the shim");
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))
        .expect("make the shim executable");

    let status = Command::new(&shim)
        .args(["-c", "kill -9 $$"])
        .status()
        .expect("run the target through the shim");

    assert_eq!(status.signal(), Some(9), "the shim swallowed the signal, reporting {status:?}");
    assert_eq!(status.code(), None);
}
