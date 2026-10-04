use super::{CmdShimBatch, ScriptRuntime, cmd_encoding::cmd_in_own_console, generate_cmd_shim};
use crate::shim::end_replaced_cmd_shim_batch;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Output,
};
use tempfile::{TempDir, tempdir};

/// A `pnpm.cmd` that runs `pnpm.exe` as a command of its batch, so cmd.exe is
/// still reading it while `pnpm.exe` runs.
const BATCH_KEPT_PNPM_CMD: &str =
    "@SETLOCAL\r\n@\"%~dp0\\..\\global\\v11\\abc\\node_modules\\pnpm\\pnpm.exe\"  %*\r\n";

fn batchless_pnpm_cmd() -> String {
    generate_cmd_shim(
        Path::new("/home/global/v11/def/node_modules/pnpm/pnpm.exe"),
        Path::new("/home/bin/pnpm.cmd"),
        None,
        &[],
        CmdShimBatch::EndedBeforeTarget,
    )
}

/// The offset of the line that ends the replaced batch, or `None` when the
/// shim has no such line.
fn end_of_replaced_batch(shim: &str) -> Option<usize> {
    shim.find("@EXIT /B %ERRORLEVEL%\r\n")
}

#[test]
fn ends_the_replaced_batch_where_cmd_reads_on_and_jumps_over_that_line() {
    let shim = batchless_pnpm_cmd();
    let laid_out = end_replaced_cmd_shim_batch(&shim, BATCH_KEPT_PNPM_CMD);
    assert_eq!(
        laid_out,
        format!(
            "@GOTO :pnpm\r\n\
             @REM cmd.exe may still be running the pnpm.cmd this fil\r\n\
             @EXIT /B %ERRORLEVEL%\r\n\
             :pnpm\r\n\
             {shim}",
        ),
    );
    assert_eq!(end_of_replaced_batch(&laid_out), Some(BATCH_KEPT_PNPM_CMD.len()));
}

#[test]
fn a_shim_that_ends_its_batch_is_replaced_as_is() {
    let shim = batchless_pnpm_cmd();
    let plain = generate_cmd_shim(
        Path::new("/home/global/v11/abc/node_modules/pnpm/pnpm.exe"),
        Path::new("/home/bin/pnpm.cmd"),
        None,
        &[],
        CmdShimBatch::EndedBeforeTarget,
    );
    assert_eq!(end_replaced_cmd_shim_batch(&shim, &plain), shim);

    let laid_out = end_replaced_cmd_shim_batch(&plain, BATCH_KEPT_PNPM_CMD);
    assert_eq!(end_replaced_cmd_shim_batch(&shim, &laid_out), shim);
}

/// The policy asks for a shim that ends its batch, but an interpreted target
/// gets one that keeps it. That shim is read to its end on every run, so a
/// layout over it would be laid out again by the next link.
#[test]
fn a_shim_that_keeps_its_batch_is_not_laid_out() {
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    let shim = generate_cmd_shim(
        Path::new("/home/global/v11/def/node_modules/pnpm/bin/pnpm.cjs"),
        Path::new("/home/bin/pnpm.cmd"),
        Some(&runtime),
        &[],
        CmdShimBatch::EndedBeforeTarget,
    );
    assert!(!shim.contains("#_undefined_#"), "{shim}");
    assert_eq!(end_replaced_cmd_shim_batch(&shim, BATCH_KEPT_PNPM_CMD), shim);
}

/// The marker that ends a batch counts only as a line of its own.
#[test]
fn a_comment_naming_the_batch_end_does_not_make_a_shim_end_its_batch() {
    let shim = batchless_pnpm_cmd();
    let replaced = format!(
        "@SETLOCAL\r\n@REM @GOTO #_undefined_# 2>NUL || is not run here\r\n{BATCH_KEPT_PNPM_CMD}",
    );
    let laid_out = end_replaced_cmd_shim_batch(&shim, &replaced);
    assert_eq!(end_of_replaced_batch(&laid_out), Some(replaced.len()));
}

