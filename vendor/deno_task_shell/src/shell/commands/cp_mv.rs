// Copyright 2018-2025 the Deno authors. MIT license.

use std::{
    ffi::{
        OsStr,
        OsString,
    },
    path::{
        Path,
        PathBuf,
    },
};

use futures::{
    FutureExt,
    future::{
        BoxFuture,
        LocalBoxFuture,
    },
};

use crate::shell::types::{
    ExecuteResult,
    ShellPipeWriter,
};

use super::{
    ShellCommand,
    ShellCommandContext,
    args::{
        ArgKind,
        parse_arg_kinds,
    },
    error::{
        ShellCommandError,
        bail,
    },
    execute_with_cancellation,
};

pub struct CpCommand;

impl ShellCommand for CpCommand {
    fn execute(&self, context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        async move {
            execute_with_cancellation!(
                cp_command(context.state.cwd(), &context.args, context.stderr),
                context.state.kill_signal()
            )
        }
        .boxed_local()
    }
}

async fn cp_command(cwd: &Path, args: &[OsString], mut stderr: ShellPipeWriter) -> ExecuteResult {
    match execute_cp(cwd, args).await {
        Ok(()) => ExecuteResult::from_exit_code(0),
        Err(err) => {
            let _ = stderr.write_line(&format!("cp: {err}"));
            ExecuteResult::from_exit_code(1)
        }
    }
}

async fn execute_cp(cwd: &Path, args: &[OsString]) -> Result<(), ShellCommandError> {
    let flags = parse_cp_args(cwd, args)?;
    for (from, to) in &flags.operations {
        if let Err(err) = do_copy_operation(&flags, from, to).await {
            bail!(
                "could not copy {} to {}: {}",
                from.specified.to_string_lossy(),
                to.specified.to_string_lossy(),
                err
            );
        }
    }
    Ok(())
}

async fn do_copy_operation(
    flags: &CpFlags,
    from: &PathWithSpecified,
    to: &PathWithSpecified,
) -> Result<(), ShellCommandError> {
    if from.path.components().eq(to.path.components()) {
        // copying to the same path truncates the source's files
        bail!("source and destination are the same");
    }
    // These are racy with the file system, but that's ok.
    // They only exists to give better error messages.
    if from.path.is_dir() {
        if flags.recursive {
            if to.path.exists() && to.path.is_file() {
                bail!("destination was a file");
            } else if to.path.is_symlink() {
                bail!("no support for copying to symlinks")
            } else if from.path.is_symlink() {
                bail!("no support for copying from symlinks")
            } else {
                copy_dir_recursively(from.path.clone(), to.path.clone()).await?;
            }
        } else {
            bail!("source was a directory; maybe specify -r")
        }
    } else {
        tokio::fs::copy(&from.path, &to.path).await?;
    }
    Ok(())
}

fn copy_dir_recursively(
    from: PathBuf,
    to: PathBuf,
) -> BoxFuture<'static, Result<(), ShellCommandError>> {
    // recursive, so box it
    async move {
        tokio::fs::create_dir_all(&to).await
            .map_err(|err| ShellCommandError::new(format!("Creating {}: {}", to.display(), err)))?;
        let mut read_dir = tokio::fs::read_dir(&from).await
            .map_err(|err| {
                ShellCommandError::new(format!("Reading {}: {}", from.display(), err))
            })?;

        while let Some(entry) = read_dir.next_entry().await? {
            let file_type = entry.file_type().await?;
            let new_from = from.join(entry.file_name());
            let new_to = to.join(entry.file_name());

            if file_type.is_dir() {
                copy_dir_recursively(new_from.clone(), new_to.clone()).await
                    .map_err(|err| {
                        ShellCommandError::new(format!(
                            "Dir {} to {}: {}",
                            new_from.display(),
                            new_to.display(),
                            err
                        ))
                    })?;
            } else if file_type.is_file() {
                tokio::fs::copy(&new_from, &new_to).await
                    .map_err(|err| {
                        ShellCommandError::new(format!(
                            "Copying {} to {}: {}",
                            new_from.display(),
                            new_to.display(),
                            err
                        ))
                    })?;
            }
        }

        Ok(())
    }
    .boxed()
}

