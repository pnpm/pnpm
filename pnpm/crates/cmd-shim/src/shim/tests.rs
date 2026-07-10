use super::{
    ScriptRuntime, escape_msys_cmd_switches, extension_program, generate_cmd_shim,
    generate_pwsh_shim, generate_sh_shim, is_shim_pointing_at, parse_shebang,
    parse_shebang_from_bytes, read_head_filled, relative_target, search_script_runtime,
    strip_exe_suffix,
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

#[test]
fn generate_sh_shim_matches_pnpm_typical_case() {
    let target = Path::new("/proj/node_modules/typescript/bin/tsc");
    let shim = Path::new("/proj/node_modules/.bin/tsc");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    let body = generate_sh_shim(target, shim, Some(&runtime));

    assert!(body.starts_with("#!/bin/sh\n"), "shebang must come first");
    assert!(
        body.contains(
            r#"basedir_win="$basedir"
exe=""
msys=""

case `uname -a` in"#
        ),
        "header must track a Windows-form basedir for WSL2/Cygwin, body was:\n{body}",
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

#[test]
fn is_shim_pointing_at_round_trips_through_marker() {
    let target = Path::new("/p/node_modules/typescript/bin/tsc");
    let shim = Path::new("/p/node_modules/.bin/tsc");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    let body = generate_sh_shim(target, shim, Some(&runtime));
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
    let body = generate_sh_shim(target, shim, None);
    assert!(
        body.contains("\"$basedir/../foo/bin/cli\"  \"$@\"\nexit $?\n"),
        "no-runtime arm must exec the target directly, body:\n{body}",
    );
    assert!(body.ends_with("# cmd-shim-target=/proj/node_modules/foo/bin/cli\n"));
}

#[test]
fn generate_sh_shim_threads_args_when_prog_is_none() {
    let target = Path::new("/p/cli");
    let shim = Path::new("/p/.bin/cli");
    let runtime = ScriptRuntime { prog: None, args: "--flag".to_string() };
    let body = generate_sh_shim(target, shim, Some(&runtime));
    assert!(
        body.contains("\"$basedir/../cli\" --flag \"$@\"\nexit $?\n"),
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
    let body = generate_sh_shim(target, shim, Some(&runtime));
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
    let body = generate_sh_shim(target, shim, Some(&runtime));

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
    let body = generate_sh_shim(target, shim, Some(&runtime));

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
    let body = generate_sh_shim(target, shim, Some(&runtime));

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
    let payload: Vec<u8> = (0..1024).map(|index| (index % 251) as u8).collect();
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
            let take = remaining.len().min(buf.len()).min(CHUNK_SIZE);
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
    let body = generate_cmd_shim(target, shim, Some(&runtime));

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
    let body = generate_cmd_shim(target, shim, None);
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
    let body = generate_pwsh_shim(target, shim, Some(&runtime));

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
    let body = generate_pwsh_shim(target, shim, None);
    assert!(
        body.contains(r#"& "$basedir/../cli""#),
        "no-runtime arm must exec the target directly, body:\n{body}",
    );
    assert!(body.ends_with("exit $LASTEXITCODE\n"));
}