#[test]
fn a_file_too_short_for_the_jump_is_replaced_as_is() {
    let shim = batchless_pnpm_cmd();
    for replaced in ["", "@x\r\n", "@GOTO :pnpm\r\n@\r\n"] {
        assert_eq!(end_replaced_cmd_shim_batch(&shim, replaced), shim, "{replaced:?}");
    }
}

#[test]
fn a_file_the_jump_fills_exactly_needs_no_padding() {
    let shim = batchless_pnpm_cmd();
    let laid_out = end_replaced_cmd_shim_batch(&shim, "@SETLOCAL  \r\n");
    assert_eq!(laid_out, format!("@GOTO :pnpm\r\n@EXIT /B %ERRORLEVEL%\r\n:pnpm\r\n{shim}"));
}

/// cmd.exe is past the target's line, not past the file, when the target
/// exits.
#[test]
fn blank_lines_after_the_target_do_not_move_where_cmd_reads_on() {
    let shim = batchless_pnpm_cmd();
    let replaced = format!("{BATCH_KEPT_PNPM_CMD}\r\n  \r\n");
    let laid_out = end_replaced_cmd_shim_batch(&shim, &replaced);
    assert_eq!(end_of_replaced_batch(&laid_out), Some(BATCH_KEPT_PNPM_CMD.len()));
}

#[test]
fn a_target_line_without_a_line_break_ends_the_file() {
    let shim = batchless_pnpm_cmd();
    let replaced = BATCH_KEPT_PNPM_CMD.trim_end_matches("\r\n");
    let laid_out = end_replaced_cmd_shim_batch(&shim, replaced);
    assert_eq!(end_of_replaced_batch(&laid_out), Some(replaced.len()));
}

#[test]
fn a_replaced_shim_that_switched_the_code_page_restores_it() {
    let shim = batchless_pnpm_cmd();
    let replaced = generate_cmd_shim(
        Path::new("/hömé/global/v11/abc/node_modules/pnpm/pnpm.exe"),
        Path::new("/bin/pnpm.cmd"),
        None,
        &[],
        CmdShimBatch::Kept,
    );
    let restore = replaced
        .find("@SET \"_PNPM_EXIT_CODE=%ERRORLEVEL%\"\r\n")
        .expect("a non-ASCII shim restores the code page after its target");
    assert!(replaced[..restore].ends_with("  %*\r\n"), "{replaced}");
    let laid_out = end_replaced_cmd_shim_batch(&shim, &replaced);
    // The replaced batch runs its own restore, with its target's exit code.
    assert!(laid_out[restore..].starts_with(&replaced[restore..]), "{laid_out}");
    assert_eq!(end_of_replaced_batch(&laid_out), None);
    assert!(laid_out.ends_with(&shim), "{laid_out}");
}

#[test]
fn padding_stays_under_the_line_length_cmd_reads() {
    let shim = batchless_pnpm_cmd();
    let replaced = format!("@SETLOCAL\r\n@\"{}\"  %*\r\n", "x".repeat(9000));
    let laid_out = end_replaced_cmd_shim_batch(&shim, &replaced);
    assert_eq!(end_of_replaced_batch(&laid_out), Some(replaced.len()));
    let padding = &laid_out["@GOTO :pnpm\r\n".len()..replaced.len()];
    for line in padding.split_inclusive("\r\n") {
        assert!(line.starts_with("@REM"), "{line:?}");
        assert!(line.ends_with("\r\n"), "{line:?}");
        assert!(line.len() <= 4000, "{} bytes", line.len());
    }
}

/// Every laid-out shim is CRLF-terminated batch text, as `.cmd` files are.
#[test]
fn the_laid_out_shim_has_crlf_line_endings_only() {
    let shim = batchless_pnpm_cmd();
    let laid_out = end_replaced_cmd_shim_batch(&shim, BATCH_KEPT_PNPM_CMD);
    assert!(
        !laid_out
            .replace("\r\n", "")
            .contains(['\r', '\n']),
        "{laid_out:?}",
    );
}