struct CpFlags {
    recursive: bool,
    operations: Vec<(PathWithSpecified, PathWithSpecified)>,
}

fn parse_cp_args(cwd: &Path, args: &[OsString]) -> Result<CpFlags, ShellCommandError> {
    let mut paths = Vec::new();
    let mut recursive = false;
    for arg in parse_arg_kinds(args) {
        match arg {
            ArgKind::Arg(arg) => {
                paths.push(arg);
            }
            ArgKind::LongFlag("recursive") | ArgKind::ShortFlag('r') | ArgKind::ShortFlag('R') => {
                recursive = true;
            }
            _ => arg.bail_unsupported()?,
        }
    }
    if paths.is_empty() {
        bail!("missing file operand");
    } else if paths.len() == 1 {
        bail!("missing destination file operand after '{}'", paths[0].to_string_lossy());
    }

    for from in &paths[..paths.len() - 1] {
        match last_specified_component(from) {
            // a trailing `..` would resolve the target to the destination's
            // parent directory, so refuse it like the guard in mv
            b".." => {
                bail!("cannot copy '{}': refusing to copy '..'", from.to_string_lossy());
            }
            // an empty final component means the source is the root (ex. `/`),
            // which has no name to place inside the destination
            b"" => {
                bail!(
                    "cannot copy '{}': the source has no final path component",
                    from.to_string_lossy()
                );
            }
            _ => {}
        }
    }
    Ok(CpFlags { recursive, operations: get_copy_and_move_operations(cwd, paths)? })
}

pub struct MvCommand;

impl ShellCommand for MvCommand {
    fn execute(&self, context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        async move {
            execute_with_cancellation!(
                mv_command(context.state.cwd(), &context.args, context.stderr),
                context.state.kill_signal()
            )
        }
        .boxed_local()
    }
}

async fn mv_command(cwd: &Path, args: &[OsString], mut stderr: ShellPipeWriter) -> ExecuteResult {
    match execute_mv(cwd, args).await {
        Ok(()) => ExecuteResult::from_exit_code(0),
        Err(err) => {
            let _ = stderr.write_line(&format!("mv: {err}"));
            ExecuteResult::from_exit_code(1)
        }
    }
}

async fn execute_mv(cwd: &Path, args: &[OsString]) -> Result<(), ShellCommandError> {
    let flags = parse_mv_args(cwd, args)?;
    for (from, to) in flags.operations {
        if let Err(err) = tokio::fs::rename(&from.path, &to.path).await {
            bail!(
                "could not move {} to {}: {}",
                from.specified.to_string_lossy(),
                to.specified.to_string_lossy(),
                err
            );
        }
    }
    Ok(())
}

struct MvFlags {
    operations: Vec<(PathWithSpecified, PathWithSpecified)>,
}

fn parse_mv_args(cwd: &Path, args: &[OsString]) -> Result<MvFlags, ShellCommandError> {
    let mut paths = Vec::new();
    for arg in parse_arg_kinds(args) {
        match arg {
            ArgKind::Arg(arg) => {
                paths.push(arg);
            }
            _ => arg.bail_unsupported()?,
        }
    }
    if paths.is_empty() {
        bail!("missing file operand");
    } else if paths.len() == 1 {
        bail!("missing destination file operand after '{}'", paths[0].to_string_lossy());
    }

    for from in &paths[..paths.len() - 1] {
        match last_specified_component(from) {
            // matches GNU mv, which errors renaming these (ex. `mv public/. dist`)
            b"." | b".." => {
                bail!("cannot move '{}': refusing to move '.' or '..'", from.to_string_lossy());
            }
            // an empty final component means the source is the root (ex. `/`),
            // which has no name to place inside the destination
            b"" => {
                bail!(
                    "cannot move '{}': the source has no final path component",
                    from.to_string_lossy()
                );
            }
            _ => {}
        }
    }
    Ok(MvFlags { operations: get_copy_and_move_operations(cwd, paths)? })
}

