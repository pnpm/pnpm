// Copyright 2018-2025 the Deno authors. MIT license.

use futures::{
    FutureExt,
    future::LocalBoxFuture,
};
use std::{
    ffi::{
        OsStr,
        OsString,
    },
    path::Path,
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

pub struct MkdirCommand;

impl ShellCommand for MkdirCommand {
    fn execute(&self, context: ShellCommandContext) -> LocalBoxFuture<'static, ExecuteResult> {
        async move {
            execute_with_cancellation!(
                mkdir_command(context.state.cwd(), &context.args, context.stderr),
                context.state.kill_signal()
            )
        }
        .boxed_local()
    }
}

async fn mkdir_command(
    cwd: &Path,
    args: &[OsString],
    mut stderr: ShellPipeWriter,
) -> ExecuteResult {
    match execute_mkdir(cwd, args).await {
        Ok(()) => ExecuteResult::from_exit_code(0),
        Err(err) => {
            let _ = stderr.write_line(&format!("mkdir: {err}"));
            ExecuteResult::from_exit_code(1)
        }
    }
}

async fn execute_mkdir(cwd: &Path, args: &[OsString]) -> Result<(), ShellCommandError> {
    let flags = parse_args(args)?;
    for specified_path in flags.paths {
        let path = cwd.join(specified_path);
        if path.is_file() || !flags.parents && path.is_dir() {
            bail!("cannot create directory '{}': File exists", specified_path.to_string_lossy());
        }
        if flags.parents {
            if let Err(err) = tokio::fs::create_dir_all(&path).await {
                bail!("cannot create directory '{}': {}", specified_path.to_string_lossy(), err);
            }
        } else if let Err(err) = tokio::fs::create_dir(&path).await {
            bail!("cannot create directory '{}': {}", specified_path.to_string_lossy(), err);
        }
    }
    Ok(())
}

#[derive(Default, Debug, PartialEq)]
struct MkdirFlags<'a> {
    parents: bool,
    paths: Vec<&'a OsStr>,
}

fn parse_args(args: &[OsString]) -> Result<MkdirFlags<'_>, ShellCommandError> {
    let mut result = MkdirFlags::default();

    for arg in parse_arg_kinds(args) {
        match arg {
            ArgKind::LongFlag("parents") | ArgKind::ShortFlag('p') => {
                result.parents = true;
            }
            ArgKind::Arg(path) => {
                result.paths.push(path);
            }
            ArgKind::LongFlag(_) | ArgKind::ShortFlag(_) => arg.bail_unsupported()?,
        }
    }

    if result.paths.is_empty() {
        bail!("missing operand");
    }

    Ok(result)
}

#[cfg(test)]
mod test {
    use tempfile::tempdir;

    use super::*;
    use std::fs;

    #[test]
    fn parses_args() {
        assert_eq!(
            parse_args(&["--parents".into(), "a".into(), "b".into(),]).unwrap(),
            MkdirFlags { parents: true, paths: vec![OsStr::new("a"), OsStr::new("b")] }
        );
        assert_eq!(
            parse_args(&["-p".into(), "a".into(), "b".into(),]).unwrap(),
            MkdirFlags { parents: true, paths: vec![OsStr::new("a"), OsStr::new("b")] }
        );
        assert_eq!(
            parse_args(&["--parents".into()])
                .err()
                .unwrap()
                .to_string(),
            "missing operand",
        );
        assert_eq!(
            parse_args(&["--parents".into(), "-p".into(), "-u".into(), "a".into(),])
                .err()
                .unwrap()
                .to_string(),
            "unsupported flag: -u",
        );
        assert_eq!(
            parse_args(&["--parents".into(), "--random-flag".into(), "a".into(),])
                .err()
                .unwrap()
                .to_string(),
            "unsupported flag: --random-flag",
        );
    }

    #[tokio::test]
    async fn test_creates() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("file.txt");
        let sub_dir_path = dir.path().join("folder");
        fs::write(&file_path, "").unwrap();
        fs::create_dir(sub_dir_path).unwrap();

        assert_eq!(
            execute_mkdir(dir.path(), &["file.txt".into()],).await
                .err()
                .unwrap()
                .to_string(),
            "cannot create directory 'file.txt': File exists"
        );

        assert_eq!(
            execute_mkdir(dir.path(), &["-p".into(), "file.txt".into()],).await
                .err()
                .unwrap()
                .to_string(),
            "cannot create directory 'file.txt': File exists"
        );

        assert_eq!(
            execute_mkdir(dir.path(), &["folder".into()],).await
                .err()
                .unwrap()
                .to_string(),
            "cannot create directory 'folder': File exists"
        );

        // should work because of -p
        execute_mkdir(dir.path(), &["-p".into(), "folder".into()]).await.unwrap();

        execute_mkdir(dir.path(), &["other".into()]).await.unwrap();
        assert!(dir.path().join("other").exists());

        // sub folder
        assert_eq!(
            execute_mkdir(dir.path(), &["sub/folder".into()],).await
                .err()
                .unwrap()
                .to_string(),
            format!("cannot create directory 'sub/folder': {}", no_such_file_error_text())
        );

        execute_mkdir(dir.path(), &["-p".into(), "sub/folder".into()]).await.unwrap();
        assert!(
            dir.path()
                .join("sub")
                .join("folder")
                .exists()
        );
    }

    fn no_such_file_error_text() -> &'static str {
        if cfg!(windows) {
            "The system cannot find the path specified. (os error 3)"
        } else {
            "No such file or directory (os error 2)"
        }
    }
}