/// A batch-kept `shim.cmd` whose target replaces the file and exits 7, run
/// through cmd.exe. The target is cmd.exe itself, running `replace.cmd`, which
/// moves the prepared replacement into place the way pnpm does and logs the
/// run.
struct ReplacingShim {
    root: TempDir,
}

impl ReplacingShim {
    const EXIT_CODE: i32 = 7;

    fn prepare() -> Self {
        let root = tempdir().unwrap();
        let comspec = std::env::var("ComSpec").expect("ComSpec names cmd.exe");
        fs::write(root.path().join("shim.cmd"), format!("@SETLOCAL\r\n@\"{comspec}\"  %*\r\n"))
            .unwrap();
        fs::write(
            root.path().join("replace.cmd"),
            format!(
                "@move /y \"%~dp0new-shim.txt\" \"%~dp0shim.cmd\" >NUL\r\n\
                 @echo run>>\"%~dp0runs.log\"\r\n\
                 @exit /b {}\r\n",
                Self::EXIT_CODE,
            ),
        )
        .unwrap();
        ReplacingShim { root }
    }

    fn shim_path(&self) -> PathBuf {
        self.root.path().join("shim.cmd")
    }

    /// The shim's target, as a batchless shim at the shim's path.
    fn batchless_shim(&self) -> String {
        let comspec = std::env::var("ComSpec").expect("ComSpec names cmd.exe");
        generate_cmd_shim(
            Path::new(&comspec),
            &self.shim_path(),
            None,
            &[],
            CmdShimBatch::EndedBeforeTarget,
        )
    }

    /// Run `shim.cmd` through `cmd /c`, with `replacement` waiting to be moved
    /// over it by the target.
    fn run_replacing_with(&self, replacement: &str) -> Output {
        fs::write(self.root.path().join("new-shim.txt"), replacement).unwrap();
        let output = cmd_in_own_console()
            .args(["/d", "/c", "shim.cmd", "/d", "/c", "replace.cmd"])
            .current_dir(self.root.path())
            .output()
            .unwrap();
        eprintln!("{output:?}");
        output
    }

    fn runs(&self) -> usize {
        fs::read_to_string(self.root.path().join("runs.log"))
            .unwrap_or_default()
            .lines()
            .count()
    }
}

/// The behavior the layout exists for: cmd.exe reads on in the replaced file
/// from where the old one's target line ended, and finds the new shim's target
/// line there.
#[test]
#[cfg_attr(not(windows), ignore = "runs cmd.exe")]
fn cmd_runs_the_target_of_a_shim_that_replaced_a_batch_kept_one_again() {
    let shim = ReplacingShim::prepare();
    let output = shim.run_replacing_with(&shim.batchless_shim());
    assert_eq!(output.status.code(), Some(ReplacingShim::EXIT_CODE));
    assert_eq!(shim.runs(), 2, "cmd.exe must run the target again without the layout");
}

#[test]
#[cfg_attr(not(windows), ignore = "runs cmd.exe")]
fn cmd_ends_the_batch_of_a_replaced_batch_kept_shim_at_the_laid_out_line() {
    let shim = ReplacingShim::prepare();
    let replaced = fs::read_to_string(shim.shim_path()).unwrap();
    let laid_out = end_replaced_cmd_shim_batch(&shim.batchless_shim(), &replaced);
    assert_ne!(laid_out, shim.batchless_shim());

    let output = shim.run_replacing_with(&laid_out);
    assert_eq!(output.status.code(), Some(ReplacingShim::EXIT_CODE));
    assert_eq!(shim.runs(), 1, "cmd.exe ran the target again:\n{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(fs::read_to_string(shim.shim_path()).unwrap(), laid_out);

    // The laid-out shim, run from its start, jumps over the line that ended
    // the replaced batch, and ends its own batch before its target.
    let output = shim.run_replacing_with(&shim.batchless_shim());
    assert_eq!(output.status.code(), Some(ReplacingShim::EXIT_CODE));
    assert_eq!(shim.runs(), 2, "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}