struct PathWithSpecified {
    path: PathBuf,
    specified: OsString,
}

fn get_copy_and_move_operations(
    cwd: &Path,
    mut paths: Vec<&OsStr>,
) -> Result<Vec<(PathWithSpecified, PathWithSpecified)>, ShellCommandError> {
    // copy and move share the same logic
    let specified_destination = paths.pop().unwrap();
    let destination = cwd.join(specified_destination);
    let from_args = paths;
    let mut operations = Vec::new();
    if from_args.len() > 1 {
        if !destination.is_dir() {
            bail!("target '{}' is not a directory", specified_destination.to_string_lossy());
        }
        for from in from_args {
            let from_path = cwd.join(from);
            let to_path = calculate_destination_path(&destination, from, &from_path);
            operations.push((
                PathWithSpecified { specified: from.into(), path: from_path },
                PathWithSpecified { specified: specified_destination.into(), path: to_path },
            ));
        }
    } else {
        let from_path = cwd.join(from_args[0]);
        let to_path = if destination.is_dir() {
            calculate_destination_path(&destination, from_args[0], &from_path)
        } else {
            destination
        };
        operations.push((
            PathWithSpecified { specified: from_args[0].into(), path: from_path },
            PathWithSpecified { specified: specified_destination.into(), path: to_path },
        ));
    }
    Ok(operations)
}

/// Calculates the path in the destination directory to copy or
/// move to, using the final component of the path as the user
/// specified it rather than the resolved path so that a trailing
/// `.` resolves to the destination itself like GNU cp
/// (ex. `cp -r public/. dist` copies the contents of `public`
/// into `dist`).
fn calculate_destination_path(
    destination: &Path,
    from_specified: &OsStr,
    from_path: &Path,
) -> PathBuf {
    // sources with no final component (a trailing `..` or the root) are
    // refused by both cp and mv before getting here, so `file_name()` is
    // `Some` in the fallback arm
    match last_specified_component(from_specified) {
        b"." => destination.to_path_buf(),
        _ => destination.join(from_path.file_name().unwrap()),
    }
}

fn last_specified_component(specified: &OsStr) -> &[u8] {
    fn is_separator(byte: &u8) -> bool {
        *byte == b'/' || (cfg!(windows) && *byte == b'\\')
    }

    let bytes = specified.as_encoded_bytes();
    let end = bytes
        .iter()
        .rposition(|b| !is_separator(b))
        .map(|i| i + 1)
        .unwrap_or(0);
    let start = bytes[..end]
        .iter()
        .rposition(is_separator)
        .map(|i| i + 1)
        .unwrap_or(0);
    &bytes[start..end]
}

#[cfg(test)]
mod test {
    use tempfile::tempdir;

    use super::*;
    use std::fs;

    #[tokio::test]
    async fn should_copy() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("file1.txt");
        let file2 = dir.path().join("file2.txt");
        fs::write(&file1, "test").unwrap();
        execute_cp(dir.path(), &["file1.txt".into(), "file2.txt".into()]).await.unwrap();
        assert!(file1.exists());
        assert!(file2.exists());

        let dest_dir = dir.path().join("dest");
        fs::create_dir(&dest_dir).unwrap();
        execute_cp(dir.path(), &["file1.txt".into(), "file2.txt".into(), "dest".into()])
            .await
            .unwrap();
        assert!(file1.exists());
        assert!(file2.exists());
        assert!(dest_dir.join("file1.txt").exists());
        assert!(dest_dir.join("file2.txt").exists());

        let new_file = dir.path().join("new.txt");
        fs::write(&new_file, "test").unwrap();
        execute_cp(dir.path(), &["new.txt".into(), "dest".into()]).await.unwrap();
        assert!(dest_dir.is_dir());
        assert!(new_file.exists());
        assert!(dest_dir.join("new.txt").exists());

        let result = execute_cp(
            dir.path(),
            &["file1.txt".into(), "file2.txt".into(), "non-existent".into()],
        )
        .await
        .err()
        .unwrap();
        assert_eq!(result.to_string(), "target 'non-existent' is not a directory");

        let result = execute_cp(dir.path(), &[]).await.err().unwrap();
        assert_eq!(result.to_string(), "missing file operand");

        let result = execute_cp(dir.path(), &["file1.txt".into()]).await.err().unwrap();
        assert_eq!(result.to_string(), "missing destination file operand after 'file1.txt'");

        // test recursive flag
        fs::create_dir_all(dest_dir.join("sub_dir")).unwrap();
        fs::write(dest_dir.join("sub_dir").join("sub.txt"), "test").unwrap();
        let dest_dir2 = dir.path().join("dest2");

        let result = execute_cp(dir.path(), &["dest".into(), "dest2".into()])
            .await
            .err()
            .unwrap();
        assert_eq!(
            result.to_string(),
            "could not copy dest to dest2: source was a directory; maybe specify -r"
        );
        assert!(!dest_dir2.exists());

        execute_cp(dir.path(), &["-r".into(), "dest".into(), "dest2".into()])
            .await
            .unwrap();
        assert!(dest_dir2.exists());
        assert!(dest_dir2.join("file1.txt").exists());
        assert!(dest_dir2.join("file2.txt").exists());
        assert!(
            dest_dir2
                .join("sub_dir")
                .join("sub.txt")
                .exists()
        );

        // copy again
        execute_cp(dir.path(), &["-r".into(), "dest".into(), "dest2".into()])
            .await
            .unwrap();

        // try copying to a file
        let result =
            execute_cp(dir.path(), &["-r".into(), "dest".into(), "dest2/file1.txt".into()])
                .await
                .err()
                .unwrap();
        assert_eq!(
            result.to_string(),
            "could not copy dest to dest2/file1.txt: destination was a file"
        )
    }

    #[tokio::test]
    async fn should_copy_trailing_dot_dir_contents() {
        // https://github.com/denoland/deno_task_shell/issues/176
        let dir = tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join("public")
                .join(".well-known"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join("public")
                .join("index.html"),
            "test",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join("public")
                .join(".well-known")
                .join("security.txt"),
            "test",
        )
        .unwrap();

        let dist_dir = dir.path().join("dist");
        fs::create_dir(&dist_dir).unwrap();
        execute_cp(dir.path(), &["-r".into(), "public/.".into(), "dist".into()])
            .await
            .unwrap();
        assert!(!dist_dir.join("public").exists());
        assert!(dist_dir.join("index.html").exists());
        assert!(
            dist_dir
                .join(".well-known")
                .join("security.txt")
                .exists()
        );

        // non-existent destination
        execute_cp(dir.path(), &["-r".into(), "public/.".into(), "dist2".into()])
            .await
            .unwrap();
        assert!(
            dir.path()
                .join("dist2")
                .join("index.html")
                .exists()
        );

        // "." as the entire source
        let dist3_dir = dir.path().join("dist3");
        fs::create_dir(&dist3_dir).unwrap();
        execute_cp(&dir.path().join("public"), &["-r".into(), ".".into(), "../dist3".into()])
            .await
            .unwrap();
        assert!(dist3_dir.join("index.html").exists());
        assert!(
            dist3_dir
                .join(".well-known")
                .join("security.txt")
                .exists()
        );

        // refuses to copy '..' since the target would resolve to the
        // destination's parent directory
        let result = execute_cp(dir.path(), &["-r".into(), "public/..".into(), "dist".into()])
            .await
            .err()
            .unwrap();
        assert_eq!(result.to_string(), "cannot copy 'public/..': refusing to copy '..'");
        let result = execute_cp(dir.path(), &["-r".into(), "..".into(), "dist".into()])
            .await
            .err()
            .unwrap();
        assert_eq!(result.to_string(), "cannot copy '..': refusing to copy '..'");

        // refuses to copy a directory to itself since that would
        // truncate its files
        let result = execute_cp(dir.path(), &["-r".into(), "public/.".into(), "public".into()])
            .await
            .err()
            .unwrap();
        assert_eq!(
            result.to_string(),
            "could not copy public/. to public: source and destination are the same"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("public")
                    .join("index.html")
            )
            .unwrap(),
            "test"
        );

        // same for copying a file to itself
        let result =
            execute_cp(dir.path(), &["public/index.html".into(), "public/index.html".into()])
                .await
                .err()
                .unwrap();
        assert_eq!(
            result.to_string(),
            "could not copy public/index.html to public/index.html: source and destination are the same"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("public")
                    .join("index.html")
            )
            .unwrap(),
            "test"
        );
    }

    #[tokio::test]
    async fn should_not_panic_with_root_source() {
        // a source whose last specified component is empty (ex. `/`) has no
        // name to place inside the destination and used to reach
        // `from_path.file_name().unwrap()`, which is `None` for the root, so
        // both cp and mv should error gracefully instead of panicking
        let dir = tempdir().unwrap();
        let dest_dir = dir.path().join("dest");
        fs::create_dir(&dest_dir).unwrap();

        let result = execute_cp(dir.path(), &["-r".into(), "/".into(), "dest".into()])
            .await
            .err()
            .unwrap();
        assert_eq!(result.to_string(), "cannot copy '/': the source has no final path component");

        let result = execute_mv(dir.path(), &["/".into(), "dest".into()]).await.err().unwrap();
        assert_eq!(result.to_string(), "cannot move '/': the source has no final path component");
    }

    #[tokio::test]
    async fn should_move() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("file1.txt");
        let file2 = dir.path().join("file2.txt");
        fs::write(&file1, "test").unwrap();
        execute_mv(dir.path(), &["file1.txt".into(), "file2.txt".into()]).await.unwrap();
        assert!(!file1.exists());
        assert!(file2.exists());

        let dest_dir = dir.path().join("dest");
        fs::write(&file1, "test").unwrap(); // recreate
        fs::create_dir(&dest_dir).unwrap();
        execute_mv(dir.path(), &["file1.txt".into(), "file2.txt".into(), "dest".into()])
            .await
            .unwrap();
        assert!(!file1.exists());
        assert!(!file2.exists());
        assert!(dest_dir.join("file1.txt").exists());
        assert!(dest_dir.join("file2.txt").exists());

        let new_file = dir.path().join("new.txt");
        fs::write(&new_file, "test").unwrap();
        execute_mv(dir.path(), &["new.txt".into(), "dest".into()]).await.unwrap();
        assert!(dest_dir.is_dir());
        assert!(!new_file.exists());
        assert!(dest_dir.join("new.txt").exists());

        let result = execute_mv(
            dir.path(),
            &["file1.txt".into(), "file2.txt".into(), "non-existent".into()],
        )
        .await
        .err()
        .unwrap();
        assert_eq!(result.to_string(), "target 'non-existent' is not a directory");

        let result = execute_mv(dir.path(), &[]).await.err().unwrap();
        assert_eq!(result.to_string(), "missing file operand");

        let result = execute_mv(dir.path(), &["file1.txt".into()]).await.err().unwrap();
        assert_eq!(result.to_string(), "missing destination file operand after 'file1.txt'");

        // refuses to move '.' or '..' like GNU mv
        let result = execute_mv(dir.path(), &["dest/.".into(), "dest2".into()])
            .await
            .err()
            .unwrap();
        assert_eq!(result.to_string(), "cannot move 'dest/.': refusing to move '.' or '..'");
        let result = execute_mv(dir.path(), &["..".into(), "dest2".into()])
            .await
            .err()
            .unwrap();
        assert_eq!(result.to_string(), "cannot move '..': refusing to move '.' or '..'");
        assert!(dest_dir.join("file1.txt").exists());
    }
}
